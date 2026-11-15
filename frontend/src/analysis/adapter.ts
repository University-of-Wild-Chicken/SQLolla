/**
 * The single version adapter for SQLolla's current local evidence protocol.
 * Unsupported schema versions are rejected before any field is trusted, and
 * evidence JSON is never rewritten.
 */
import { buildEvidenceView } from "./evidence";
import {
  asArray,
  asRecord,
  booleanField,
  GuardError,
  hashField,
  integerField,
  isRecord,
  literalField,
  optionalStringField,
  requireSchema,
  stringArray,
  stringField,
} from "./guards";
import { canonicalJson } from "./format";
import { parseSqlType, sqlTypeText } from "./sqlTypes";
import { integerText, originalJson, stringifyJson } from "./json";
import type {
  AdapterFailure,
  CatalogModel,
  CellModel,
  ColumnModel,
  CompletenessModel,
  EnumerationError,
  EnumerationRecordModel,
  EnumerationStatus,
  EvidenceBundleModel,
  FactModel,
  InstanceModel,
  JsonObject,
  NextResult,
  NextStep,
  OpenedAnalysis,
  OpenResult,
  OutcomeModel,
  PropertyModel,
  RequestModel,
  SqlErrorObservation,
  SuccessObservation,
  TableModel,
} from "./types";

const ENUMERATION_SCHEMA = "sqlolla.enumeration_response.v1";
const BUNDLE_SCHEMA = "sqlolla.evidence_bundle.v1";
const INSTANCE_SCHEMA = "sqlolla.instance.v1";
const RECORD_SCHEMA = "sqlolla.instance_record.v1";

/** Convert a caught guard error into a contract result without hiding its path. */
function contractFailure(error: unknown, raw: unknown): AdapterFailure {
  if (error instanceof GuardError) {
    return { kind: "contract", code: error.code, message: error.message, raw };
  }
  const message = error instanceof Error ? error.message : "Unknown evidence shape failure";
  return { kind: "contract", code: "contract.shape", message, raw };
}

/** Preserve an explicit backend error envelope only after validating its shape. */
function parseErrorEnvelope(raw: unknown): EnumerationError {
  const object = asRecord(raw, "response");
  requireSchema(object, ENUMERATION_SCHEMA, "response");
  if (object["status"] !== "error") {
    throw new GuardError("contract.shape", "response.status must be error");
  }
  const code = stringField(object, "code", "response");
  const message = stringField(object, "message", "response");
  if (object["enumeration_exhausted"] !== false) {
    throw new GuardError("contract.shape", "An error response must state enumeration_exhausted=false");
  }
  return { kind: "enumeration", code, message, raw };
}

/** Parse one ordered schema column. */
function parseColumn(value: unknown, path: string): ColumnModel {
  const object = asRecord(value, path);
  const sqlType = parseSqlType(object["ty"], `${path}.ty`);
  return {
    name: stringField(object, "name", path),
    ty: sqlTypeText(sqlType),
    sqlType,
    nullable: booleanField(object, "nullable", path),
    qualifier: optionalStringField(object, "qualifier", path, null),
    schema: optionalStringField(object, "schema", path, null),
    raw: value,
  };
}

/** Parse an ordered column array. */
function parseColumns(value: unknown, path: string): readonly ColumnModel[] {
  return asArray(value, path).map((column, index) => parseColumn(column, `${path}[${index}]`));
}

/** Parse one typed SQLolla cell, including an explicit typed NULL. */
function parseCell(value: unknown, path: string): CellModel {
  const object = asRecord(value, path);
  const type = stringField(object, "type", path);
  if (!Object.prototype.hasOwnProperty.call(object, "value")) {
    throw new GuardError("contract.shape", `${path}.value is required`);
  }
  if (type === "null") parseSqlType(object["value"], `${path}.value`);
  if (type === "int64" && integerText(object["value"]) === null) {
    throw new GuardError("contract.integer_precision", `${path} contains an imprecise integer`);
  }
  return { type, value: object["value"], raw: value };
}

/** Parse one ordered row of typed cells. */
function parseRow(value: unknown, path: string): readonly CellModel[] {
  return asArray(value, path).map((cell, index) => parseCell(cell, `${path}[${index}]`));
}

/** Parse an array of typed rows. */
function parseRows(value: unknown, path: string): readonly (readonly CellModel[])[] {
  return asArray(value, path).map((row, index) => parseRow(row, `${path}[${index}]`));
}

