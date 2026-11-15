/**
 * Runtime-validated evidence types derived from SQLolla's current local
 * instances protocol. Version strings live only in `adapter.ts`.
 */

/** A JSON object as received from the local analysis transport. */
export type JsonObject = Record<string, unknown>;

/** Exact backend type descriptors, retained independently of presentation labels. */
export type SqlTypeModel = string | Readonly<Record<string, unknown>>;

/** One field of an ordered relation schema. */
export interface ColumnModel {
  readonly name: string;
  readonly ty: string;
  readonly sqlType?: SqlTypeModel;
  readonly nullable: boolean;
  readonly qualifier: string | null;
  readonly schema: string | null;
  readonly raw: unknown;
}

/** A typed SQLolla cell, including an explicitly typed NULL. */
export interface CellModel {
  readonly type: string;
  readonly value: unknown;
  readonly raw: unknown;
}

/** One catalog relation with ordered columns. */
export interface TableModel {
  readonly key: string;
  readonly schema: string;
  readonly name: string;
  readonly columns: readonly ColumnModel[];
  readonly raw: unknown;
}

/** The explicit closed catalog supplied with a request. */
export interface CatalogModel {
  readonly searchPath: readonly string[];
  readonly tables: Readonly<Record<string, TableModel>>;
  readonly raw: unknown;
}

/** A supported finite read-result property. */
export type PropertyModel =
  | {
      readonly kind: "only_value" | "no_value";
      readonly column: number;
      readonly value: CellModel;
      readonly raw: unknown;
    }
  | { readonly kind: "max_rows"; readonly max: number; readonly raw: unknown }
  | { readonly kind: "no_sql_error"; readonly raw: unknown };

/** The strict request shape relevant to review and evidence inspection. */
export interface RequestModel {
  readonly sql: string;
  readonly mode: string;
  readonly catalog: CatalogModel;
  readonly parameters: Readonly<Record<string, CellModel>>;
  readonly database: Readonly<Record<string, readonly (readonly CellModel[])[]>>;
  readonly property: PropertyModel | null;
  readonly bounds: Readonly<Record<string, number>>;
  readonly domain: readonly CellModel[];
  readonly includeModel: boolean;
  readonly execution: unknown;
  readonly raw: unknown;
}

/** A bounded observation of a successful statement. */
export interface SuccessObservation {
  readonly database: Readonly<Record<string, readonly (readonly CellModel[])[]>>;
  readonly rows: readonly (readonly CellModel[])[];
  readonly ordering: "bag" | "ordered" | "unknown";
  readonly affected: number;
  readonly commandTag: string;
  readonly effects: readonly unknown[];
  readonly raw: unknown;
}

/** A modeled SQL rejection observed inside otherwise valid solver evidence. */
export interface SqlErrorObservation {
  readonly sqlstate: string;
  readonly phase: string;
  readonly constraintName: string | null;
  readonly columnName: string | null;
  readonly exposedRows: readonly (readonly CellModel[])[];
  readonly effects: readonly unknown[];
  readonly raw: unknown;
}

/** The optional statement-level observation attached to finite evidence. */
export interface OutcomeModel {
  readonly status: "succeeded" | "sql_error";
  readonly success: SuccessObservation | null;
  readonly error: SqlErrorObservation | null;
  readonly raw: unknown;
}

/** The replayed semantic payload of one stored instance. */
export interface SemanticModel {
  readonly database: Readonly<Record<string, readonly (readonly CellModel[])[]>>;
  readonly catalog: CatalogModel | null;
  readonly output: readonly (readonly CellModel[])[] | null;
  readonly outcome: OutcomeModel | null;
  readonly transitionIndex: number | null;
  readonly raw: unknown;
}

/** A stable JSON Pointer fact supplied by the backend. */
export interface FactModel {
  readonly reference: string;
  readonly kind: string;
  readonly occurrence: number | null;
  readonly value: unknown;
  readonly raw: unknown;
}

/** Immutable completeness claims about the encoded instance and view transport. */
export interface CompletenessModel {
  readonly encodedInstance: boolean;
  readonly transport: boolean;
  readonly view: boolean;
}

/** The versioned instance payload consumed by review and presentation.
 */
