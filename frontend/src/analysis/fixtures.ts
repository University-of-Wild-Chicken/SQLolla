/**
 * Explicit, provenance-labelled demo scenarios. Evidence payloads mimic the
 * backend's current schemas; they are illustrative and were not produced by a
 * parser, compiler, Alloy solver or Rust replay. The adapter still validates
 * every shape at runtime.
 */
import {
  catalogModel,
  demoAlloy,
  demoProvenance,
  fixtureHash,
  int32,
  int64,
  makeBundle,
  makeErrorResponse,
  makeInstance,
  makeNextResponse,
  makeOpenResponse,
  makeRecord,
  readIr,
  scanIr,
  terminalClaim,
  terminalNext,
  text,
  typedNull,
  type RawCatalog,
  type RawColumn,
  type RawDatabase,
  type RawRow,
  type RawSemantic,
} from "./fixtureFactory";
import type { CatalogModel, FixtureScenario, TransportFailure } from "./types";

const orderColumns: readonly RawColumn[] = [
  { name: "order_ref", ty: "text", nullable: false, qualifier: null, schema: null },
  { name: "tenant_id", ty: "int32", nullable: true, qualifier: null, schema: null },
  { name: "note", ty: "text", nullable: true, qualifier: null, schema: null },
];

const scopedColumns: readonly RawColumn[] = [
  { name: "order_ref", ty: "text", nullable: false, qualifier: null, schema: null },
  { name: "tenant_id", ty: "int32", nullable: false, qualifier: null, schema: null },
  { name: "customer_name", ty: "text", nullable: true, qualifier: null, schema: null },
  { name: "note", ty: "text", nullable: true, qualifier: null, schema: null },
];

const inventoryColumns: readonly RawColumn[] = [
  { name: "sku", ty: "text", nullable: false, qualifier: null, schema: null },
  { name: "on_hand", ty: "int32", nullable: false, qualifier: null, schema: null },
];

const mutationOutput: readonly RawColumn[] = [
  { name: "sku", ty: "text", nullable: false, qualifier: null, schema: null },
  { name: "on_hand", ty: "int32", nullable: false, qualifier: null, schema: null },
  { name: "event_id", ty: "int64", nullable: false, qualifier: null, schema: null },
];

const tenantCatalog: RawCatalog = {
  search_path: ["public"],
  tables: {
    "public.orders": { schema: "public", name: "orders", columns: orderColumns },
  },
};

const scopedCatalog: RawCatalog = {
  search_path: ["public"],
  tables: {
    "public.orders": { schema: "public", name: "orders", columns: scopedColumns },
  },
};

const inventoryCatalog: RawCatalog = {
  search_path: ["public"],
  tables: {
    "public.inventory": { schema: "public", name: "inventory", columns: inventoryColumns },
  },
};

const builderCatalogRaw: RawCatalog = {
  search_path: ["public"],
  tables: {
    "public.orders": {
      schema: "public",
      name: "orders",
      columns: [
        { name: "order_ref", ty: "text", nullable: false, qualifier: null, schema: null },
        { name: "tenant_id", ty: "int32", nullable: true, qualifier: null, schema: null },
        { name: "customer_id", ty: "int32", nullable: true, qualifier: null, schema: null },
        { name: "note", ty: "text", nullable: true, qualifier: null, schema: null },
      ],
    },
    "public.customers": {
      schema: "public",
      name: "customers",
      columns: [
        { name: "customer_id", ty: "int32", nullable: false, qualifier: null, schema: null },
        { name: "display_name", ty: "text", nullable: true, qualifier: null, schema: null },
        { name: "region", ty: "text", nullable: true, qualifier: null, schema: null },
      ],
    },
    "public.inventory": {
      schema: "public",
      name: "inventory",
      columns: [
        { name: "sku", ty: "text", nullable: false, qualifier: null, schema: null },
        { name: "on_hand", ty: "int32", nullable: false, qualifier: null, schema: null },
        { name: "reorder_point", ty: "int32", nullable: false, qualifier: null, schema: null },
      ],
    },
  },
};

