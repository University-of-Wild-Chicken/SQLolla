/**
 * Typed local relation builder. Every edit is a draft awaiting a deterministic
 * compiler bridge; this component never emits canonical SQL, IR or Alloy.
 */
import { useState, type Dispatch, type JSX } from "react";
import type { ColumnModel } from "../analysis/types";
import type { DraftAction, DraftFilter, DraftParameter, DraftState, DraftType } from "../builder/model";
import { draftErrors, parameterSummary, readablePlan, renderDraftSql } from "../builder/render";
import type { LeftRailProps } from "./LeftRail";
import { DraftCheck } from "./DraftCheck";

type DispatchDraft = Dispatch<DraftAction>;
type BuilderProps = Pick<
  LeftRailProps,
  "scenarios" | "selectedId" | "draft" | "onStartDraft" | "onDiscardDraft" | "dispatchDraft" | "onCheckDraft" | "onStageDraft"
>;
type Option = { readonly value: string | number; readonly label: string };

const TYPES = ["int32", "int64", "bool", "text"] as const;
const TYPE_OPTIONS: readonly Option[] = TYPES.map((value) => ({ value, label: value }));
const OPERATOR_OPTIONS: readonly Option[] = [
  { value: "eq", label: "=" },
  { value: "neq", label: "≠" },
  { value: "lt", label: "<" },
  { value: "lte", label: "≤" },
  { value: "gt", label: ">" },
  { value: "gte", label: "≥" },
  { value: "is_null", label: "IS NULL" },
  { value: "is_not_null", label: "IS NOT NULL" },
];

/** Local type inference for filter controls, never semantic coercion. */
function typeFor(column: ColumnModel | undefined): DraftType {
  return column && ["int32", "int64", "bool", "text"].includes(column.ty) ? (column.ty as DraftType) : "text";
}

/** Build dropdown options for a string array. */
function options(values: readonly string[]): readonly Option[] {
  return values.map((value) => ({ value, label: value }));
}

interface SelectFieldProps {
  readonly label: string;
  readonly value: string | number;
  readonly options: readonly Option[];
  readonly onChange: (value: string) => void;
}

/** Render a labelled accessible select. */
function SelectField({ label, value, options: choices, onChange }: SelectFieldProps): JSX.Element {
  return (
    <select aria-label={label} value={value} onChange={(event) => onChange(event.target.value)}>
      {choices.map((choice) => (
        <option key={String(choice.value)} value={choice.value}>
          {choice.label}
        </option>
      ))}
    </select>
  );
}

/** Render the builder heading. */
function BuilderHeading(): JSX.Element {
  return (
    <div className="rail-heading">
      <span className="mono-label">QUERY STUDIO · LOCAL DRAFT</span>
      <h2>Build the question</h2>
      <p>Every edit marks this as a draft awaiting analysis and sets aside prior results.</p>
    </div>
  );
}

/** Render the explicit schema context and bridge boundary. */
function EmptyBoundary({
  scenario,
  onStartDraft,
}: {
  readonly scenario: LeftRailProps["scenarios"][number] | undefined;
  readonly onStartDraft: () => void;
}): JSX.Element {
  return (
    <>
      <div className="builder-blank">
        <span className="mono-label">SCHEMA CONTEXT</span>
        <p>{scenario?.builderCatalog ? Object.keys(scenario.builderCatalog.tables).join(" · ") : "No schema context available."}</p>
        <p>Starting a draft sets the currently stored evidence aside. No query is executed and no AI call is made.</p>
      </div>
      <button type="button" className="primary-button" onClick={onStartDraft} disabled={!scenario?.builderCatalog}>
        Start draft for this schema
      </button>
    </>
  );
}

/** Render the builder's no-draft entry state. */
function BuilderEmpty({
  scenario,
  onStartDraft,
}: {
  readonly scenario: LeftRailProps["scenarios"][number] | undefined;
  readonly onStartDraft: () => void;
}): JSX.Element {
  return (
    <div className="rail-scroll">
      <div className="rail-heading">
        <h2>Build a query</h2>
        <p>Choose tables and columns, then add filters, joins or a row limit. Check the draft with the analysis server before starting a new review.</p>
      </div>
      <EmptyBoundary scenario={scenario} onStartDraft={onStartDraft} />
    </div>
  );
}

