/**
 * Explicit factories for illustrative evidence fixtures. All identities are
 * deterministic fixture-local tokens shaped like SHA-256; they are deliberately
 * not presented as backend fingerprints or authenticated hashes.
 */
import { escapePointerSegment } from "./format";
import type { CatalogModel, ColumnModel, FixtureProvenance, TableModel } from "./types";

/** One raw typed cell in generated fixture JSON. */
export interface RawCell {
  readonly type: string;
  readonly value: unknown;
}

/** One raw row. */
export type RawRow = readonly RawCell[];

/** A table-to-row-bags JSON object. */
export type RawDatabase = Readonly<Record<string, readonly RawRow[]>>;

/** One raw catalog column. */
export interface RawColumn {
  readonly name: string;
  readonly ty: string;
  readonly nullable: boolean;
  readonly qualifier: string | null;
  readonly schema: string | null;
}

/** One raw catalog table. */
export interface RawTable {
  readonly schema: string;
  readonly name: string;
  readonly columns: readonly RawColumn[];
}

/** The raw closed catalog object carried by a fixture request. */
export interface RawCatalog {
  readonly search_path: readonly string[];
  readonly tables: Readonly<Record<string, RawTable>>;
}

/** A successful finite observation in raw fixture JSON. */
export interface RawSuccess {
  readonly database: RawDatabase;
  readonly rows: readonly RawRow[];
  readonly ordering: "bag" | "ordered";
  readonly affected: number;
  readonly command_tag: string;
  readonly effects: readonly Record<string, unknown>[];
}

/** A modeled SQL rejection in raw fixture JSON. */
export interface RawFailure {
  readonly sqlstate: string;
  readonly phase: string;
  readonly constraint_name: string | null;
  readonly column_name: string | null;
  readonly exposed_rows: readonly RawRow[];
  readonly effects: readonly Record<string, unknown>[];
}

/** One raw statement outcome. */
export interface RawOutcome {
  readonly status: "succeeded" | "sql_error";
  readonly outcome: RawSuccess | RawFailure;
}

/** The raw semantic payload of a fixture instance. */
export interface RawSemantic {
  readonly database: RawDatabase;
  readonly catalog?: RawCatalog;
  readonly output?: readonly RawRow[];
  readonly outcome?: RawOutcome;
  readonly transition_index?: number;
}

/** One raw fact supplied to the evidence view. */
export interface RawFact {
  readonly reference: string;
  readonly kind: string;
  readonly occurrence: number;
  readonly value: unknown;
}

/** Build an int32 typed cell. */
export function int32(value: number): RawCell {
  return { type: "int32", value };
}

/** Build a text typed cell. */
export function text(value: string): RawCell {
  return { type: "text", value };
}

/** Build an explicitly typed NULL cell. */
export function typedNull(type: "int32" | "text" | "bool" | "int64"): RawCell {
  return { type: "null", value: type };
}

/** Build an int64 typed cell. */
export function int64(value: number): RawCell {
  return { type: "int64", value };
}

/** Create a deterministic fixture-local identity with no cryptographic claim. */
export function fixtureHash(seed: string): string {
  let state = 0x811c9dc5;
  const words: string[] = [];
  for (let round = 0; round < 8; round += 1) {
    for (let index = 0; index < seed.length; index += 1) {
      state = Math.imul(state ^ (seed.charCodeAt(index) + round), 16777619);
    }
    state ^= (state << 13) | (state >>> 19);
    words.push((state >>> 0).toString(16).padStart(8, "0"));
  }
  return words.join("");
}

/** Convert a raw catalog to the builder-facing schema model. */
export function catalogModel(raw: RawCatalog): CatalogModel {
  const tables: Record<string, TableModel> = {};
  for (const [key, table] of Object.entries(raw.tables)) {
    tables[key] = {
      key,
      schema: table.schema,
      name: table.name,
      columns: table.columns.map(columnModel),
      raw: table,
    };
  }
  return { searchPath: raw.search_path, tables, raw };
}

/** Convert one raw column to a schema model. */
function columnModel(raw: RawColumn): ColumnModel {
  return {
    name: raw.name,
    ty: raw.ty,
    nullable: raw.nullable,
    qualifier: raw.qualifier,
    schema: raw.schema,
    raw,
  };
}

/** Create fixture provenance safe to display beside canonical-shaped evidence. */
export function demoProvenance(label: FixtureProvenance["label"], source: string, note: string): FixtureProvenance {
  return { label, source, note };
}

/** Build raw provenance for the bundle without inventing tool identities. */
function rawProvenance(source: string): Record<string, unknown> {
  return {
    fixture: true,
    note: "Illustrative frontend fixture. No parser, compiler, Alloy solver, or Rust replay was executed.",
    source,
    assurance: "demo data only",
  };
}

