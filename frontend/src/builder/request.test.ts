import { describe, expect, it } from "vitest";
import { BUILDER_CATALOG, scenarioById } from "../analysis/fixtures";
import { parseOpenPayload } from "../analysis/adapter";
import { stringifyJson } from "../analysis/json";
import { createDraft } from "./model";
import { draftRequest } from "./request";

describe("deterministic draft requests", () => {
  it("retains numbered Int64 parameters and supplies every explicit input bag", () => {
    const draft = createDraft(BUILDER_CATALOG);
    const request = draftRequest({ ...draft, parameters: [{ ...draft.parameters[0]!, type: "int64", value: "9223372036854775807" }] }, null);
    expect(stringifyJson(request)).toContain('"1":{"type":"int64","value":9223372036854775807}');
    expect(request["database"]).toEqual(Object.fromEntries(Object.keys(BUILDER_CATALOG.tables).map((table) => [table, []])));
    expect(request["sql"]).toContain("$1");
    expect(request["mode"]).toBe("compile");
    expect(request).not.toHaveProperty("property");
    expect(request).not.toHaveProperty("bounds");
  });

  it("rejects integer overflow before transport and retains typed NULL", () => {
    const draft = createDraft(BUILDER_CATALOG);
    expect(() => draftRequest({ ...draft, parameters: [{ ...draft.parameters[0]!, type: "int64", value: "9223372036854775808" }] }, null)).toThrow();
    const request = draftRequest({ ...draft, parameters: [{ ...draft.parameters[0]!, type: "null", nullType: "int64" }] }, null);
    expect(request["parameters"]).toEqual({ "1": { type: "null", value: "int64" } });
  });

  it("uses the displayed before-state instead of silently inventing a live database", () => {
    const result = parseOpenPayload(scenarioById("tenant-boundary")!.openPayload);
    if (!result.ok) throw new Error(result.error.message);
    const evidence = result.value.evidence!;
    const request = draftRequest(createDraft(evidence.bundle.request.catalog), evidence);
    expect(request["database"]).toEqual(
      Object.fromEntries(Object.entries(evidence.instance.semantic.database).map(([key, rows]) => [key, rows.map((row) => row.map((cell) => cell.raw))])),
    );
  });
});