/** Render the business-intent free-text field. */
function IntentField({ value, dispatch }: { readonly value: string; readonly dispatch: DispatchDraft }): JSX.Element {
  return (
    <label className="field">
      Business intent
      <textarea
        rows={2}
        maxLength={240}
        value={value}
        onChange={(event) => dispatch({ type: "intent", intent: event.target.value })}
        placeholder="What should this query answer?"
      />
    </label>
  );
}

/** Render one selectable catalog column. */
function ColumnCheck({ column, checked, onToggle }: { readonly column: ColumnModel; readonly checked: boolean; readonly onToggle: () => void }): JSX.Element {
  return (
    <label className="check-row">
      <input type="checkbox" checked={checked} onChange={onToggle} />
      <span>
        {column.name}
        <small>
          {column.ty}
          {column.nullable ? " · nullable" : ""}
        </small>
      </span>
    </label>
  );
}

/** Render the columns of one selected table. */
function TableColumns({
  draft,
  table,
  dispatch,
}: {
  readonly draft: DraftState;
  readonly table: NonNullable<DraftState["tables"][number]>;
  readonly dispatch: DispatchDraft;
}): JSX.Element {
  return (
    <div className="column-grid">
      {draft.catalog.tables[table.tableId]?.columns.map((column) => (
        <ColumnCheck
          key={column.name}
          column={column}
          checked={table.selectedColumns.includes(column.name)}
          onToggle={() => dispatch({ type: "toggle-column", tableId: table.tableId, columnName: column.name })}
        />
      ))}
    </div>
  );
}

/** Render one selected table. */
function TableCard({
  draft,
  tableId,
  dispatch,
  canRemove,
}: {
  readonly draft: DraftState;
  readonly tableId: string;
  readonly dispatch: DispatchDraft;
  readonly canRemove: boolean;
}): JSX.Element | null {
  const table = draft.tables.find((candidate) => candidate.tableId === tableId);
  if (!table || !draft.catalog.tables[tableId]) return null;
  return (
    <fieldset className="table-card">
      <legend>
        <span>{table.tableId}</span>
        <span className="alias-badge">AS {table.alias}</span>
      </legend>
      <TableColumns draft={draft} table={table} dispatch={dispatch} />
      {canRemove ? (
        <button type="button" className="text-button" onClick={() => dispatch({ type: "remove-table", tableId })}>
          Remove table
        </button>
      ) : null}
    </fieldset>
  );
}

/** Render the add-table selector. */
function AddTable({ draft, dispatch }: { readonly draft: DraftState; readonly dispatch: DispatchDraft }): JSX.Element {
  const [tableId, setTableId] = useState("");
  const available = Object.keys(draft.catalog.tables).filter((id) => !draft.tables.some((table) => table.tableId === id));
  return (
    <div className="builder-section__head">
      <span className="mono-label">TABLES</span>
      <SelectField label="Table to add" value={tableId} options={[{ value: "", label: "Choose schema table…" }, ...options(available)]} onChange={setTableId} />
      <button
        type="button"
        className="text-button"
        disabled={!tableId}
        onClick={() => {
          dispatch({ type: "add-table", tableId });
          setTableId("");
        }}
      >
        Add table
      </button>
    </div>
  );
}

/** Render table selection and the selected table cards. */
function TablesSection({ draft, dispatch }: { readonly draft: DraftState; readonly dispatch: DispatchDraft }): JSX.Element {
  return (
    <section className="builder-section">
      <AddTable draft={draft} dispatch={dispatch} />
      {draft.tables.map((table) => (
        <TableCard key={table.tableId} draft={draft} tableId={table.tableId} dispatch={dispatch} canRemove={draft.tables.length > 1} />
      ))}
    </section>
  );
}

/** Render a parameter's type selector. */
function ParameterTypeField({ parameter, dispatch }: { readonly parameter: DraftParameter; readonly dispatch: DispatchDraft }): JSX.Element {
  return (
    <SelectField
      label={`Type for parameter ${parameter.number}`}
      value={parameter.type}
      options={[...TYPE_OPTIONS, { value: "null", label: "typed NULL" }]}
      onChange={(type) => dispatch({ type: "update-parameter", number: parameter.number, patch: { type: type as DraftType, value: "" } })}
    />
  );
}

