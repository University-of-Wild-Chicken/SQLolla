import { parseCatalog } from "./adapter";
import type { CatalogModel, FixtureScenario } from "./types";

/** A blank production workspace holds only the explicit request fields. */
export const EMPTY_REQUEST = {
  id: "browser-query",
  sql: "",
  mode: "analyze",
  catalog: {
    search_path: ["public"],
    tables: {},
  },
  parameters: {},
  bounds: {},
  domain: [],
  property: { kind: "no_sql_error" },
  include_model: true,
};

/** Start with a valid empty catalog until the user declares or imports their own. */
export const EMPTY_CATALOG: CatalogModel = parseCatalog(EMPTY_REQUEST.catalog, "request.catalog");

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
  builderCatalog: EMPTY_CATALOG,
  transportFailure: null,
};
