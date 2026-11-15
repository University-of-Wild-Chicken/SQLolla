/**
 * Structural draft readiness and SQL preview rendering. This is deliberately
 * not SQL parsing or semantic compilation and never produces canonical evidence.
 */
import type { DraftFilter, DraftJoin, DraftParameter, DraftState, PlanStep } from "./model";

const OPERATOR_SQL: Readonly<Record<DraftFilter["operator"], string>> = {
  eq: "=",
  neq: "<>",
  lt: "<",
  lte: "<=",
  gt: ">",
  gte: ">=",
  is_null: "IS NULL",
  is_not_null: "IS NOT NULL",
};

/** Quote a schema/column/alias identifier for preview only. */
function quoteIdent(identifier: string): string {
  return /^[a-z_][a-z0-9_]*$/.test(identifier) ? identifier : `"${identifier.replaceAll('"', '""')}"`;
}

/** Render a typed literal with an explicit type for a draft preview. */
function renderLiteral(filter: DraftFilter): string {
  if (filter.literalType === "null") return renderTypedNull(filter.literalNullType);
  if (filter.literalType === "bool") return filter.literal.toLowerCase() === "true" ? "TRUE" : "FALSE";
  if (filter.literalType === "int32" || filter.literalType === "int64") return filter.literal;
  return `'${filter.literal.replaceAll("'", "''")}'`;
}

/** Render a typed NULL literal with an explicit SQL type. */
function renderTypedNull(type: Exclude<DraftFilter["literalType"], "null">): string {
  const sqlType = type === "int32" ? "integer" : type === "int64" ? "bigint" : type === "bool" ? "boolean" : "text";
  return `CAST(NULL AS ${sqlType})`;
}

/** Render one draft filter condition. */
function renderFilter(state: DraftState, filter: DraftFilter): string {
  const table = state.tables.find((candidate) => candidate.tableId === filter.tableId);
  if (!table) return "";
  const left = `${quoteIdent(table.alias)}.${quoteIdent(filter.columnName)}`;
  if (filter.operator === "is_null" || filter.operator === "is_not_null") {
    return `${left} ${OPERATOR_SQL[filter.operator]}`;
  }
  const right = filter.source === "parameter" ? `$${filter.parameterNumber}` : renderLiteral(filter);
  return `${left} ${OPERATOR_SQL[filter.operator]} ${right}`;
}

/** Check that a filter literal is renderable for its declared type. */
function literalError(filter: DraftFilter): string | null {
  if (filter.source !== "literal") return null;
  if (filter.literalType === "null") return null;
  if (filter.literalType === "int32" && !/^-?\d+$/.test(filter.literal)) return "Enter an integer filter value.";
  if (filter.literalType === "int64" && !/^-?\d+$/.test(filter.literal)) return "Enter an integer filter value.";
  if (filter.literalType === "bool" && !["true", "false"].includes(filter.literal.toLowerCase())) {
    return "Filter value must be TRUE or FALSE.";
  }
  return null;
}

/** Check that a parameter's typed value is syntactically renderable. */
function parameterError(parameter: DraftParameter): string | null {
  if (parameter.type === "null") return null;
  if (parameter.type === "int32" || parameter.type === "int64") {
    if (!/^-?\d+$/.test(parameter.value)) return `Parameter $${parameter.number} needs an integer value.`;
    const bits = parameter.type === "int32" ? 32n : 64n;
    const value = BigInt(parameter.value);
    if (value < -(1n << (bits - 1n)) || value >= 1n << (bits - 1n)) return `Parameter $${parameter.number} is outside ${parameter.type} range.`;
    return null;
  }
  if (parameter.type === "bool" && !["true", "false"].includes(parameter.value)) {
    return `Parameter $${parameter.number} needs TRUE or FALSE.`;
  }
  return null;
}

/** Report structural join gaps. */
function joinErrors(state: DraftState): readonly string[] {
  const errors: string[] = [];
  for (const join of state.joins) {
    const left = state.tables.find((table) => table.tableId === join.leftTableId);
    const right = state.tables.find((table) => table.tableId === join.tableId);
    if (!left || !right || !join.leftColumn || !join.rightColumn) {
      errors.push(`Complete the join for ${join.tableId}.`);
    }
  }
  for (const table of state.tables.slice(1)) {
    if (!state.joins.some((join) => join.tableId === table.tableId)) {
      errors.push(`Add a join for ${table.tableId}.`);
    }
  }
  return errors;
}

/** Report structural filter gaps and malformed literal values. */
function filterErrors(state: DraftState): readonly string[] {
  const errors: string[] = [];
  for (const filter of state.filters) {
    if (!state.tables.some((table) => table.tableId === filter.tableId)) {
      errors.push("A filter refers to an unselected table.");
    }
    if (!filter.columnName) errors.push("Choose a column for every filter.");
    if (filter.source === "parameter" && !state.parameters.some((parameter) => parameter.number === filter.parameterNumber)) {
      errors.push(`Filter parameter $${filter.parameterNumber} is missing.`);
    }
    const error = literalError(filter);
    if (error) errors.push(error);
  }
  return errors;
}