/** Render a parameter's typed value controls. */
function ParameterValue({ parameter, dispatch }: { readonly parameter: DraftParameter; readonly dispatch: DispatchDraft }): JSX.Element {
  return (
    <>
      <ParameterTypeField parameter={parameter} dispatch={dispatch} />
      {parameter.type === "null" ? (
        <SelectField
          label={`NULL type for parameter ${parameter.number}`}
          value={parameter.nullType}
          options={TYPE_OPTIONS}
          onChange={(nullType) => dispatch({ type: "update-parameter", number: parameter.number, patch: { nullType: nullType as DraftParameter["nullType"] } })}
        />
      ) : (
        <input
          aria-label={`Value for parameter ${parameter.number}`}
          value={parameter.value}
          onChange={(event) => dispatch({ type: "update-parameter", number: parameter.number, patch: { value: event.target.value } })}
          placeholder={parameter.type === "bool" ? "true / false" : "typed value"}
        />
      )}
    </>
  );
}

/** Render one numbered parameter. */
function ParameterRow({ parameter, dispatch }: { readonly parameter: DraftParameter; readonly dispatch: DispatchDraft }): JSX.Element {
  return (
    <fieldset className="parameter-row">
      <legend>${parameter.number}</legend>
      <ParameterValue parameter={parameter} dispatch={dispatch} />
      <span className="parameter-type">${parameter.number} identity preserved</span>
    </fieldset>
  );
}

/** Render numbered parameters and the add action. */
function ParameterSection({ draft, dispatch }: { readonly draft: DraftState; readonly dispatch: DispatchDraft }): JSX.Element {
  return (
    <section className="builder-section">
      <div className="builder-section__head">
        <span className="mono-label">PARAMETERS</span>
        <button type="button" className="text-button" onClick={() => dispatch({ type: "add-parameter" })}>
          Add parameter
        </button>
      </div>
      {draft.parameters.map((parameter) => (
        <ParameterRow key={parameter.number} parameter={parameter} dispatch={dispatch} />
      ))}
    </section>
  );
}

/** Render the filter table selector. */
function FilterTableField({
  draft,
  filter,
  dispatch,
}: {
  readonly draft: DraftState;
  readonly filter: DraftFilter;
  readonly dispatch: DispatchDraft;
}): JSX.Element {
  const updateTable = (tableId: string): void => {
    const next = draft.catalog.tables[tableId]?.columns[0];
    dispatch({ type: "update-filter", id: filter.id, patch: { tableId, columnName: next?.name ?? "", literalType: typeFor(next) } });
  };
  return (
    <SelectField
      label={`Table for filter ${filter.id}`}
      value={filter.tableId}
      options={options(draft.tables.map((item) => item.tableId))}
      onChange={updateTable}
    />
  );
}

/** Render the filter column selector. */
function FilterColumnField({
  draft,
  filter,
  dispatch,
}: {
  readonly draft: DraftState;
  readonly filter: DraftFilter;
  readonly dispatch: DispatchDraft;
}): JSX.Element {
  const table = draft.catalog.tables[filter.tableId];
  const columns = table?.columns.map((column) => ({ value: column.name, label: column.name })) ?? [];
  return (
    <SelectField
      label={`Column for filter ${filter.id}`}
      value={filter.columnName}
      options={columns}
      onChange={(columnName) =>
        dispatch({
          type: "update-filter",
          id: filter.id,
          patch: { columnName, literalType: typeFor(table?.columns.find((column) => column.name === columnName)) },
        })
      }
    />
  );
}

/** Render the filter operator selector. */
function FilterOperatorField({ filter, dispatch }: { readonly filter: DraftFilter; readonly dispatch: DispatchDraft }): JSX.Element {
  return (
    <SelectField
      label={`Operator for filter ${filter.id}`}
      value={filter.operator}
      options={OPERATOR_OPTIONS}
      onChange={(operator) => dispatch({ type: "update-filter", id: filter.id, patch: { operator: operator as DraftFilter["operator"] } })}
    />
  );
}

/** Render filter table/column/operator selection. */
function FilterTarget({
  draft,
  filter,
  dispatch,
}: {
  readonly draft: DraftState;
  readonly filter: DraftFilter;
  readonly dispatch: DispatchDraft;
}): JSX.Element {
  return (
    <div className="filter-row">
      <FilterTableField draft={draft} filter={filter} dispatch={dispatch} />
      <FilterColumnField draft={draft} filter={filter} dispatch={dispatch} />
      <FilterOperatorField filter={filter} dispatch={dispatch} />
    </div>
  );
}