/** Parse a table-to-row-bags database. */
function parseDatabase(value: unknown, path: string): Readonly<Record<string, readonly (readonly CellModel[])[]>> {
  const object = asRecord(value, path);
  return Object.fromEntries(Object.entries(object).map(([table, rows]) => [table, parseRows(rows, `${path}.${table}`)]));
}

/** Parse one explicitly typed numbered parameter. */
function parseParameters(value: unknown, path: string): Readonly<Record<string, CellModel>> {
  const object = asRecord(value, path);
  return Object.fromEntries(
    Object.entries(object).map(([key, cell]) => {
      if (!/^\d+$/.test(key)) throw new GuardError("contract.shape", `${path} keys must be numeric`);
      return [key, parseCell(cell, `${path}.${key}`)];
    }),
  );
}

/** Parse the explicit catalog that grounds relation names and column types. */
function parseCatalog(value: unknown, path: string): CatalogModel {
  const object = asRecord(value, path);
  const tables = asRecord(object["tables"], `${path}.tables`);
  const parsed = Object.fromEntries(Object.entries(tables).map(([key, table]) => [key, parseTable(table, `${path}.tables.${key}`, key)]));
  return {
    searchPath: stringArray(object["search_path"], `${path}.search_path`),
    tables: parsed,
    raw: value,
  };
}

/** Parse one explicit catalog table and its stable identifier. */
function parseTable(value: unknown, path: string, key: string): TableModel {
  const object = asRecord(value, path);
  return {
    key,
    schema: stringField(object, "schema", path),
    name: stringField(object, "name", path),
    columns: parseColumns(object["columns"], `${path}.columns`),
    raw: value,
  };
}

/** Parse a supported finite read-result property. */
function parseProperty(value: unknown, path: string): PropertyModel {
  const object = asRecord(value, path);
  const kind = literalField(object, "kind", path, ["only_value", "no_value", "max_rows", "no_sql_error"] as const);
  if (kind === "no_sql_error") return { kind, raw: value };
  if (kind === "max_rows") {
    return { kind, max: integerField(object, "max", path), raw: value };
  }
  return {
    kind,
    column: integerField(object, "column", path),
    value: parseCell(object["value"], `${path}.value`),
    raw: value,
  };
}

/** Parse symbolic bounds as non-negative finite capacities. */
function parseBounds(value: unknown, path: string): Readonly<Record<string, number>> {
  const object = asRecord(value, path);
  return Object.fromEntries(
    Object.entries(object).map(([key, bound]) => {
      if (typeof bound !== "number" || !Number.isInteger(bound) || bound < 0) {
        throw new GuardError("contract.shape", `${path}.${key} must be a non-negative integer`);
      }
      return [key, bound];
    }),
  );
}

/** Parse the strict request fields used by review and details views. */
function parseRequest(value: unknown, path: string): RequestModel {
  const object = asRecord(value, path);
  const catalog = parseCatalog(object["catalog"], `${path}.catalog`);
  const database = object["database"] === undefined ? {} : parseDatabase(object["database"], `${path}.database`);
  const parameters = object["parameters"] === undefined ? {} : parseParameters(object["parameters"], `${path}.parameters`);
  const property = object["property"] === undefined || object["property"] === null ? null : parseProperty(object["property"], `${path}.property`);
  return {
    sql: stringField(object, "sql", path),
    mode: stringField(object, "mode", path),
    catalog,
    parameters,
    database,
    property,
    bounds: object["bounds"] === undefined ? {} : parseBounds(object["bounds"], `${path}.bounds`),
    domain: object["domain"] === undefined ? [] : parseRowsDomain(object["domain"], `${path}.domain`),
    includeModel: object["include_model"] === true,
    execution: object["execution"] ?? null,
    raw: value,
  };
}

/** Parse a symbolic value domain as one ordered list of typed cells. */
function parseRowsDomain(value: unknown, path: string): readonly CellModel[] {
  return asArray(value, path).map((cell, index) => parseCell(cell, `${path}[${index}]`));
}

/** Validate one of the three declared workflow modes. */
function validateMode(mode: string): void {
  if (!["compare", "compile", "analyze"].includes(mode)) {
    throw new GuardError("contract.workflow", `Unsupported request mode "${mode}"`);
  }
}

/** Validate symbolic-only fields and required subject context. */
function validateAnalyze(request: RequestModel): void {
  if (Object.keys(request.database).length > 0) {
    throw new GuardError("contract.workflow", "Symbolic analysis must not include a concrete database");
  }
  if (!request.property) throw new GuardError("contract.workflow", "Analyze requires a property");
}