export interface InstanceModel {
  readonly schema: string;
  readonly instanceId: string;
  readonly modelRevisionSha256: string;
  readonly kind: "example" | "counterexample";
  readonly semanticSha256: string;
  readonly alloyRelationsSha256: string;
  readonly assuranceKind: string;
  readonly validation: string;
  readonly semantic: SemanticModel;
  readonly alloyRelations: unknown;
  readonly outputSchema: readonly ColumnModel[];
  readonly facts: readonly FactModel[];
  readonly completeness: CompletenessModel;
  readonly enumerationUnit: string | null;
  readonly raw: unknown;
}

/** The immutable SQL/IR/Alloy bundle shared by a journal. */
export interface EvidenceBundleModel {
  readonly schema: string;
  readonly kind: "example" | "counterexample";
  readonly maxInstances: number;
  readonly modelRevisionSha256: string;
  readonly request: RequestModel;
  readonly ir: unknown;
  readonly model: string;
  readonly outputSchema: readonly ColumnModel[];
  readonly assuranceKind: string;
  readonly provenance: unknown;
  readonly assumptions: unknown;
  readonly raw: unknown;
}

/** One immutable stored enumeration step. */
export interface EnumerationRecordModel {
  readonly schema: string;
  readonly ordinal: number;
  readonly previousRecordSha256: string;
  readonly status: "instance" | "exhausted" | "limit";
  readonly instance: InstanceModel | null;
  readonly solver: unknown;
  readonly raw: unknown;
}

/** The backend's successful enumeration terminal distinction. */
export type EnumerationStatus = "instance" | "exhausted" | "limit";

/** A structured backend analysis error, never an exhaustion claim. */
export interface EnumerationError {
  readonly kind: "enumeration";
  readonly code: string;
  readonly message: string;
  readonly raw: unknown;
}

/** A frontend contract rejection: unsupported version or invalid shape. */
export interface AdapterFailure {
  readonly kind: "contract";
  readonly code: string;
  readonly message: string;
  readonly raw: unknown;
}

/** A host/transport failure, explicitly separate from solver or SQL outcomes. */
export interface TransportFailure {
  readonly kind: "transport";
  readonly code: string;
  readonly message: string;
  readonly retryable: boolean;
}

/** One occurrence in a before/after/output relation presentation. */
export interface OccurrenceView {
  readonly ordinal: number;
  readonly position: number;
  readonly cells: readonly CellModel[];
  readonly factReference: string | null;
  readonly changedColumns: readonly number[];
}

/** One relation table with exact schema context and ordered occurrences. */
export interface RelationView {
  readonly tableId: string;
  readonly schemaName: string;
  readonly tableName: string;
  readonly columns: readonly ColumnModel[];
  readonly rows: readonly OccurrenceView[];
  readonly schemaSupplied: boolean;
}

/** Ordered output occurrences normalized across native and finite profiles. */
export interface OutputView {
  readonly columns: readonly ColumnModel[];
  readonly rows: readonly OccurrenceView[];
  readonly ordering: "bag" | "ordered" | "unknown";
  readonly factBase: string;
}

/** An effect/event observation shown next to the statement outcome. */
export interface EffectObservation {
  readonly kind: string;
  readonly label: string;
  readonly fields: Readonly<Record<string, string>>;
  readonly reference: string | null;
  readonly raw: unknown;
}

/** A human-readable view of a modeled SQL rejection. */
export interface ErrorObservation {
  readonly sqlstate: string;
  readonly phase: string;
  readonly constraintName: string | null;
  readonly columnName: string | null;
  readonly exposedRows: readonly OccurrenceView[];
  readonly effects: readonly EffectObservation[];
}

/** Statement-level observations made visible without semantic reinterpretation. */
export interface StatementView {
  readonly outcome: "none" | "succeeded" | "sql_error";
  readonly commandTag: string | null;
  readonly affected: number | null;
  readonly ordering: "bag" | "ordered" | "unknown";
  readonly effects: readonly EffectObservation[];
  readonly error: ErrorObservation | null;
  readonly poststateTables: readonly RelationView[];
}

/** One internally checked fact reference and its cross-check result. */
export interface FactIntegrityIssue {
  readonly reference: string;
  readonly reason: string;
}

/** Everything the presentation layer needs; canonical evidence remains untouched. */
export interface EvidenceView {
  readonly bundle: EvidenceBundleModel;
  readonly instance: InstanceModel;
  readonly inputTables: readonly RelationView[];
  readonly output: OutputView;
  readonly statement: StatementView;
  readonly facts: readonly FactModel[];
  readonly invalidFacts: readonly FactIntegrityIssue[];
  readonly typedNullCount: number;
  readonly duplicateInputRows: number;
  readonly textDescription: string;
}