/** Explicit multi-table schema available to the draft query builder. */
export const BUILDER_CATALOG: CatalogModel = catalogModel(builderCatalogRaw);

const TENANT_REVISION = fixtureHash("revision:tenant");
const TENANT_BUNDLE_ID = fixtureHash("bundle:tenant");
const TENANT_CURSOR_1 = "fixture-cursor-tenant-1";
const TENANT_CURSOR_2 = "fixture-cursor-tenant-2";

const tenantRows: readonly RawRow[] = [
  [text("ORD-1007"), int32(7), text("Annual renewal")],
  [text("ORD-1007"), int32(7), text("Annual renewal")],
  [text("ORD-2041"), int32(42), text("Acme export — 東京")],
  [text("ORD-3099"), typedNull("int32"), text("No tenant assigned")],
  [text("ORD-4099"), int32(42), text("<script>alert('x')</script> \" onload=\"alert(1)")],
];

const tenantNextRows: readonly RawRow[] = [
  [text("ORD-5001"), int32(42), text("Second tenant leak")],
  [text("ORD-6002"), typedNull("int32"), text("Another unassigned order")],
];

const tenantRequest = {
  sql: "SELECT order_ref, tenant_id, note\nFROM public.orders",
  catalog: tenantCatalog,
  database: {},
  parameters: {},
  mode: "analyze",
  property: { kind: "only_value", column: 1, value: int32(7) },
  bounds: { "public.orders": 6 },
  domain: [typedNull("int32"), int32(7), int32(42)],
  include_model: true,
};

const tenantBundle = makeBundle({
  kind: "counterexample",
  revision: TENANT_REVISION,
  request: tenantRequest,
  ir: scanIr("public.orders", orderColumns),
  model: demoAlloy("check"),
  outputSchema: orderColumns,
  assuranceKind: "native_ra_alloy_with_rust_replay",
  source: "examples/translation-pairs.json (tenant-missing-filter witness), structurally transformed",
});

const tenantSemantic: RawSemantic = {
  database: { "public.orders": tenantRows },
  catalog: tenantCatalog,
  output: tenantRows,
};

const tenantInstance = makeInstance({
  kind: "counterexample",
  revision: TENANT_REVISION,
  semantic: tenantSemantic,
  outputSchema: orderColumns,
  assuranceKind: "native_ra_alloy_with_rust_replay",
});

const tenantRecord0 = makeRecord(0, TENANT_BUNDLE_ID, "instance", tenantInstance);
const tenantRecord1 = makeRecord(
  1,
  fixtureHash("tenant:record:0"),
  "instance",
  makeInstance({
    kind: "counterexample",
    revision: TENANT_REVISION,
    semantic: {
      database: { "public.orders": tenantNextRows },
      catalog: tenantCatalog,
      output: tenantNextRows,
    },
    outputSchema: orderColumns,
    assuranceKind: "native_ra_alloy_with_rust_replay",
  }),
);
const tenantClaim = "A counterexample exists within this exact finite model, property, domain and bounds. It is not a claim about all PostgreSQL behavior.";

const tenantOpen = makeOpenResponse({
  bundleId: TENANT_BUNDLE_ID,
  bundle: tenantBundle,
  records: [tenantRecord0],
  cursor: TENANT_CURSOR_1,
  claim: tenantClaim,
});

const tenantNextOne = makeNextResponse({
  bundleId: TENANT_BUNDLE_ID,
  bundle: tenantBundle,
  record: tenantRecord1,
  cursor: TENANT_CURSOR_2,
  claim: tenantClaim,
});

const tenantNextEnd = terminalNext({
  bundleId: TENANT_BUNDLE_ID,
  bundle: tenantBundle,
  previous: fixtureHash("tenant:record:1"),
  ordinal: 2,
  status: "exhausted",
});

const SCOPED_REVISION = fixtureHash("revision:scoped");
const SCOPED_BUNDLE_ID = fixtureHash("bundle:scoped");
const SCOPED_CURSOR_1 = "fixture-cursor-scoped-1";

