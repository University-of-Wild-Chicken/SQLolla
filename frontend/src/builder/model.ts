/**
 * Pure local draft builder. It renders an unvalidated convenience SQL preview
 * from typed cards; it is not a parser and does not create canonical IR.
 */
import type { CatalogModel, ColumnModel, TableModel } from "../analysis/types";

/** Scalar types the card builder can display faithfully. */
export type DraftType = "int32" | "int64" | "bool" | "text" | "null";

/** One explicitly typed numbered parameter in the draft. */
export interface DraftParameter {
  readonly number: number;
  readonly type: DraftType;
  readonly value: string;
  readonly nullType: Exclude<DraftType, "null">;
}

/** One filter card with a relative value source. */
export interface DraftFilter {
  readonly id: number;
  readonly tableId: string;
  readonly columnName: string;
  readonly operator: "eq" | "neq" | "lt" | "lte" | "gt" | "gte" | "is_null" | "is_not_null";
  readonly source: "literal" | "parameter";
  readonly parameterNumber: number;
  readonly literal: string;
  readonly literalType: DraftType;
  readonly literalNullType: Exclude<DraftType, "null">;
}

/** One join card for one additional table. */
export interface DraftJoin {
  readonly tableId: string;
  readonly kind: "inner" | "left";
  readonly leftTableId: string;
  readonly leftColumn: string;
  readonly rightColumn: string;
}

/** One selected table with an alias and a subset of catalog columns. */
export interface DraftTable {
  readonly tableId: string;
  readonly alias: string;
  readonly selectedColumns: readonly string[];
}

/** The complete client-only draft state. */
export interface DraftState {
  readonly catalog: CatalogModel;
  readonly tables: readonly DraftTable[];
  readonly filters: readonly DraftFilter[];
  readonly joins: readonly DraftJoin[];
  readonly parameters: readonly DraftParameter[];
  readonly limit: string;
  readonly intent: string;
  readonly nextId: number;
}

/** Actions that edit only the local draft and invalidation state. */
export type DraftAction =
  | { readonly type: "start"; readonly catalog: CatalogModel; readonly starterDefaults?: boolean }
  | { readonly type: "extend-catalog"; readonly catalog: CatalogModel }
  | { readonly type: "reset" }
  | { readonly type: "intent"; readonly intent: string }
  | { readonly type: "add-table"; readonly tableId: string }
  | { readonly type: "remove-table"; readonly tableId: string }
  | { readonly type: "toggle-column"; readonly tableId: string; readonly columnName: string }
  | { readonly type: "add-filter" }
  | { readonly type: "update-filter"; readonly id: number; readonly patch: Partial<DraftFilter> }
  | { readonly type: "remove-filter"; readonly id: number }
  | { readonly type: "update-join"; readonly tableId: string; readonly patch: Partial<DraftJoin> }
  | { readonly type: "limit"; readonly limit: string }
  | { readonly type: "add-parameter" }
  | {
      readonly type: "update-parameter";
      readonly number: number;
      readonly patch: Partial<DraftParameter>;
    };

/** A readable relational-plan step shown above the SQL preview. */
export interface PlanStep {
  readonly key: string;
  readonly title: string;
  readonly detail: string;
}

/** Return a selected draft table by catalog identity. */
export function draftTable(state: DraftState, tableId: string): DraftTable | undefined {
  return state.tables.find((table) => table.tableId === tableId);
}

/** Return a catalog table by identity. */
export function catalogTable(state: DraftState, tableId: string): TableModel | undefined {
  return state.catalog.tables[tableId];
}

/** Generate a readable, unique alias for a newly selected table. */
function uniqueAlias(table: TableModel, existing: readonly DraftTable[]): string {
  const base = (table.name.match(/[A-Za-z]/)?.[0] ?? "t").toLowerCase();
  const used = new Set(existing.map((item) => item.alias));
  for (let suffix = 1; suffix < 100; suffix += 1) {
    const candidate = suffix === 1 ? base : `${base}${suffix}`;
    if (!used.has(candidate)) return candidate;
  }
  return `t${existing.length + 1}`;
}

/** Derive a safe draft type from a catalog column. */
function draftType(column: ColumnModel | undefined): DraftType {
  return column && ["int32", "int64", "bool", "text"].includes(column.ty) ? (column.ty as DraftType) : "text";
}

/** Build an empty, explicitly schema-bound draft. */
function emptyDraft(catalog: CatalogModel): DraftState {
  return {
    catalog,
    tables: [],
    filters: [],
    joins: [],
    parameters: [],
    limit: "",
    intent: "",
    nextId: 2,
  };
}

/** Seed one filter for a preferred tenant column without semantic inference. */
function defaultFilter(table: TableModel, tenant: ColumnModel | undefined): readonly DraftFilter[] {
  if (!tenant) return [];
  return [
    {
      id: 1,
      tableId: table.key,
      columnName: tenant.name,
      operator: "eq",
      source: "parameter",
      parameterNumber: 1,
      literal: "",
      literalType: draftType(tenant),
      literalNullType: "int32",
    },
  ];
}

/** Build the default local draft for an explicit schema context. */
export function createDraft(catalog: CatalogModel, starterDefaults = true): DraftState {
  const table = catalog.tables["public.orders"] ?? Object.values(catalog.tables)[0];
  if (!table) return emptyDraft(catalog);
  const tenant = starterDefaults ? table.columns.find((column) => column.name === "tenant_id") : undefined;
  return {
    catalog,
    tables: [
      {
        tableId: table.key,
        alias: uniqueAlias(table, []),
        selectedColumns: table.columns.slice(0, 2).map((column) => column.name),
      },
    ],
    filters: defaultFilter(table, tenant),
    joins: [],
    parameters: tenant ? [{ number: 1, type: draftType(tenant), value: "7", nullType: "int32" }] : [],
    limit: starterDefaults ? "50" : "",
    intent: "",
    nextId: 2,
  };
}