/** Provide an illustrative native-profile IR excerpt without claiming compilation. */
export function scanIr(table: string, columns: readonly RawColumn[]): Record<string, unknown> {
  return readIr(
    table,
    columns,
    columns.map((_, index) => index),
    columns,
  );
}

/** Build an illustrative projection/scan excerpt for selected column positions. */
export function readIr(
  table: string,
  inputColumns: readonly RawColumn[],
  selected: readonly number[],
  outputColumns: readonly RawColumn[],
): Record<string, unknown> {
  const expressions = selected.map((index) => ({
    node: { Column: index },
    ty: inputColumns[index]?.ty,
    nullable: inputColumns[index]?.nullable ?? true,
  }));
  return {
    node: {
      Project: {
        expressions,
        input: { node: { Scan: { table } }, schema: inputColumns },
      },
    },
    schema: outputColumns,
  };
}

/** Provide a clearly labelled illustrative Alloy source excerpt. */
export function demoAlloy(command: "run" | "check"): string {
  return [
    "// DEMO FIXTURE — illustrative shape only, not solver-produced.",
    "// No parser, Rust compiler, Alloy solver or replay was executed.",
    "sig Row {}",
    `run QuerySemantics for 3`,
    command === "check" ? `check QueryProperty for 3` : "// no property assertion in this example",
  ].join("\n");
}

/** Build a bundle envelope using only fixture-local provenance. */
export function makeBundle(args: {
  readonly kind: "example" | "counterexample";
  readonly revision: string;
  readonly request: Record<string, unknown>;
  readonly ir: unknown;
  readonly model: string;
  readonly outputSchema: readonly RawColumn[];
  readonly assuranceKind: string;
  readonly source: string;
  readonly maxInstances?: number;
}): Record<string, unknown> {
  return {
    schema: "sqlolla.evidence_bundle.v1",
    kind: args.kind,
    max_instances: args.maxInstances ?? 16,
    model_revision_sha256: args.revision,
    request: args.request,
    ir: args.ir,
    model: args.model,
    output_schema: args.outputSchema,
    assurance_kind: args.assuranceKind,
    provenance: rawProvenance(args.source),
    assumptions: {
      status: "SAT",
      fixture: true,
      note: "Illustrative assumptions; this fixture was not generated by a solver.",
    },
  };
}

/** Append occurrence facts for one row bag under its exact JSON Pointer path. */
function addRows(facts: RawFact[], rows: readonly RawRow[], path: string, kind: string): void {
  rows.forEach((row, index) => {
    const reference = `${path}/${index}`;
    facts.push({ reference, kind, occurrence: index, value: row });
  });
}

/** Append one raw fact per retained effect. */
function addEffects(facts: RawFact[], effects: readonly Record<string, unknown>[]): void {
  effects.forEach((effect, index) => {
    facts.push({
      reference: `/semantic/outcome/outcome/effects/${index}`,
      kind: "effect",
      occurrence: index,
      value: effect,
    });
  });
}

/** Append one raw fact per successful poststate row and output/effects observation. */
function addSuccessFacts(facts: RawFact[], success: RawSuccess): void {
  for (const [table, rows] of Object.entries(success.database)) {
    addRows(facts, rows, `/semantic/outcome/outcome/database/${escapePointerSegment(table)}`, "poststate_row");
  }
  addRows(facts, success.rows, "/semantic/outcome/outcome/rows", "output_row");
  addEffects(facts, success.effects);
}

/** Append raw facts for rows exposed before a modeled SQL rejection. */
function addFailureFacts(facts: RawFact[], failure: RawFailure): void {
  addRows(facts, failure.exposed_rows, "/semantic/outcome/outcome/exposed_rows", "output_row");
  addEffects(facts, failure.effects);
}

/** Build all raw fact pointers exactly where the evidence view links them. */
export function buildFacts(semantic: RawSemantic): readonly RawFact[] {
  const facts: RawFact[] = [];
  for (const [table, rows] of Object.entries(semantic.database)) {
    addRows(facts, rows, `/semantic/database/${escapePointerSegment(table)}`, "input_row");
  }
  if (semantic.output) addRows(facts, semantic.output, "/semantic/output", "output_row");
  if (semantic.outcome?.status === "succeeded") {
    addSuccessFacts(facts, semantic.outcome.outcome as RawSuccess);
  }
  if (semantic.outcome?.status === "sql_error") {
    addFailureFacts(facts, semantic.outcome.outcome as RawFailure);
  }
  return facts;
}