/** Render parameter-source selection for a filter. */
function FilterParameterValue({
  draft,
  filter,
  dispatch,
}: {
  readonly draft: DraftState;
  readonly filter: DraftFilter;
  readonly dispatch: DispatchDraft;
}): JSX.Element {
  const parameter = draft.parameters.find((candidate) => candidate.number === filter.parameterNumber);
  return (
    <>
      <SelectField
        label={`Parameter for filter ${filter.id}`}
        value={filter.parameterNumber}
        options={draft.parameters.map((candidate) => ({ value: candidate.number, label: `$${candidate.number} · ${candidate.type}` }))}
        onChange={(number) => dispatch({ type: "update-filter", id: filter.id, patch: { parameterNumber: Number(number) } })}
      />
      <span className="parameter-type">{parameter ? `$${parameter.number} reused` : "missing parameter"}</span>
    </>
  );
}

/** Render the filter literal type selector. */
function LiteralTypeField({ filter, dispatch }: { readonly filter: DraftFilter; readonly dispatch: DispatchDraft }): JSX.Element {
  return (
    <SelectField
      label={`Literal type for filter ${filter.id}`}
      value={filter.literalType}
      options={[...TYPE_OPTIONS, { value: "null", label: "typed NULL" }]}
      onChange={(type) => dispatch({ type: "update-filter", id: filter.id, patch: { literalType: type as DraftType } })}
    />
  );
}

/** Render literal-source selection for a filter. */
function FilterLiteralValue({ filter, dispatch }: { readonly filter: DraftFilter; readonly dispatch: DispatchDraft }): JSX.Element {
  return (
    <>
      <LiteralTypeField filter={filter} dispatch={dispatch} />
      {filter.literalType === "null" ? (
        <SelectField
          label={`NULL type for filter ${filter.id}`}
          value={filter.literalNullType}
          options={TYPE_OPTIONS}
          onChange={(type) => dispatch({ type: "update-filter", id: filter.id, patch: { literalNullType: type as DraftFilter["literalNullType"] } })}
        />
      ) : (
        <input
          aria-label={`Literal for filter ${filter.id}`}
          value={filter.literal}
          onChange={(event) => dispatch({ type: "update-filter", id: filter.id, patch: { literal: event.target.value } })}
          placeholder="typed value"
        />
      )}
    </>
  );
}

/** Render literal or parameter value controls for a filter. */
function FilterValue({ draft, filter, dispatch }: { readonly draft: DraftState; readonly filter: DraftFilter; readonly dispatch: DispatchDraft }): JSX.Element {
  return (
    <div className="filter-row">
      <SelectField
        label={`Value source for filter ${filter.id}`}
        value={filter.source}
        options={[
          { value: "parameter", label: "parameter" },
          { value: "literal", label: "literal" },
        ]}
        onChange={(source) =>
          dispatch({
            type: "update-filter",
            id: filter.id,
            patch: { source: source as DraftFilter["source"] },
          })
        }
      />
      {filter.source === "parameter" ? (
        <FilterParameterValue draft={draft} filter={filter} dispatch={dispatch} />
      ) : (
        <FilterLiteralValue filter={filter} dispatch={dispatch} />
      )}
    </div>
  );
}

/** Render one filter card. */
function FilterCard({ draft, filter, dispatch }: { readonly draft: DraftState; readonly filter: DraftFilter; readonly dispatch: DispatchDraft }): JSX.Element {
  const binary = filter.operator !== "is_null" && filter.operator !== "is_not_null";
  return (
    <fieldset className="filter-card">
      <legend>Filter {filter.id}</legend>
      <FilterTarget draft={draft} filter={filter} dispatch={dispatch} />
      {binary ? <FilterValue draft={draft} filter={filter} dispatch={dispatch} /> : null}
      <button type="button" className="text-button" onClick={() => dispatch({ type: "remove-filter", id: filter.id })}>
        Remove filter
      </button>
    </fieldset>
  );
}

/** Render filters and the add action. */
function FilterSection({ draft, dispatch }: { readonly draft: DraftState; readonly dispatch: DispatchDraft }): JSX.Element {
  return (
    <section className="builder-section">
      <div className="builder-section__head">
        <span className="mono-label">FILTERS</span>
        <button type="button" className="text-button" onClick={() => dispatch({ type: "add-filter" })}>
          Add filter
        </button>
      </div>
      {draft.filters.length === 0 ? <p className="empty-note">No filters yet. A draft without filters is still unvalidated.</p> : null}
      {draft.filters.map((filter) => (
        <FilterCard key={filter.id} draft={draft} filter={filter} dispatch={dispatch} />
      ))}
    </section>
  );
}