const scopedOrders: readonly RawRow[] = [
  [text("ORD-1007"), int32(7), text("Acme 東京"), text("Duplicate line")],
  [text("ORD-1007"), int32(7), text("Acme 東京"), text("Duplicate line")],
  [text("ORD-1008"), int32(7), text("<img src=x onerror=alert(1)>"), typedNull("text")],
];

const scopedInput: RawDatabase = { "public.orders": scopedOrders };
const scopedOutput: readonly RawRow[] = scopedOrders.map((row) => [row[0] ?? text(""), row[2] ?? text("")]);
const scopedOutputColumns: readonly RawColumn[] = [scopedColumns[0]!, scopedColumns[2]!];

const scopedRequest = {
  sql: "SELECT order_ref, customer_name\nFROM public.orders\nWHERE tenant_id = $1",
  catalog: scopedCatalog,
  database: scopedInput,
  parameters: { "1": int32(7) },
  mode: "compile",
  bounds: {},
  domain: [],
  include_model: true,
};

const scopedBundle = makeBundle({
  kind: "example",
  revision: SCOPED_REVISION,
  request: scopedRequest,
  ir: readIr("public.orders", scopedColumns, [0, 2], scopedOutputColumns),
  model: demoAlloy("run"),
  outputSchema: scopedOutputColumns,
  assuranceKind: "native_ra_alloy_with_rust_replay",
  source: "frontend demo schema based on the documented request and instance shapes",
});

const scopedInstance = makeInstance({
  kind: "example",
  revision: SCOPED_REVISION,
  semantic: { database: scopedInput, catalog: scopedCatalog, output: scopedOutput },
  outputSchema: scopedOutputColumns,
  assuranceKind: "native_ra_alloy_with_rust_replay",
});

const scopedRecord0 = makeRecord(0, SCOPED_BUNDLE_ID, "instance", scopedInstance);
const scopedOpen = makeOpenResponse({
  bundleId: SCOPED_BUNDLE_ID,
  bundle: scopedBundle,
  records: [scopedRecord0],
  cursor: SCOPED_CURSOR_1,
  claim: "This is one satisfiable stored example. It does not prove the query safe or complete.",
});

const scopedEnd = terminalNext({
  bundleId: SCOPED_BUNDLE_ID,
  bundle: scopedBundle,
  previous: fixtureHash("scoped:record:0"),
  ordinal: 1,
  status: "exhausted",
});

const MUTATION_REVISION = fixtureHash("revision:mutation");
const MUTATION_BUNDLE_ID = fixtureHash("bundle:mutation");
const MUTATION_CURSOR_1 = "fixture-cursor-mutation-1";
const mutationInput: RawDatabase = {
  "public.inventory": [
    [text("SKU-001"), int32(8)],
    [text("SKU-002"), int32(3)],
  ],
};
const mutationPoststate: RawDatabase = {
  "public.inventory": [
    [text("SKU-001"), int32(7)],
    [text("SKU-002"), int32(3)],
  ],
};

const mutationRequest = {
  sql: "UPDATE public.inventory\nSET on_hand = on_hand - 1\nWHERE sku = $1\nRETURNING sku, on_hand, nextval('public.audit_seq') AS event_id",
  catalog: inventoryCatalog,
  database: mutationInput,
  parameters: { "1": text("SKU-001") },
  mode: "compile",
  bounds: {},
  domain: [],
  include_model: true,
  execution: {
    sequences: {
      complete: true,
      exclusive_snapshot: true,
      sequences: {
        "public.audit_seq": {
          oid: 42001,
          schema: "public",
          name: "audit_seq",
          state: { last_value: 1042, is_called: true },
          increment: 1,
          minimum: 1,
          maximum: 1000000,
          cycle: false,
          cache: 1,
          can_use: true,
        },
      },
    },
    max_outcomes: 8,
    max_rows: 64,
    settings: {},
    table_metadata: {
      "public.inventory": {
        complete: true,
        defaults: {},
        primary_key: null,
        unique: [],
        checks: [],
        foreign_keys: [],
        has_triggers: false,
        row_security: false,
        has_inheritance_or_partitioning: false,
      },
    },
  },
};