/** Validate concrete-only fields and the complete database requirement. */
function validateConcrete(request: RequestModel): void {
  if (request.property || Object.keys(request.bounds).length > 0 || request.domain.length > 0) {
    throw new GuardError("contract.workflow", "Concrete modes must not mix symbolic fields");
  }
  const missingTable = Object.keys(request.catalog.tables).find((table) => !(table in request.database));
  if (missingTable) {
    throw new GuardError("contract.missing_context", `Concrete request omits catalog table "${missingTable}"; complete database required`);
  }
}

/** Validate bound table identities and the extra counterexample context. */
function validateSymbolicBounds(request: RequestModel, kind: "example" | "counterexample"): void {
  if (Object.keys(request.bounds).some((table) => !(table in request.catalog.tables))) {
    throw new GuardError("contract.workflow", "A bound refers to an unknown catalog table");
  }
  if (kind !== "counterexample") return;
  if (request.property === null) {
    throw new GuardError("contract.workflow", "Counterexample needs a property");
  }
  if (request.domain.length === 0) {
    throw new GuardError("contract.workflow", "Counterexample needs a finite domain");
  }
  if (Object.keys(request.bounds).length === 0) {
    throw new GuardError("contract.workflow", "Counterexample needs bounds");
  }
}

/** Check the request workflow distinctions without parsing or executing SQL. */
function validateRequestWorkflow(request: RequestModel, kind: "example" | "counterexample"): void {
  validateMode(request.mode);
  if (request.mode === "analyze") validateAnalyze(request);
  else validateConcrete(request);
  validateSymbolicBounds(request, kind);
}

/** Parse an immutable bundle after validating its schema version. */
function parseBundle(value: unknown): EvidenceBundleModel {
  const object = asRecord(value, "bundle");
  requireSchema(object, BUNDLE_SCHEMA, "bundle");
  const kind = literalField(object, "kind", "bundle", ["example", "counterexample"] as const);
  const request = parseRequest(object["request"], "bundle.request");
  validateRequestWorkflow(request, kind);
  const maxInstances = integerField(object, "max_instances", "bundle");
  if (maxInstances < 1 || maxInstances > 256) {
    throw new GuardError("contract.shape", "bundle.max_instances must be between 1 and 256");
  }
  return {
    schema: BUNDLE_SCHEMA,
    kind,
    maxInstances,
    modelRevisionSha256: hashField(object, "model_revision_sha256", "bundle"),
    request,
    ir: object["ir"],
    model: stringField(object, "model", "bundle"),
    outputSchema: parseColumns(object["output_schema"], "bundle.output_schema"),
    assuranceKind: stringField(object, "assurance_kind", "bundle"),
    provenance: object["provenance"],
    assumptions: object["assumptions"],
    raw: value,
  };
}

/** Parse one stable JSON Pointer fact without interpreting its value as instruction. */
function parseFact(value: unknown, path: string): FactModel {
  const object = asRecord(value, path);
  const reference = stringField(object, "reference", path);
  if (!reference.startsWith("/")) {
    throw new GuardError("contract.shape", `${path}.reference must be a JSON Pointer`);
  }
  const occurrence = object["occurrence"];
  return {
    reference,
    kind: stringField(object, "kind", path),
    occurrence:
      occurrence === undefined || occurrence === null
        ? null
        : (() => {
            if (typeof occurrence !== "number" || !Number.isInteger(occurrence) || occurrence < 0) {
              throw new GuardError("contract.shape", `${path}.occurrence must be a non-negative integer`);
            }
            return occurrence;
          })(),
    value: object["value"],
    raw: value,
  };
}

/** Parse all supplied facts while preserving their references exactly. */
function parseFacts(value: unknown): readonly FactModel[] {
  return asArray(value, "instance.facts").map((fact, index) => parseFact(fact, `instance.facts[${index}]`));
}

/** Parse the three separate encoded/transport/view completeness flags. */
function parseCompleteness(value: unknown): CompletenessModel {
  const object = asRecord(value, "instance.completeness");
  return {
    encodedInstance: booleanField(object, "encoded_instance", "instance.completeness"),
    transport: booleanField(object, "transport", "instance.completeness"),
    view: booleanField(object, "view", "instance.completeness"),
  };
}