/** Add a table and a join card when one is structurally required. */
function addTable(state: DraftState, tableId: string): DraftState {
  const table = state.catalog.tables[tableId];
  if (!table || state.tables.some((selected) => selected.tableId === tableId)) return state;
  const alias = uniqueAlias(table, state.tables);
  const nextTable = {
    tableId,
    alias,
    selectedColumns: table.columns.slice(0, 2).map((column) => column.name),
  };
  const joins =
    state.tables.length === 0
      ? []
      : [
          ...state.joins,
          {
            tableId,
            kind: "inner" as const,
            leftTableId: state.tables[0]!.tableId,
            leftColumn: state.tables[0]!.selectedColumns[0] ?? "",
            rightColumn: "",
          },
        ];
  return { ...state, tables: [...state.tables, nextTable], joins, nextId: state.nextId + 1 };
}

/** Remove a later table together with dependent filter and join cards. */
function removeTable(state: DraftState, tableId: string): DraftState {
  if (state.tables.length <= 1) return state;
  return {
    ...state,
    tables: state.tables.filter((table) => table.tableId !== tableId),
    filters: state.filters.filter((filter) => filter.tableId !== tableId),
    joins: state.joins.filter((join) => join.tableId !== tableId && join.leftTableId !== tableId),
  };
}

/** Toggle one selected projection column. */
function toggleColumn(state: DraftState, tableId: string, columnName: string): DraftState {
  return {
    ...state,
    tables: state.tables.map((table) => {
      if (table.tableId !== tableId) return table;
      const selected = table.selectedColumns.includes(columnName)
        ? table.selectedColumns.filter((name) => name !== columnName)
        : [...table.selectedColumns, columnName];
      return { ...table, selectedColumns: selected };
    }),
  };
}

/** Add a filter card using the first selected table/column when available. */
function addFilter(state: DraftState): DraftState {
  const table = state.tables[0];
  const catalogTable = table ? state.catalog.tables[table.tableId] : undefined;
  if (!table || !catalogTable) return state;
  const column = catalogTable.columns.find((candidate) => candidate.name === (table.selectedColumns[0] ?? "")) ?? catalogTable.columns[0];
  if (!column) return state;
  return {
    ...state,
    filters: [
      ...state.filters,
      {
        id: state.nextId,
        tableId: table.tableId,
        columnName: column.name,
        operator: "eq",
        source: "parameter",
        parameterNumber: state.parameters[0]?.number ?? 1,
        literal: "",
        literalType: draftType(column),
        literalNullType: "int32",
      },
    ],
    nextId: state.nextId + 1,
  };
}

/** Add a uniquely numbered typed parameter without renumbering existing tokens. */
function addParameter(state: DraftState): DraftState {
  const number = Math.max(0, ...state.parameters.map((parameter) => parameter.number)) + 1;
  return {
    ...state,
    parameters: [...state.parameters, { number, type: "int32", value: "", nullType: "int32" }],
  };
}

/** Update a join card in place. */
function updateJoin(state: DraftState, tableId: string, patch: Partial<DraftJoin>): DraftState {
  return {
    ...state,
    joins: state.joins.map((join) => (join.tableId === tableId ? { ...join, ...patch } : join)),
  };
}

/** Update one filter while preserving local parameter identity. */
function updateFilter(state: DraftState, id: number, patch: Partial<DraftFilter>): DraftState {
  return {
    ...state,
    filters: state.filters.map((filter) => (filter.id === id ? { ...filter, ...patch } : filter)),
  };
}

/** Remove one filter by local card identity. */
function removeFilter(state: DraftState, id: number): DraftState {
  return { ...state, filters: state.filters.filter((filter) => filter.id !== id) };
}

/** Update one numbered parameter without renumbering any existing identity. */
function updateParameter(state: DraftState, action: Extract<DraftAction, { type: "update-parameter" }>): DraftState {
  return {
    ...state,
    parameters: state.parameters.map((parameter) => (parameter.number === action.number ? { ...parameter, ...action.patch } : parameter)),
  };
}

/** Pure draft reducer. Every action produces a new unvalidated local draft. */
export function draftReducer(state: DraftState | null, action: DraftAction): DraftState | null {
  if (action.type === "start") return createDraft(action.catalog, action.starterDefaults);
  if (action.type === "reset") return null;
  if (!state) return state;
  if (action.type === "extend-catalog") return { ...state, catalog: action.catalog };
  if (action.type === "intent") return { ...state, intent: action.intent };
  if (action.type === "limit") return { ...state, limit: action.limit };
  switch (action.type) {
    case "add-table":
      return addTable(state, action.tableId);
    case "remove-table":
      return removeTable(state, action.tableId);
    case "toggle-column":
      return toggleColumn(state, action.tableId, action.columnName);
    case "add-filter":
      return addFilter(state);
    case "update-filter":
      return updateFilter(state, action.id, action.patch);
    case "remove-filter":
      return removeFilter(state, action.id);
    case "update-join":
      return updateJoin(state, action.tableId, action.patch);
    case "add-parameter":
      return addParameter(state);
    case "update-parameter":
      return updateParameter(state, action);
    default:
      return state;
  }
}