const mutationIr = {
  Update: {
    target: { table: "public.inventory", schema: inventoryColumns },
    assignments: [{ column: 1, expression: { node: { Binary: { op: "Sub" } }, ty: "int32" } }],
    returning: {
      schema: mutationOutput,
      expressions: [{ node: { Column: 0 } }, { node: { Column: 1 } }],
    },
  },
};

const mutationBundle = makeBundle({
  kind: "example",
  revision: MUTATION_REVISION,
  request: mutationRequest,
  ir: mutationIr,
  model: demoAlloy("run"),
  outputSchema: mutationOutput,
  assuranceKind: "evaluator_backed_table_with_rust_replay",
  source: "current finite outcome and effect schema in crates/sqlolla-core/src/outcome.rs",
});

const mutationInstance = makeInstance({
  kind: "example",
  revision: MUTATION_REVISION,
  semantic: {
    database: mutationInput,
    catalog: inventoryCatalog,
    outcome: {
      status: "succeeded",
      outcome: {
        database: mutationPoststate,
        rows: [[text("SKU-001"), int32(7), int64(1043)]],
        ordering: "bag",
        affected: 1,
        command_tag: "UPDATE 1",
        effects: [{ kind: "sequence_value", sequence: "public.audit_seq", value: 1043 }],
      },
    },
    transition_index: 0,
  },
  outputSchema: mutationOutput,
  assuranceKind: "evaluator_backed_table_with_rust_replay",
});

const mutationRecord = makeRecord(0, MUTATION_BUNDLE_ID, "instance", mutationInstance);
const mutationOpen = makeOpenResponse({
  bundleId: MUTATION_BUNDLE_ID,
  bundle: mutationBundle,
  records: [mutationRecord],
  cursor: MUTATION_CURSOR_1,
  claim: "One evaluator-backed transition within the supplied finite context; not independent SQL semantic verification.",
});
const mutationEnd = terminalNext({
  bundleId: MUTATION_BUNDLE_ID,
  bundle: mutationBundle,
  previous: fixtureHash("mutation:record:0"),
  ordinal: 1,
  status: "exhausted",
});

const SQL_ERROR_REVISION = fixtureHash("revision:sql-error");
const SQL_ERROR_BUNDLE_ID = fixtureHash("bundle:sql-error");
const sqlErrorInput: RawDatabase = {
  "public.inventory": [
    [text("SKU-001"), int32(8)],
    [text("SKU-002"), int32(3)],
  ],
};
const sqlErrorRequest = {
  sql: "UPDATE public.inventory\nSET on_hand = on_hand - 10\nWHERE sku = $1",
  catalog: inventoryCatalog,
  database: sqlErrorInput,
  parameters: { "1": text("SKU-001") },
  mode: "compile",
  bounds: {},
  domain: [],
  include_model: true,
};

const sqlErrorBundle = makeBundle({
  kind: "example",
  revision: SQL_ERROR_REVISION,
  request: sqlErrorRequest,
  ir: {
    Update: {
      target: { table: "public.inventory", schema: inventoryColumns },
      assignments: [{ column: 1 }],
    },
  },
  model: demoAlloy("run"),
  outputSchema: [],
  assuranceKind: "evaluator_backed_table_with_rust_replay",
  source: "documented SQL error outcome shape; diagnostics have no supplied fact references",
});

const sqlErrorInstance = makeInstance({
  kind: "example",
  revision: SQL_ERROR_REVISION,
  semantic: {
    database: sqlErrorInput,
    catalog: inventoryCatalog,
    outcome: {
      status: "sql_error",
      outcome: {
        sqlstate: "23514",
        phase: "statement",
        constraint_name: "inventory_on_hand_check",
        column_name: null,
        exposed_rows: [],
        effects: [],
      },
    },
    transition_index: 2,
  },
  outputSchema: [],
  assuranceKind: "evaluator_backed_table_with_rust_replay",
});