/** Parse a successful statement observation, including its ordered rows. */
function parseSuccessObservation(value: unknown): SuccessObservation {
  const object = asRecord(value, "semantic.outcome.outcome");
  return {
    database: parseDatabase(object["database"], "semantic.outcome.outcome.database"),
    rows: parseRows(object["rows"], "semantic.outcome.outcome.rows"),
    ordering: literalField(object, "ordering", "semantic.outcome.outcome", ["bag", "ordered"] as const),
    affected: integerField(object, "affected", "semantic.outcome.outcome"),
    commandTag: stringField(object, "command_tag", "semantic.outcome.outcome"),
    effects: asArray(object["effects"], "semantic.outcome.outcome.effects"),
    raw: value,
  };
}

/** Parse a modeled SQL rejection without turning it into an outer analysis error. */
function parseSqlErrorObservation(value: unknown): SqlErrorObservation {
  const object = asRecord(value, "semantic.outcome.outcome");
  return {
    sqlstate: stringField(object, "sqlstate", "semantic.outcome.outcome"),
    phase: stringField(object, "phase", "semantic.outcome.outcome"),
    constraintName: optionalStringField(object, "constraint_name", "semantic.outcome.outcome", null),
    columnName: optionalStringField(object, "column_name", "semantic.outcome.outcome", null),
    exposedRows: parseRows(object["exposed_rows"], "semantic.outcome.outcome.exposed_rows"),
    effects: asArray(object["effects"], "semantic.outcome.outcome.effects"),
    raw: value,
  };
}

/** Parse a finite success/error statement outcome. */
function parseOutcome(value: unknown): OutcomeModel {
  const object = asRecord(value, "semantic.outcome");
  const status = literalField(object, "status", "semantic.outcome", ["succeeded", "sql_error"] as const);
  const payload = asRecord(object["outcome"], "semantic.outcome.outcome");
  return {
    status,
    success: status === "succeeded" ? parseSuccessObservation(payload) : null,
    error: status === "sql_error" ? parseSqlErrorObservation(payload) : null,
    raw: value,
  };
}

/** Parse the replayed semantic payload without deriving execution semantics. */
function parseSemantic(value: unknown): InstanceModel["semantic"] {
  const object = asRecord(value, "instance.semantic");
  const catalog = object["catalog"] === undefined || object["catalog"] === null ? null : parseCatalog(object["catalog"], "instance.semantic.catalog");
  const output = object["output"] === undefined || object["output"] === null ? null : parseRows(object["output"], "instance.semantic.output");
  const outcome = object["outcome"] === undefined || object["outcome"] === null ? null : parseOutcome(object["outcome"]);
  const transitionIndex = object["transition_index"];
  return {
    database: parseDatabase(object["database"], "instance.semantic.database"),
    catalog,
    output,
    outcome,
    transitionIndex: typeof transitionIndex === "number" ? integerField(object, "transition_index", "instance.semantic") : null,
    raw: value,
  };
}

/** Parse the immutable instance identity and its supplied evidence. */
function parseInstance(value: unknown): InstanceModel {
  const object = asRecord(value, "instance");
  requireSchema(object, INSTANCE_SCHEMA, "instance");
  return {
    schema: INSTANCE_SCHEMA,
    instanceId: hashField(object, "instance_id", "instance"),
    modelRevisionSha256: hashField(object, "model_revision_sha256", "instance"),
    kind: literalField(object, "kind", "instance", ["example", "counterexample"] as const),
    semanticSha256: hashField(object, "semantic_sha256", "instance"),
    alloyRelationsSha256: hashField(object, "alloy_relations_sha256", "instance"),
    assuranceKind: stringField(object, "assurance_kind", "instance"),
    validation: stringField(object, "validation", "instance"),
    semantic: parseSemantic(object["semantic"]),
    alloyRelations: object["alloy_relations"],
    outputSchema: parseColumns(object["output_schema"], "instance.output_schema"),
    facts: parseFacts(object["facts"]),
    completeness: parseCompleteness(object["completeness"]),
    enumerationUnit: optionalStringField(object, "enumeration_unit", "instance", null),
    raw: value,
  };
}

/** Parse one immutable enum record and enforce empty terminals. */
function parseRecord(value: unknown): EnumerationRecordModel {
  const object = asRecord(value, "record");
  requireSchema(object, RECORD_SCHEMA, "record");
  const status = literalField(object, "status", "record", ["instance", "exhausted", "limit"] as const);
  const instance = status === "instance" ? parseInstance(object["instance"]) : null;
  if (status !== "instance" && object["instance"] !== null) {
    throw new GuardError("contract.shape", "A terminal record must not carry an instance");
  }
  if (status === "instance" && object["instance"] === null) {
    throw new GuardError("contract.shape", "An instance record must carry an instance");
  }
  return {
    schema: RECORD_SCHEMA,
    ordinal: integerField(object, "ordinal", "record"),
    previousRecordSha256: hashField(object, "previous_record_sha256", "record"),
    status,
    instance,
    solver: asRecord(object["solver"], "record.solver"),
    raw: value,
  };
}

