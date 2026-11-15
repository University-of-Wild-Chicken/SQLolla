import { stringifyJson } from "./json";
/**
 * Deterministic presentation derivation over already-validated evidence. This
 * module links supplied facts and shapes read-only tables; it never executes or
 * reinterprets SQL.
 */
import { canonicalJson, cellText, escapePointerSegment, resolveJsonPointer, tableLabel } from "./format";
import type {
  CatalogModel,
  CellModel,
  ColumnModel,
  EffectObservation,
  ErrorObservation,
  EvidenceBundleModel,
  EvidenceView,
  FactIntegrityIssue,
  FactModel,
  InstanceModel,
  OccurrenceView,
  OutputView,
  RelationView,
  StatementView,
  SuccessObservation,
  TableModel,
} from "./types";

/** Index facts by their exact supplied JSON Pointer, preserving no new identities. */
function indexFacts(facts: readonly FactModel[]): ReadonlyMap<string, FactModel> {
  return new Map(facts.map((fact) => [fact.reference, fact]));
}

/** Cross-check embedded facts against their pointers; this is integrity, not authentication. */
function checkFacts(facts: readonly FactModel[], semantic: unknown): readonly FactIntegrityIssue[] {
  const issues: FactIntegrityIssue[] = [];
  for (const fact of facts) {
    const resolved = resolveJsonPointer(semantic, fact.reference);
    if (resolved === undefined) {
      issues.push({
        reference: fact.reference,
        reason: "Reference does not resolve in semantic evidence",
      });
    } else if (canonicalJson(resolved) !== canonicalJson(fact.value)) {
      issues.push({
        reference: fact.reference,
        reason: "Fact value differs from semantic evidence",
      });
    }
  }
  return issues;
}

/** Prefer the catalog attached to the semantic payload, then the bundle request catalog. */
function selectCatalog(bundle: EvidenceBundleModel, instance: InstanceModel): CatalogModel {
  return instance.semantic.catalog ?? bundle.request.catalog;
}

/** Supply explicit placeholder columns only when the catalog truly omitted a table. */
function inferColumns(rows: readonly (readonly CellModel[])[]): readonly ColumnModel[] {
  const width = rows.reduce((max, row) => Math.max(max, row.length), 0);
  return Array.from({ length: width }, (_, index) => {
    const sample = rows[0]?.[index];
    return {
      name: `column_${index + 1}`,
      ty: sample?.type ?? "unknown",
      nullable: true,
      qualifier: null,
      schema: null,
      raw: null,
    };
  });
}

/** Resolve a table's display schema while marking schema absence rather than hiding it. */
function relationSchema(
  tableId: string,
  catalog: CatalogModel,
  rows: readonly (readonly CellModel[])[],
): { columns: readonly ColumnModel[]; schemaSupplied: boolean } {
  const table: TableModel | undefined = catalog.tables[tableId];
  return table ? { columns: table.columns, schemaSupplied: true } : { columns: inferColumns(rows), schemaSupplied: false };
}

/** Build ordered row occurrences, keeping duplicate positions as separate nodes. */
function makeOccurrences(
  rows: readonly (readonly CellModel[])[],
  basePath: string,
  facts: ReadonlyMap<string, FactModel>,
  compareRows: readonly (readonly CellModel[])[] | null = null,
): readonly OccurrenceView[] {
  return rows.map((cells, index) => {
    const reference = `${basePath}/${index}`;
    const previous = compareRows?.[index];
    const changedColumns =
      previous && previous.length === cells.length
        ? cells.flatMap((cell, column) => (canonicalJson(cell.raw) === canonicalJson(previous[column]?.raw) ? [] : [column]))
        : [];
    return {
      ordinal: index,
      position: index + 1,
      cells,
      factReference: facts.has(reference) ? reference : null,
      changedColumns,
    };
  });
}