/** Build one immutable fixture instance with complete fact pointers. */
export function makeInstance(args: {
  readonly kind: "example" | "counterexample";
  readonly revision: string;
  readonly semantic: RawSemantic;
  readonly outputSchema: readonly RawColumn[];
  readonly assuranceKind: string;
}): Record<string, unknown> {
  const facts = buildFacts(args.semantic);
  const semanticHash = fixtureHash(`${args.revision}:${JSON.stringify(args.semantic)}`);
  const relations = { "this/Review": [[fixtureHash(`${semanticHash}:atom`)]] };
  const relationsHash = fixtureHash(JSON.stringify(relations));
  return {
    schema: "sqlolla.instance.v1",
    instance_id: fixtureHash(`${args.revision}:${args.kind}:${semanticHash}:${relationsHash}`),
    model_revision_sha256: args.revision,
    kind: args.kind,
    semantic_sha256: semanticHash,
    alloy_relations_sha256: relationsHash,
    assurance_kind: args.assuranceKind,
    validation: "RUST_REPLAY_CONFIRMED",
    semantic: args.semantic,
    alloy_relations: relations,
    output_schema: args.outputSchema,
    facts,
    completeness: { encoded_instance: true, transport: true, view: true },
    enumeration_unit: "alloy_solution",
    text_policy: "labels, SQL and values are untrusted text; escape when rendering",
  };
}

/** Build one fixture journal record. */
export function makeRecord(ordinal: number, previous: string, status: "instance" | "exhausted" | "limit", instance: unknown = null): Record<string, unknown> {
  const solver =
    status === "instance"
      ? { status: "SAT", fixture: true, ordinal, command_kind: "run" }
      : {
          status: status === "exhausted" ? "UNSAT" : "NOT_REQUESTED",
          fixture: true,
          ordinal,
          reason: status === "limit" ? "configured_instance_limit" : "fixture terminal state",
        };
  return {
    schema: "sqlolla.instance_record.v1",
    ordinal,
    previous_record_sha256: previous,
    status,
    instance,
    solver,
  };
}

/** Build an `instances.open` success envelope without inventing a live run. */
export function makeOpenResponse(args: {
  readonly bundleId: string;
  readonly bundle: Record<string, unknown>;
  readonly records: readonly Record<string, unknown>[];
  readonly cursor: string | null;
  readonly claim: string;
}): Record<string, unknown> {
  const current = args.records[args.records.length - 1];
  if (!current) throw new Error("Fixture open response requires a record");
  const revision = String(args.bundle["model_revision_sha256"]);
  const assurance = String(args.bundle["assurance_kind"]);
  const status = String(current["status"]);
  return {
    schema: "sqlolla.enumeration_response.v1",
    status: "ok",
    bundle_id: args.bundleId,
    model_revision_sha256: revision,
    assurance_kind: assurance,
    enumeration_status: status,
    enumeration_exhausted: status === "exhausted",
    cursor: args.cursor,
    record: current,
    records: args.records,
    bundle: args.bundle,
    max_instances: args.bundle["max_instances"],
    claim: args.claim,
  };
}

/** Build a next-step envelope without bundle or records. */
export function makeNextResponse(args: {
  readonly bundleId: string;
  readonly bundle: Record<string, unknown>;
  readonly record: Record<string, unknown>;
  readonly cursor: string | null;
  readonly claim: string;
}): Record<string, unknown> {
  const status = String(args.record["status"]);
  return {
    schema: "sqlolla.enumeration_response.v1",
    status: "ok",
    bundle_id: args.bundleId,
    model_revision_sha256: String(args.bundle["model_revision_sha256"]),
    assurance_kind: String(args.bundle["assurance_kind"]),
    enumeration_status: status,
    enumeration_exhausted: status === "exhausted",
    cursor: args.cursor,
    record: args.record,
    max_instances: args.bundle["max_instances"],
    claim: args.claim,
  };
}

/** Build a terminal record plus matching response at a fixture cursor. */
export function terminalNext(args: {
  readonly bundleId: string;
  readonly bundle: Record<string, unknown>;
  readonly previous: string;
  readonly ordinal: number;
  readonly status: "exhausted" | "limit";
}): Record<string, unknown> {
  const record = makeRecord(args.ordinal, args.previous, args.status);
  return makeNextResponse({
    bundleId: args.bundleId,
    bundle: args.bundle,
    record,
    cursor: null,
    claim: terminalClaim(args.status),
  });
}

/** Plain terminal copy that never implies safety. */
export function terminalClaim(status: "exhausted" | "limit"): string {
  return status === "exhausted"
    ? "No further solver cases within this exact finite model; this is not a global safety claim."
    : "The configured instance limit was reached; enumeration was not exhausted.";
}

/** Build a well-shaped backend error envelope for a fixture state. */
export function makeErrorResponse(code: string, message: string): Record<string, unknown> {
  return {
    schema: "sqlolla.enumeration_response.v1",
    status: "error",
    code,
    message,
    enumeration_exhausted: false,
  };
}