interface ParsedEnvelope {
  readonly status: EnumerationStatus;
  readonly record: EnumerationRecordModel;
  readonly bundleId: string;
  readonly revision: string;
  readonly assuranceKind: string;
  readonly exhausted: boolean;
  readonly cursor: string | null;
  readonly maxInstances: number | null;
  readonly claim: string | null;
}

/** Validate the cursor/exhaustion invariants of a successful enumeration response. */
function validateEnvelopeState(status: EnumerationStatus, exhausted: boolean, cursor: string | null): void {
  if (exhausted !== (status === "exhausted")) {
    throw new GuardError("contract.shape", "enumeration_exhausted must be true only for exhausted");
  }
  if ((status === "instance") !== (cursor !== null)) {
    throw new GuardError("contract.shape", "Only instance responses may carry a cursor");
  }
}

/** Parse the state fields shared by open and next envelopes. */
function parseEnvelopeState(object: Record<string, unknown>): {
  status: EnumerationStatus;
  cursor: string | null;
  exhausted: boolean;
} {
  const status = literalField(object, "enumeration_status", "response", ["instance", "exhausted", "limit"] as const);
  const exhausted = booleanField(object, "enumeration_exhausted", "response");
  const cursor = optionalStringField(object, "cursor", "response", null);
  validateEnvelopeState(status, exhausted, cursor);
  return { status, cursor, exhausted };
}

/**
 * Parse the shared successful enumeration fields, including the terminal
 * distinction and the record's required state.
 */
function parseEnvelope(raw: unknown): ParsedEnvelope {
  const object = asRecord(raw, "response");
  requireSchema(object, ENUMERATION_SCHEMA, "response");
  if (object["status"] !== "ok") {
    throw new GuardError("contract.shape", "response.status must be ok");
  }
  const state = parseEnvelopeState(object);
  const record = parseRecord(object["record"]);
  if (record.status !== state.status) {
    throw new GuardError("contract.shape", "Enumeration status and record status differ");
  }
  return {
    ...state,
    record,
    bundleId: hashField(object, "bundle_id", "response"),
    revision: hashField(object, "model_revision_sha256", "response"),
    assuranceKind: stringField(object, "assurance_kind", "response"),
    maxInstances: object["max_instances"] === undefined ? null : integerField(object, "max_instances", "response"),
    claim: optionalStringField(object, "claim", "response", null),
  };
}

/** Parse and cross-check an `instances.open` payload. */
export function parseOpenPayload(raw: unknown): OpenResult {
  try {
    if (isRecord(raw) && raw["status"] === "error") {
      return { ok: false, error: parseErrorEnvelope(raw) };
    }
    const object = asRecord(raw, "open response");
    requireSchema(object, ENUMERATION_SCHEMA, "open response");
    const bundle = parseBundle(object["bundle"]);
    const records = asArray(object["records"], "open response.records").map(parseRecord);
    if (records.length === 0) throw new GuardError("contract.shape", "records must not be empty");
    const envelope = parseEnvelope(raw);
    if (envelope.revision !== bundle.modelRevisionSha256) {
      throw new GuardError("contract.stale_revision", "Open response model revision differs from its immutable bundle");
    }
    const last = records[records.length - 1];
    if (!last || canonicalJson(last.raw) !== canonicalJson(envelope.record.raw)) {
      throw new GuardError("contract.shape", "open record is not the last committed record");
    }
    return {
      ok: true,
      value: buildOpened(bundle, records, envelope, raw),
    };
  } catch (error) {
    return { ok: false, error: contractFailure(error, raw) };
  }
}

/** Validate a start envelope without treating its partial response as an opened evidence bundle. */
export function startedIdentity(raw: unknown): { bundleId: string; revision: string } {
  if (isRecord(raw) && raw["status"] === "error") {
    const failure = parseErrorEnvelope(raw);
    throw new GuardError(failure.code, failure.message);
  }
  const envelope = parseEnvelope(raw);
  return { bundleId: envelope.bundleId, revision: envelope.revision };
}

