import { parseCatalog } from "./adapter";
import type { CatalogModel, FixtureScenario } from "./types";

/** Editable input example only; every displayed result must come from a new backend analysis. */
export const STARTER_REQUEST = {
  id: "browser-tenant-example",
  sql: "SELECT id, tenant_id FROM orders",
  mode: "analyze",
  catalog: {
    search_path: ["public"],
    tables: {
      orders: {
        schema: "public",
        name: "orders",
        columns: [
          { name: "id", ty: "int32", nullable: false },
          { name: "tenant_id", ty: "int32", nullable: false },
        ],
      },
    },
  },
  parameters: {},
  bounds: { orders: 1 },
  domain: [
    { type: "int32", value: 0 },
    { type: "int32", value: 1 },
  ],
  property: { kind: "only_value", column: 1, value: { type: "int32", value: 1 } },
  include_model: true,
};

/** Use the starter schema for the low-code builder until the user analyzes their own catalog. */
export const STARTER_CATALOG: CatalogModel = parseCatalog(STARTER_REQUEST.catalog, "starter.catalog");

/** Supply presentation metadata before a solver bundle exists, without fabricating evidence. */
export const NATIVE_SCENARIO: FixtureScenario = {
  id: "native",
  title: "Query result",
  shortTitle: "Analysis",
  description: "A bounded case generated from your query and input rules.",
  group: "primary",
  statusLabel: "Analysis",
  bundleId: null,
  provenance: { label: "Local solver evidence", source: "SQLolla host", note: "Verified by replay before review." },
  summarySeed: null,
  openPayload: null,
  nextByCursor: {},
  builderCatalog: STARTER_CATALOG,
  transportFailure: null,
};