/** Build one before/after relation collection with exact schema context. */
function buildRelations(
  database: Readonly<Record<string, readonly (readonly CellModel[])[]>>,
  catalog: CatalogModel,
  basePath: string,
  facts: ReadonlyMap<string, FactModel>,
  compare: Readonly<Record<string, readonly (readonly CellModel[])[]>> | null = null,
): readonly RelationView[] {
  return Object.entries(database).map(([tableId, rows]) => {
    const labels = tableLabel(tableId);
    const schema = relationSchema(tableId, catalog, rows);
    const compareRows = compare?.[tableId];
    const comparable = compareRows && compareRows.length === rows.length ? compareRows : null;
    return {
      tableId,
      schemaName: labels.schema,
      tableName: labels.name,
      columns: schema.columns,
      schemaSupplied: schema.schemaSupplied,
      rows: makeOccurrences(rows, `${basePath}/${escapePointerSegment(tableId)}`, facts, comparable),
    };
  });
}

/** Normalize a table row bag into a deterministic duplicate key. */
function rowKey(row: readonly CellModel[]): string {
  return canonicalJson(row.map((cell) => cell.raw));
}

/** Count exact duplicate row occurrences while preserving each displayed row. */
function countDuplicateRows(database: Readonly<Record<string, readonly (readonly CellModel[])[]>>): number {
  let duplicates = 0;
  for (const rows of Object.values(database)) {
    const groups = new Map<string, number>();
    for (const row of rows) groups.set(rowKey(row), (groups.get(rowKey(row)) ?? 0) + 1);
    for (const count of groups.values()) duplicates += Math.max(0, count - 1);
  }
  return duplicates;
}

/** Count typed NULL cells across every visible deterministic observation. */
function countTypedNulls(input: readonly RelationView[], output: OutputView, statement: StatementView): number {
  const groups = [
    ...input.map((table) => table.rows),
    ...statement.poststateTables.map((table) => table.rows),
    output.rows,
    statement.error?.exposedRows ?? [],
  ];
  return groups.reduce((total, rows) => total + rows.reduce((sum, row) => sum + row.cells.filter((cell) => cell.type === "null").length, 0), 0);
}

/** Humanize one effect kind without giving it SQL execution meaning. */
function effectLabel(kind: string): string {
  const labels: Record<string, string> = {
    sequence_value: "Sequence value allocated",
    table_lock: "Table lock observation",
    row_lock: "Row lock observation",
    notification: "Notification queued",
    advisory_lock: "Advisory lock observation",
  };
  return labels[kind] ?? `Effect observation: ${kind}`;
}

/** Render top-level effect fields as exact plain text. */
function effectFields(effect: Record<string, unknown>): Readonly<Record<string, string>> {
  return Object.fromEntries(
    Object.entries(effect)
      .filter(([key]) => key !== "kind")
      .map(([key, value]) => [key, typeof value === "string" ? value : stringifyJson(value)]),
  );
}

/** Convert supplied effects into visible observation cards, linking only existing facts. */
function buildEffects(effects: readonly unknown[], basePath: string, facts: ReadonlyMap<string, FactModel>): readonly EffectObservation[] {
  return effects.map((effect, index) => {
    const object = typeof effect === "object" && effect !== null ? (effect as Record<string, unknown>) : {};
    const kind = typeof object["kind"] === "string" ? object["kind"] : "unknown";
    const reference = `${basePath}/${index}`;
    return {
      kind,
      label: effectLabel(kind),
      fields: effectFields(object),
      reference: facts.has(reference) ? reference : null,
      raw: effect,
    };
  });
}

/** Build the error observation from a finite SQL rejection. */
function buildError(error: InstanceModel["semantic"]["outcome"], facts: ReadonlyMap<string, FactModel>): ErrorObservation | null {
  if (!error?.error) return null;
  const value = error.error;
  return {
    sqlstate: value.sqlstate,
    phase: value.phase,
    constraintName: value.constraintName,
    columnName: value.columnName,
    exposedRows: makeOccurrences(value.exposedRows, "/semantic/outcome/outcome/exposed_rows", facts),
    effects: buildEffects(value.effects, "/semantic/outcome/outcome/effects", facts),
  };
}

/** Build poststate relations for successful finite outcomes. */
function buildPoststate(
  success: SuccessObservation | null,
  catalog: CatalogModel,
  facts: ReadonlyMap<string, FactModel>,
  input: EvidenceBundleModel["request"]["database"],
): readonly RelationView[] {
  if (!success) return [];
  return buildRelations(success.database, catalog, "/semantic/outcome/outcome/database", facts, input);
}