/** Report all structural gaps that prevent a SQL preview. */
export function draftErrors(state: DraftState): readonly string[] {
  if (state.tables.length === 0) return ["Choose at least one table."];
  const errors = state.tables.some((table) => table.selectedColumns.length > 0) ? [] : ["Choose at least one result column."];
  for (const table of state.tables) {
    if (!state.catalog.tables[table.tableId]) {
      errors.push(`Table ${table.tableId} is not in the schema context.`);
    }
  }
  errors.push(...joinErrors(state), ...filterErrors(state));
  for (const parameter of state.parameters) {
    const error = parameterError(parameter);
    if (error) errors.push(error);
  }
  if (state.limit && (!/^\d+$/.test(state.limit) || Number(state.limit) > 500)) {
    errors.push("Limit must be a whole number from 0 to 500, or blank.");
  }
  return errors;
}

/** Render the unvalidated SQL preview, or null while the draft is incomplete. */
export function renderDraftSql(state: DraftState): string | null {
  if (draftErrors(state).length > 0) return null;
  const projection = state.tables.flatMap((table) => table.selectedColumns.map((column) => `${quoteIdent(table.alias)}.${quoteIdent(column)}`));
  const first = state.tables[0];
  if (!first) return null;
  const lines = [
    `SELECT ${projection.join(", ")}`,
    `FROM ${quoteIdent(state.catalog.tables[first.tableId]?.schema ?? "public")}.${quoteIdent(state.catalog.tables[first.tableId]?.name ?? first.tableId)} AS ${quoteIdent(first.alias)}`,
  ];
  for (const join of state.joins) {
    const right = state.catalog.tables[join.tableId];
    const left = state.tables.find((table) => table.tableId === join.leftTableId);
    if (!right || !left) continue;
    lines.push(
      `${join.kind === "left" ? "LEFT JOIN" : "JOIN"} ${quoteIdent(right.schema)}.${quoteIdent(right.name)} AS ${quoteIdent(state.tables.find((table) => table.tableId === join.tableId)?.alias ?? right.name)}`,
      `ON ${quoteIdent(left.alias)}.${quoteIdent(join.leftColumn)} = ${quoteIdent(state.tables.find((table) => table.tableId === join.tableId)?.alias ?? right.name)}.${quoteIdent(join.rightColumn)}`,
    );
  }
  if (state.filters.length > 0) lines.push("WHERE", state.filters.map((filter) => `  ${renderFilter(state, filter)}`).join("\n  AND\n"));
  if (state.limit) lines.push(`LIMIT ${state.limit}`);
  return `${lines.join("\n")}\n`;
}

/** Build readable table and join steps. */
function relationSteps(state: DraftState): readonly PlanStep[] {
  const steps: PlanStep[] = state.tables.map((table) => ({
    key: `from-${table.tableId}`,
    title: `From ${table.tableId}`,
    detail: `Alias ${table.alias}. Columns: ${state.catalog.tables[table.tableId]?.columns.map((column) => `${column.name} ${column.ty}${column.nullable ? " NULL" : ""}`).join(", ") ?? "schema missing"}.`,
  }));
  for (const join of state.joins) {
    const right = state.tables.find((table) => table.tableId === join.tableId);
    steps.push({
      key: `join-${join.tableId}`,
      title: `${join.kind === "left" ? "Keep unmatched" : "Match"} ${join.tableId}`,
      detail:
        right && join.leftColumn && join.rightColumn ? `${join.leftTableId}.${join.leftColumn} = ${join.tableId}.${join.rightColumn}` : "Join is incomplete.",
    });
  }
  return steps;
}

/** Build readable filter, projection and limit steps. */
function outputSteps(state: DraftState): readonly PlanStep[] {
  const steps: PlanStep[] = state.filters.map((filter) => ({
    key: `filter-${filter.id}`,
    title: "Filter rows",
    detail: `${filter.tableId}.${filter.columnName || "unset"} ${OPERATOR_SQL[filter.operator]} ${filter.source === "parameter" ? `$${filter.parameterNumber}` : filter.literal || "unset"}.`,
  }));
  const projection = state.tables.flatMap((table) => table.selectedColumns.map((column) => `${table.alias}.${column}`));
  steps.push({
    key: "project",
    title: "Return columns",
    detail: projection.length > 0 ? projection.join(", ") : "No columns selected.",
  });
  steps.push({
    key: "limit",
    title: "Result limit",
    detail: state.limit ? `${state.limit} rows` : "No limit in this draft.",
  });
  return steps;
}

/** Build readable plan cards without executing any semantics. */
export function readablePlan(state: DraftState): readonly PlanStep[] {
  return [...relationSteps(state), ...outputSteps(state)];
}

/** Summarize parameter identity for the draft, including typed NULLs. */
export function parameterSummary(parameters: readonly DraftParameter[]): string {
  return parameters
    .map((parameter) => {
      const value = parameter.type === "null" ? `NULL::${parameter.nullType}` : parameter.value || "(empty)";
      return `$${parameter.number} : ${parameter.type === "null" ? "typed NULL" : parameter.type} = ${value}`;
    })
    .join("\n");
}

/** Re-export filter/join choices needed by the form controls. */
export type { DraftFilter, DraftJoin };