const SQL_ERROR_CURSOR = "fixture-cursor-sql-error-1";
const sqlErrorRecord = makeRecord(0, SQL_ERROR_BUNDLE_ID, "instance", sqlErrorInstance);
const sqlErrorOpen = makeOpenResponse({
  bundleId: SQL_ERROR_BUNDLE_ID,
  bundle: sqlErrorBundle,
  records: [sqlErrorRecord],
  cursor: SQL_ERROR_CURSOR,
  claim: "A modeled SQL rejection is an observed outcome, not an outer engine failure or safety claim.",
});
const sqlErrorEnd = terminalNext({
  bundleId: SQL_ERROR_BUNDLE_ID,
  bundle: sqlErrorBundle,
  previous: fixtureHash("sql-error:record:0"),
  ordinal: 1,
  status: "exhausted",
});

const LARGE_REVISION = fixtureHash("revision:large");
const LARGE_BUNDLE_ID = fixtureHash("bundle:large");
const LARGE_CURSOR = "fixture-cursor-large-1";
const largeColumns: readonly RawColumn[] = [
  { name: "order_ref", ty: "text", nullable: false, qualifier: null, schema: null },
  { name: "tenant_id", ty: "int32", nullable: true, qualifier: null, schema: null },
];
const largeCatalog: RawCatalog = {
  search_path: ["public"],
  tables: { "public.orders": { schema: "public", name: "orders", columns: largeColumns } },
};
const largeRows: readonly RawRow[] = Array.from({ length: 32 }, (_, index) => [
  text(`ORD-${String(2000 + (index % 14)).padStart(4, "0")}`),
  index % 7 === 0 ? typedNull("int32") : int32(index % 3 === 0 ? 42 : 7),
]);
const largeInput: RawDatabase = { "public.orders": largeRows };
const largeRequest = {
  sql: "SELECT order_ref, tenant_id\nFROM public.orders",
  catalog: largeCatalog,
  database: largeInput,
  parameters: {},
  mode: "compile",
  bounds: {},
  domain: [],
  include_model: true,
};
const largeBundle = makeBundle({
  kind: "example",
  revision: LARGE_REVISION,
  request: largeRequest,
  ir: scanIr("public.orders", largeColumns),
  model: demoAlloy("run"),
  outputSchema: largeColumns,
  assuranceKind: "native_ra_alloy_with_rust_replay",
  source: "illustrative stress fixture for graph limits and view-completeness disclosure",
});
const largeInstance = makeInstance({
  kind: "example",
  revision: LARGE_REVISION,
  semantic: { database: largeInput, catalog: largeCatalog, output: largeRows },
  outputSchema: largeColumns,
  assuranceKind: "native_ra_alloy_with_rust_replay",
});
const largeOpen = makeOpenResponse({
  bundleId: LARGE_BUNDLE_ID,
  bundle: largeBundle,
  records: [makeRecord(0, LARGE_BUNDLE_ID, "instance", largeInstance)],
  cursor: LARGE_CURSOR,
  claim: "A larger stored example used to demonstrate a visibly partial graph preview. Original JSON remains available.",
});
const largeEnd = terminalNext({
  bundleId: LARGE_BUNDLE_ID,
  bundle: largeBundle,
  previous: fixtureHash("large:record:0"),
  ordinal: 1,
  status: "exhausted",
});

const EXHAUSTED_BUNDLE_ID = fixtureHash("bundle:terminal-demo");
const LIMIT_BUNDLE_ID = fixtureHash("bundle:limit-demo");
const STALE_BUNDLE_ID = fixtureHash("bundle:stale-demo");
const terminalExhaustedOpen = makeOpenResponse({
  bundleId: EXHAUSTED_BUNDLE_ID,
  bundle: scopedBundle,
  records: [makeRecord(0, EXHAUSTED_BUNDLE_ID, "exhausted")],
  cursor: null,
  claim: terminalClaim("exhausted"),
});
const terminalLimitOpen = makeOpenResponse({
  bundleId: LIMIT_BUNDLE_ID,
  bundle: scopedBundle,
  records: [makeRecord(0, LIMIT_BUNDLE_ID, "limit")],
  cursor: null,
  claim: terminalClaim("limit"),
});