/** Render the join type selector. */
function JoinTypeField({
  draft,
  joinTable,
  dispatch,
}: {
  readonly draft: DraftState;
  readonly joinTable: string;
  readonly dispatch: DispatchDraft;
}): JSX.Element {
  const join = draft.joins.find((candidate) => candidate.tableId === joinTable)!;
  return (
    <SelectField
      label={`Join type for ${joinTable}`}
      value={join.kind}
      options={[
        { value: "inner", label: "INNER JOIN" },
        { value: "left", label: "LEFT JOIN" },
      ]}
      onChange={(kind) => dispatch({ type: "update-join", tableId: joinTable, patch: { kind: kind as "inner" | "left" } })}
    />
  );
}

/** Render the join left-table selector. */
function JoinLeftTableField({
  draft,
  joinTable,
  dispatch,
}: {
  readonly draft: DraftState;
  readonly joinTable: string;
  readonly dispatch: DispatchDraft;
}): JSX.Element {
  const join = draft.joins.find((candidate) => candidate.tableId === joinTable)!;
  const tables = draft.tables.filter((table) => table.tableId !== joinTable).map((table) => table.tableId);
  return (
    <SelectField
      label={`Left table for ${joinTable}`}
      value={join.leftTableId}
      options={options(tables)}
      onChange={(leftTableId) => dispatch({ type: "update-join", tableId: joinTable, patch: { leftTableId, leftColumn: "" } })}
    />
  );
}

/** Render join type and left table choices. */
function JoinTables({ draft, joinTable, dispatch }: { readonly draft: DraftState; readonly joinTable: string; readonly dispatch: DispatchDraft }): JSX.Element {
  return (
    <>
      <JoinTypeField draft={draft} joinTable={joinTable} dispatch={dispatch} />
      <JoinLeftTableField draft={draft} joinTable={joinTable} dispatch={dispatch} />
    </>
  );
}

/** Render the left join-column selector. */
function JoinLeftColumnField({
  draft,
  joinTable,
  dispatch,
}: {
  readonly draft: DraftState;
  readonly joinTable: string;
  readonly dispatch: DispatchDraft;
}): JSX.Element {
  const join = draft.joins.find((candidate) => candidate.tableId === joinTable)!;
  const left = draft.tables.find((table) => table.tableId === join.leftTableId);
  const columns = left ? options(draft.catalog.tables[left.tableId]?.columns.map((column) => column.name) ?? []) : [];
  return (
    <SelectField
      label={`Left column for ${joinTable}`}
      value={join.leftColumn}
      options={[{ value: "", label: "choose column…" }, ...columns]}
      onChange={(leftColumn) => dispatch({ type: "update-join", tableId: joinTable, patch: { leftColumn } })}
    />
  );
}

/** Render the right join-column selector. */
function JoinRightColumnField({
  draft,
  joinTable,
  dispatch,
}: {
  readonly draft: DraftState;
  readonly joinTable: string;
  readonly dispatch: DispatchDraft;
}): JSX.Element {
  const right = draft.tables.find((table) => table.tableId === joinTable);
  const columns = right ? options(draft.catalog.tables[right.tableId]?.columns.map((column) => column.name) ?? []) : [];
  const join = draft.joins.find((candidate) => candidate.tableId === joinTable)!;
  return (
    <SelectField
      label={`Right column for ${joinTable}`}
      value={join.rightColumn}
      options={[{ value: "", label: "choose column…" }, ...columns]}
      onChange={(rightColumn) => dispatch({ type: "update-join", tableId: joinTable, patch: { rightColumn } })}
    />
  );
}

/** Render join column choices. */
function JoinColumns({
  draft,
  joinTable,
  dispatch,
}: {
  readonly draft: DraftState;
  readonly joinTable: string;
  readonly dispatch: DispatchDraft;
}): JSX.Element {
  return (
    <>
      <JoinLeftColumnField draft={draft} joinTable={joinTable} dispatch={dispatch} />
      <span aria-hidden="true">=</span>
      <JoinRightColumnField draft={draft} joinTable={joinTable} dispatch={dispatch} />
    </>
  );
}