/** A successful open analysis result with its optional reviewed instance. */
export interface OpenedAnalysis {
  readonly kind: "opened";
  readonly bundle: EvidenceBundleModel;
  readonly records: readonly EnumerationRecordModel[];
  readonly current: EnumerationRecordModel;
  readonly evidence: EvidenceView | null;
  readonly bundleId: string;
  readonly modelRevisionSha256: string;
  readonly assuranceKind: string;
  readonly status: EnumerationStatus;
  readonly enumerationExhausted: boolean;
  readonly cursor: string | null;
  readonly maxInstances: number | null;
  readonly claim: string | null;
  readonly raw: unknown;
}

/** One successful next step, retaining no bundle in the backend response. */
export interface NextStep {
  readonly kind: "step";
  readonly current: EnumerationRecordModel;
  readonly evidence: EvidenceView | null;
  readonly bundleId: string;
  readonly modelRevisionSha256: string;
  readonly assuranceKind: string;
  readonly status: EnumerationStatus;
  readonly enumerationExhausted: boolean;
  readonly cursor: string | null;
  readonly maxInstances: number | null;
  readonly claim: string | null;
  readonly raw: unknown;
}

/** A transport-neutral read of `instances.open` or `instances.next`. */
export type OpenResult = { readonly ok: true; readonly value: OpenedAnalysis } | { readonly ok: false; readonly error: AdapterFailure | EnumerationError };

/** A transport-neutral read of `instances.next`. */
export type NextResult = { readonly ok: true; readonly value: NextStep } | { readonly ok: false; readonly error: AdapterFailure | EnumerationError };

/** One real backend command shape for a future browser host bridge. */
export interface OpenCommand {
  readonly action: "instances.open";
  readonly bundle_id: string;
}

/** One real backend command shape for a future browser host bridge. */
export interface NextCommand {
  readonly action: "instances.next";
  readonly cursor: string;
}

/** The strict start envelope forwarded unchanged to the local host. */
export interface StartCommand {
  readonly action: "instances.start";
  readonly request: unknown;
  readonly kind: "example" | "counterexample";
  readonly max_instances: number;
}

/** Same-origin solver transport with explicit fixture injection for UI tests. */
export interface AnalysisTransport {
  readonly id: string;
  readonly label: string;
  readonly kind: "fixture" | "local-host";
  readonly fixtures?: readonly FixtureScenario[];
  open(command: OpenCommand): Promise<TransportResult>;
  next(command: NextCommand): Promise<TransportResult>;
  start(command: StartCommand): Promise<TransportResult>;
  command?(request: unknown): Promise<TransportResult>;
}

/** A transport result keeps host failures out of solver and SQL outcomes. */
export type TransportResult = { readonly ok: true; readonly payload: unknown } | { readonly ok: false; readonly error: TransportFailure };

/** An illustrative explanation supplied outside canonical backend evidence. */
export interface DemoClaim {
  readonly text: string;
  readonly factReferences: readonly string[];
}

/** A fixture explanation, clearly separated from evidence and provider output. */
export interface DemoSummary {
  readonly title: string;
  readonly claims: readonly DemoClaim[];
  readonly assumptions: readonly string[];
  readonly proposedEdit: string | null;
  readonly proposedEditLabel: string | null;
  readonly demonstrated: boolean;
}

/** Scenario-owned explanation metadata; claims are built from supplied facts. */
export interface DemoSummarySeed {
  readonly title: string;
  readonly assumptions: readonly string[];
  readonly proposedEdit: string | null;
  readonly proposedEditLabel: string | null;
}

/** Fixture provenance displayed to make illustrative data unmistakable. */
export interface FixtureProvenance {
  readonly label: "Transformed repository evidence" | "Illustrative demo fixture" | "Local solver evidence";
  readonly source: string;
  readonly note: string;
}

/** A library scenario assembled from versioned raw evidence plus demo metadata. */
export interface FixtureScenario {
  readonly id: string;
  readonly shortTitle: string;
  readonly title: string;
  readonly description: string;
  readonly group: "primary" | "states";
  readonly statusLabel: string;
  readonly bundleId: string | null;
  readonly provenance: FixtureProvenance;
  readonly summarySeed: DemoSummarySeed | null;
  readonly openPayload: unknown;
  readonly nextByCursor: Readonly<Record<string, unknown>>;
  readonly builderCatalog: CatalogModel | null;
  readonly transportFailure: TransportFailure | null;
}