const staleOpen = {
  ...scopedOpen,
  bundle_id: STALE_BUNDLE_ID,
  model_revision_sha256: fixtureHash("revision:stale-demo"),
};
const transportGap: TransportFailure = {
  kind: "transport",
  code: "transport.host_bridge_unavailable",
  message: "No browser-safe host bridge is connected. The local UTF-8 JSON-lines CLI cannot be spawned by this page.",
  retryable: false,
};

const tenantProvenance = demoProvenance(
  "Transformed repository evidence",
  "examples/translation-pairs.json",
  "Tenant/NULL structure is transformed from the repository witness. Expanded duplicates, Unicode and hostile-looking text are illustrative demo data.",
);
const scopedProvenance = demoProvenance(
  "Illustrative demo fixture",
  "documented SQLolla request and instance shapes",
  "A hand-authored satisfying example. It is not solver-produced and carries no safety conclusion.",
);
const mutationProvenance = demoProvenance(
  "Illustrative demo fixture",
  "crates/sqlolla-core/src/outcome.rs and the enumeration README",
  "The mutation and sequence-effect shape is illustrative. No deterministic compile or Alloy enumeration was run for this demo.",
);
const stateProvenance = demoProvenance(
  "Illustrative demo fixture",
  "crates/sqlolla/src/instances and contracts/m2/gameplan.json",
  "Terminal and error payloads are hand-authored to exercise honest status language. They are not exhaustive backend states.",
);
const largeProvenance = demoProvenance(
  "Illustrative demo fixture",
  "frontend graph-limit stress fixture",
  "Generated rows are illustrative only; they demonstrate a visibly partial graph view over unmodified original JSON.",
);