/** Render one join card. */
function JoinCard({ draft, joinTable, dispatch }: { readonly draft: DraftState; readonly joinTable: string; readonly dispatch: DispatchDraft }): JSX.Element {
  if (!draft.joins.some((join) => join.tableId === joinTable)) return <p className="empty-note">Join missing for {joinTable}.</p>;
  return (
    <fieldset className="join-card">
      <legend>{joinTable}</legend>
      <div className="filter-row">
        <JoinTables draft={draft} joinTable={joinTable} dispatch={dispatch} />
        <JoinColumns draft={draft} joinTable={joinTable} dispatch={dispatch} />
      </div>
    </fieldset>
  );
}

/** Render join cards for each additional table. */
function JoinSection({ draft, dispatch }: { readonly draft: DraftState; readonly dispatch: DispatchDraft }): JSX.Element | null {
  if (draft.tables.length < 2) return null;
  return (
    <section className="builder-section">
      <span className="mono-label">JOINS</span>
      {draft.joins.map((join) => (
        <JoinCard key={join.tableId} draft={draft} joinTable={join.tableId} dispatch={dispatch} />
      ))}
    </section>
  );
}

/** Render the result limit. */
function LimitField({ draft, dispatch }: { readonly draft: DraftState; readonly dispatch: DispatchDraft }): JSX.Element {
  return (
    <label className="field field--inline">
      Result limit
      <input inputMode="numeric" value={draft.limit} onChange={(event) => dispatch({ type: "limit", limit: event.target.value })} placeholder="e.g. 50" />
      <span>Whole number 0–500, or blank. LIMIT is a draft bound, not a solver bound.</span>
    </label>
  );
}

/** Render the readable draft plan. */
function PlanSection({ draft }: { readonly draft: DraftState }): JSX.Element {
  return (
    <section className="builder-section">
      <span className="mono-label">READABLE RELATIONAL PLAN</span>
      <ol className="mini-plan">
        {readablePlan(draft).map((step) => (
          <li key={step.key}>
            <strong>{step.title}</strong>
            <span>{step.detail}</span>
          </li>
        ))}
      </ol>
    </section>
  );
}

/** Render the unvalidated SQL preview. */
function SqlSection({ sql, errors }: { readonly sql: string | null; readonly errors: readonly string[] }): JSX.Element {
  return (
    <section className="builder-section builder-sql">
      <div className="builder-section__head">
        <span className="mono-label">DRAFT SQL PREVIEW · NOT VALIDATED</span>
        <span className="draft-badge">DRAFT</span>
      </div>
      {sql ? (
        <pre className="code-block">{sql}</pre>
      ) : (
        <div className="inline-alert">
          <strong>Preview unavailable</strong>
          <span>{errors[0] ?? "Complete the draft."}</span>
        </div>
      )}
      {errors.length > 0 ? (
        <ul className="draft-errors">
          {errors.slice(0, 3).map((error) => (
            <li key={error}>{error}</li>
          ))}
        </ul>
      ) : null}
    </section>
  );
}

/** Render parameter identity in the preview. */
function ParameterIdentity({ parameters }: { readonly parameters: DraftState["parameters"] }): JSX.Element {
  return (
    <div className="builder-params-summary">
      <span className="mono-label">PARAMETER IDENTITY</span>
      <pre>{parameterSummary(parameters) || "No parameters."}</pre>
    </div>
  );
}

/** Render the full typed builder. */
export function BuilderPanel({
  draft,
  scenarios,
  selectedId,
  onStartDraft,
  onDiscardDraft,
  dispatchDraft,
  onCheckDraft,
  onStageDraft,
}: BuilderProps): JSX.Element {
  const scenario = scenarios.find((candidate) => candidate.id === selectedId);
  if (!draft) return <BuilderEmpty scenario={scenario} onStartDraft={onStartDraft} />;
  const errors = draftErrors(draft);
  return (
    <div className="rail-scroll builder-panel">
      <BuilderHeading />
      <IntentField value={draft.intent} dispatch={dispatchDraft} />
      <TablesSection draft={draft} dispatch={dispatchDraft} />
      <ParameterSection draft={draft} dispatch={dispatchDraft} />
      <FilterSection draft={draft} dispatch={dispatchDraft} />
      <JoinSection draft={draft} dispatch={dispatchDraft} />
      <LimitField draft={draft} dispatch={dispatchDraft} />
      <PlanSection draft={draft} />
      <SqlSection sql={renderDraftSql(draft)} errors={errors} />
      <ParameterIdentity parameters={draft.parameters} />
      <DraftCheck draft={draft} onCheck={onCheckDraft} onStage={onStageDraft} onDiscard={onDiscardDraft} />
    </div>
  );
}