/** Describe a native read example that has no statement-level outcome. */
function emptyStatement(): StatementView {
  return {
    outcome: "none",
    commandTag: null,
    affected: null,
    ordering: "unknown",
    effects: [],
    error: null,
    poststateTables: [],
  };
}

/** Build statement observations for a supplied finite outcome. */
function finiteStatement(
  outcome: NonNullable<InstanceModel["semantic"]["outcome"]>,
  catalog: CatalogModel,
  facts: ReadonlyMap<string, FactModel>,
  input: EvidenceBundleModel["request"]["database"],
): StatementView {
  const success = outcome.success;
  return {
    outcome: outcome.status,
    commandTag: success?.commandTag ?? null,
    affected: success?.affected ?? null,
    ordering: success?.ordering ?? "unknown",
    effects: buildEffects(success?.effects ?? outcome.error?.effects ?? [], "/semantic/outcome/outcome/effects", facts),
    error: buildError(outcome, facts),
    poststateTables: buildPoststate(success, catalog, facts, input),
  };
}

/** Build statement observations without implying an outcome when none was supplied. */
function buildStatement(
  semantic: InstanceModel["semantic"],
  catalog: CatalogModel,
  facts: ReadonlyMap<string, FactModel>,
  input: EvidenceBundleModel["request"]["database"],
): StatementView {
  return semantic.outcome ? finiteStatement(semantic.outcome, catalog, facts, input) : emptyStatement();
}

/** Choose native top-level output or finite success.rows, preserving its fact base. */
function buildOutput(semantic: InstanceModel["semantic"], columns: readonly ColumnModel[], facts: ReadonlyMap<string, FactModel>): OutputView {
  if (semantic.output) {
    return {
      columns,
      rows: makeOccurrences(semantic.output, "/semantic/output", facts),
      ordering: semantic.outcome?.success?.ordering ?? "unknown",
      factBase: "/semantic/output",
    };
  }
  if (semantic.outcome?.success) {
    return {
      columns,
      rows: makeOccurrences(semantic.outcome.success.rows, "/semantic/outcome/outcome/rows", facts),
      ordering: semantic.outcome.success.ordering,
      factBase: "/semantic/outcome/outcome/rows",
    };
  }
  return { columns, rows: [], ordering: "unknown", factBase: "" };
}

/** Compose a concise screen-reader description from deterministic counts only. */
function describe(bundle: EvidenceBundleModel, instance: InstanceModel, input: readonly RelationView[], output: OutputView, statement: StatementView): string {
  const inputText = input.map((table) => `${table.tableId}: ${table.rows.length} occurrences`).join("; ");
  const error = statement.error ? ` SQL error ${statement.error.sqlstate}.` : "";
  const duplicate = countDuplicateRows(instance.semantic.database);
  return `${instance.kind === "counterexample" ? "Counterexample" : "Example"} from ${bundle.kind} revision. Before ${inputText || "no tables"}. ${output.rows.length} output occurrences. ${duplicate} duplicate input occurrence${duplicate === 1 ? "" : "s"}.${error}`;
}

/** Build all read-only views while leaving the immutable evidence untouched. */
export function buildEvidenceView(bundle: EvidenceBundleModel, instance: InstanceModel): EvidenceView {
  const catalog = selectCatalog(bundle, instance);
  const facts = indexFacts(instance.facts);
  const inputTables = buildRelations(instance.semantic.database, catalog, "/semantic/database", facts);
  const output = buildOutput(instance.semantic, instance.outputSchema, facts);
  const statement = buildStatement(instance.semantic, catalog, facts, bundle.request.database);
  return {
    bundle,
    instance,
    inputTables,
    output,
    statement,
    facts: instance.facts,
    invalidFacts: checkFacts(instance.facts, instance.raw),
    typedNullCount: countTypedNulls(inputTables, output, statement),
    duplicateInputRows: countDuplicateRows(instance.semantic.database),
    textDescription: describe(bundle, instance, inputTables, output, statement),
  };
}

/** Re-export cell formatting so graph/table components share one text policy. */
export { cellText };