/** A freshly started revision must reopen with exactly the identity returned by its start operation. */
export function parseStartedOpen(raw: unknown, identity: ReturnType<typeof startedIdentity>): OpenResult {
  const result = parseOpenPayload(raw);
  if (!result.ok) return result;
  if (result.value.bundleId !== identity.bundleId || result.value.modelRevisionSha256 !== identity.revision) {
    return { ok: false, error: { kind: "contract", code: "contract.stale_revision", message: "Reopened analysis differs from the started revision", raw } };
  }
  return result;
}

/** Assemble a validated open view and derive presentation-only relations. */
function buildOpened(
  bundle: EvidenceBundleModel,
  records: readonly EnumerationRecordModel[],
  envelope: ReturnType<typeof parseEnvelope>,
  raw: unknown,
): OpenedAnalysis {
  const instance = envelope.record.instance;
  assertInstanceMatchesBundle(instance, bundle);
  const evidence = instance ? buildEvidenceView(bundle, instance) : null;
  return {
    kind: "opened",
    bundle,
    records,
    current: envelope.record,
    evidence,
    bundleId: envelope.bundleId,
    modelRevisionSha256: envelope.revision,
    assuranceKind: envelope.assuranceKind,
    status: envelope.status,
    enumerationExhausted: envelope.exhausted,
    cursor: envelope.cursor,
    maxInstances: envelope.maxInstances ?? bundle.maxInstances,
    claim: envelope.claim,
    raw,
  };
}

/** Reject an instance that claims a revision or kind outside its bundle. */
function assertInstanceMatchesBundle(instance: InstanceModel | null, bundle: EvidenceBundleModel): void {
  if (!instance) return;
  if (instance.modelRevisionSha256 !== bundle.modelRevisionSha256) {
    throw new GuardError("contract.stale_revision", "Instance revision differs from its bundle");
  }
  if (instance.kind !== bundle.kind) {
    throw new GuardError("contract.workflow", "Instance kind differs from its bundle");
  }
}

/** Reject a next response that belongs to another immutable revision. */
function assertNextRevision(envelope: ParsedEnvelope, bundle: EvidenceBundleModel, expectedBundleId: string): void {
  if (envelope.revision !== bundle.modelRevisionSha256 || envelope.bundleId !== expectedBundleId) {
    throw new GuardError("contract.stale_revision", "Next response belongs to a different immutable revision");
  }
}

/** Cross-check a next envelope against the currently open immutable bundle. */
function buildNextStep(raw: unknown, bundle: EvidenceBundleModel, expectedBundleId: string): NextStep {
  const object = asRecord(raw, "next response");
  requireSchema(object, ENUMERATION_SCHEMA, "next response");
  const envelope = parseEnvelope(raw);
  assertNextRevision(envelope, bundle, expectedBundleId);
  const instance = envelope.record.instance;
  assertInstanceMatchesBundle(instance, bundle);
  return {
    kind: "step",
    current: envelope.record,
    evidence: instance ? buildEvidenceView(bundle, instance) : null,
    bundleId: envelope.bundleId,
    modelRevisionSha256: envelope.revision,
    assuranceKind: envelope.assuranceKind,
    status: envelope.status,
    enumerationExhausted: envelope.exhausted,
    cursor: envelope.cursor,
    maxInstances: envelope.maxInstances,
    claim: envelope.claim,
    raw,
  };
}

/** Interpret one next payload, including a structured backend error envelope. */
function parseNextOrError(raw: unknown, bundle: EvidenceBundleModel, expectedBundleId: string): NextResult {
  if (isRecord(raw) && raw["status"] === "error") return { ok: false, error: parseErrorEnvelope(raw) };
  return { ok: true, value: buildNextStep(raw, bundle, expectedBundleId) };
}

/** Parse and cross-check an `instances.next` payload against the open bundle. */
export function parseNextPayload(raw: unknown, bundle: EvidenceBundleModel, expectedBundleId: string): NextResult {
  try {
    return parseNextOrError(raw, bundle, expectedBundleId);
  } catch (error) {
    return { ok: false, error: contractFailure(error, raw) };
  }
}

/** Describe an error source for UI classification without asserting safety. */
export function errorSource(error: AdapterFailure | EnumerationError): "contract" | "enumeration" {
  return error.kind;
}

/** Expose the raw JSON of any parsed payload for local inspection/download. */
export function payloadJson(raw: JsonObject | unknown): string {
  return originalJson(raw) ?? stringifyJson(raw, 2);
}