/** The demo library. Primary scenarios first; unusual terminal states are secondary. */
export const DEMO_SCENARIOS: readonly FixtureScenario[] = [
  {
    id: "tenant-boundary",
    shortTitle: "Tenant boundary",
    title: "Orders without a tenant filter",
    description: "See exactly which rows can cross a tenant boundary and why the rule fails.",
    group: "primary",
    statusLabel: "Counterexample",
    bundleId: TENANT_BUNDLE_ID,
    provenance: tenantProvenance,
    summarySeed: {
      title: "What this counterexample shows",
      assumptions: [
        "Illustrative fixture data; no live database query was executed.",
        "This covers only the supplied finite bounds and domain.",
        "Accepted cases are use cases to preserve, not proof of safety.",
      ],
      proposedEdit: "SELECT order_ref, tenant_id, note\nFROM public.orders\nWHERE tenant_id = $1",
      proposedEditLabel: "Add a parameterized tenant predicate",
    },
    openPayload: tenantOpen,
    nextByCursor: {
      [TENANT_CURSOR_1]: tenantNextOne,
      [TENANT_CURSOR_2]: tenantNextEnd,
    },
    builderCatalog: BUILDER_CATALOG,
    transportFailure: null,
  },
  {
    id: "scoped-orders",
    shortTitle: "Scoped orders",
    title: "A parameterized tenant read",
    description: "Inspect one satisfying example with duplicate rows and a typed NULL.",
    group: "primary",
    statusLabel: "Example",
    bundleId: SCOPED_BUNDLE_ID,
    provenance: scopedProvenance,
    summarySeed: {
      title: "What this example shows",
      assumptions: [
        "One satisfying case does not prove correctness for other rows.",
        "Output positions are shown as supplied; bag output is not inherently ordered.",
        "Markup-like text is data and must never be treated as HTML.",
      ],
      proposedEdit: null,
      proposedEditLabel: null,
    },
    openPayload: scopedOpen,
    nextByCursor: {
      [SCOPED_CURSOR_1]: scopedEnd,
    },
    builderCatalog: BUILDER_CATALOG,
    transportFailure: null,
  },
  {
    id: "inventory-update",
    shortTitle: "Stock mutation",
    title: "A bounded stock update with an effect",
    description: "Review before/after rows, RETURNING output and a sequence allocation.",
    group: "primary",
    statusLabel: "Mutation / effect",
    bundleId: MUTATION_BUNDLE_ID,
    provenance: mutationProvenance,
    summarySeed: {
      title: "What this mutation observation shows",
      assumptions: [
        "The effect is retained from the supplied transition, not recomputed in the browser.",
        "This finite evaluator table is not independent proof of PostgreSQL semantics.",
        "No transaction, concurrency or production-database correctness is implied.",
      ],
      proposedEdit: null,
      proposedEditLabel: null,
    },
    openPayload: mutationOpen,
    nextByCursor: { [MUTATION_CURSOR_1]: mutationEnd },
    builderCatalog: BUILDER_CATALOG,
    transportFailure: null,
  },
  {
    id: "state-large",
    shortTitle: "Large graph",
    title: "A large result with a partial graph",
    description: "Inspect a deliberately partial graph preview and recover the original JSON.",
    group: "states",
    statusLabel: "Partial view",
    bundleId: LARGE_BUNDLE_ID,
    provenance: largeProvenance,
    summarySeed: {
      title: "What this large example demonstrates",
      assumptions: [
        "The graph node cap is a presentation limit, not a solver or evidence limit.",
        "The table view and original JSON retain every supplied row.",
        "A partial graph is never marked view-complete.",
      ],
      proposedEdit: null,
      proposedEditLabel: null,
    },
    openPayload: largeOpen,
    nextByCursor: { [LARGE_CURSOR]: largeEnd },
    builderCatalog: BUILDER_CATALOG,
    transportFailure: null,
  },
  {
    id: "state-exhausted",
    shortTitle: "No more cases",
    title: "Exhausted within this exact model",
    description: "A real terminal distinction that is not a statement of global safety.",
    group: "states",
    statusLabel: "Exhausted",
    bundleId: EXHAUSTED_BUNDLE_ID,
    provenance: stateProvenance,
    summarySeed: null,
    openPayload: terminalExhaustedOpen,
    nextByCursor: {},
    builderCatalog: BUILDER_CATALOG,
    transportFailure: null,
  },
  {
    id: "state-limit",
    shortTitle: "Limit reached",
    title: "Configured limit reached",
    description: "Enumeration stopped at its configured capacity; no exhaustion is claimed.",
    group: "states",
    statusLabel: "Limit",
    bundleId: LIMIT_BUNDLE_ID,
    provenance: stateProvenance,
    summarySeed: null,
    openPayload: terminalLimitOpen,
    nextByCursor: {},
    builderCatalog: BUILDER_CATALOG,
    transportFailure: null,
  },
  {
    id: "state-unsupported",
    shortTitle: "Unsupported",
    title: "Unsupported feature",
    description: "A coverage refusal with an explicit diagnostic, not a safety result.",
    group: "states",
    statusLabel: "Unsupported",
    bundleId: fixtureHash("bundle:state-unsupported"),
    provenance: stateProvenance,
    summarySeed: null,
    openPayload: makeErrorResponse("unsupported.order_by", "ORDER BY shape is not admitted by this finite profile"),
    nextByCursor: {},
    builderCatalog: BUILDER_CATALOG,
    transportFailure: null,
  },
  {
    id: "state-missing-context",
    shortTitle: "Missing context",
    title: "Missing schema context",
    description: "A complete database is required for this concrete shape.",
    group: "states",
    statusLabel: "Missing context",
    bundleId: fixtureHash("bundle:state-missing-context"),
    provenance: stateProvenance,
    summarySeed: null,
    openPayload: makeErrorResponse("invalid.database", "concrete modes require every catalog table; use an empty bag explicitly"),
    nextByCursor: {},
    builderCatalog: BUILDER_CATALOG,
    transportFailure: null,
  },
  {
    id: "state-sql-error",
    shortTitle: "SQL error",
    title: "A modeled SQL rejection",
    description: "The statement fails a CHECK; no poststate is published.",
    group: "states",
    statusLabel: "SQL error",
    bundleId: SQL_ERROR_BUNDLE_ID,
    provenance: stateProvenance,
    summarySeed: null,
    openPayload: sqlErrorOpen,
    nextByCursor: { [SQL_ERROR_CURSOR]: sqlErrorEnd },
    builderCatalog: BUILDER_CATALOG,
    transportFailure: null,
  },
  {
    id: "state-replay-mismatch",
    shortTitle: "Replay mismatch",
    title: "Replay mismatch",
    description: "Replay rejected the solver observation; evidence is refused.",
    group: "states",
    statusLabel: "Replay mismatch",
    bundleId: fixtureHash("bundle:state-replay-mismatch"),
    provenance: stateProvenance,
    summarySeed: null,
    openPayload: makeErrorResponse("evidence.replay", "Native Alloy/Rust result mismatch"),
    nextByCursor: {},
    builderCatalog: BUILDER_CATALOG,
    transportFailure: null,
  },
  {
    id: "state-timeout",
    shortTitle: "Resource limit",
    title: "Timeout or resource error",
    description: "Capacity stopped replay; this is inconclusive, not exhaustion.",
    group: "states",
    statusLabel: "Resource",
    bundleId: fixtureHash("bundle:state-timeout"),
    provenance: stateProvenance,
    summarySeed: null,
    openPayload: makeErrorResponse("resource.resume_time", "Complete prefix replay exceeded time capacity"),
    nextByCursor: {},
    builderCatalog: BUILDER_CATALOG,
    transportFailure: null,
  },
  {
    id: "state-stale",
    shortTitle: "Stale revision",
    title: "Stale revision response",
    description: "A response from another model revision is discarded and the cursor is retained.",
    group: "states",
    statusLabel: "Stale revision",
    bundleId: STALE_BUNDLE_ID,
    provenance: stateProvenance,
    summarySeed: null,
    openPayload: staleOpen,
    nextByCursor: {},
    builderCatalog: BUILDER_CATALOG,
    transportFailure: null,
  },
  {
    id: "state-archive",
    shortTitle: "Archive error",
    title: "Archive or store error",
    description: "The local evidence archive could not be trusted or read.",
    group: "states",
    statusLabel: "Archive",
    bundleId: fixtureHash("bundle:state-archive"),
    provenance: stateProvenance,
    summarySeed: null,
    openPayload: makeErrorResponse("store.corrupt", "Archive digest/schema mismatch"),
    nextByCursor: {},
    builderCatalog: BUILDER_CATALOG,
    transportFailure: null,
  },
  {
    id: "state-transport-gap",
    shortTitle: "Host gap",
    title: "No host analysis bridge",
    description: "A transport failure distinct from SQL errors, solver results and archive failures.",
    group: "states",
    statusLabel: "Transport",
    bundleId: fixtureHash("bundle:state-transport-gap"),
    provenance: stateProvenance,
    summarySeed: null,
    openPayload: makeErrorResponse("transport.host_bridge_unavailable", "No browser-safe host bridge is connected"),
    nextByCursor: {},
    builderCatalog: BUILDER_CATALOG,
    transportFailure: transportGap,
  },
];

/** Scenario selected when the workspace first loads. */
export const DEFAULT_SCENARIO_ID = "tenant-boundary";

/** Look up a demo scenario by a stable library identifier. */
export function scenarioById(id: string): FixtureScenario | undefined {
  return DEMO_SCENARIOS.find((scenario) => scenario.id === id);
}

/** Resolve the next raw payload for a fixture cursor, or an invalid-cursor error. */
export function nextPayloadFor(scenario: FixtureScenario, cursor: string): unknown {
  return scenario.nextByCursor[cursor] ?? makeErrorResponse("cursor.invalid", "This cursor does not belong to the selected fixture revision");
}
