import { describe, expect, it } from "vitest";
import { parseOpenPayload, parseStartedOpen, payloadJson, startedIdentity } from "./adapter";
import { canonicalJson, cellText, propertySentence } from "./format";
import { scenarioById } from "./fixtures";
import { integerText, parseJson, stringifyJson } from "./json";
import { parseSqlType, sqlTypeText } from "./sqlTypes";

describe("lossless evidence transport", () => {
  it("preserves signed Int64 extrema, numeric lexemes and original response text", () => {
    const source = '{ "max":9223372036854775807,"min":-9223372036854775808,"decimal":1.2300,"index":3 }';
    const value = parseJson(source) as Record<string, unknown>;
    expect(integerText(value["max"])).toBe("9223372036854775807");
    expect(integerText(value["min"])).toBe("-9223372036854775808");
    expect(integerText(Number("9223372036854775807"))).toBeNull();
    expect(value["index"]).toBe(3);
    expect(payloadJson(value)).toBe(source);
    expect(stringifyJson(value)).toContain('"decimal":1.2300');
    expect(canonicalJson(value)).toContain('"max":9223372036854775807');
    expect(cellText({ type: "int64", value: value["max"], raw: null })).toBe("9223372036854775807");
  });

  it("retains nominal array types and structured typed NULL descriptions", () => {
    const type = { array: { enum: { name: "public.state", labels: ["queued", "done"] } } };
    expect(parseSqlType(type, "type")).toBe(type);
    expect(sqlTypeText(type)).toBe("public.state[]");
    expect(cellText({ type: "null", value: type, raw: null })).toBe("NULL · public.state[]");
    expect(sqlTypeText(parseSqlType({ numeric: { precision: 10, scale: -2 } }, "type"))).toBe("numeric(10,-2)");
    expect(() => parseSqlType({ array: "unrecognized" }, "type")).toThrow();
  });

  it("reads no_sql_error and rejects a reopened revision different from the start", () => {
    const original = scenarioById("tenant-boundary")!.openPayload;
    const raw = parseJson(stringifyJson(original)) as Record<string, any>;
    raw["bundle"].request.property = { kind: "no_sql_error" };
    const result = parseOpenPayload(raw);
    expect(result.ok).toBe(true);
    if (!result.ok) throw new Error(result.error.message);
    expect(propertySentence(result.value.bundle.request.property)).toContain("without a modeled SQL error");
    const identity = startedIdentity(original);
    expect(parseStartedOpen(original, { ...identity, revision: "0".repeat(64) })).toMatchObject({ ok: false, error: { code: "contract.stale_revision" } });
  });

  it("rejects an already-rounded Int64 cell before review", () => {
    const raw = parseJson(stringifyJson(scenarioById("tenant-boundary")!.openPayload)) as Record<string, any>;
    raw["bundle"].request.parameters = { "1": { type: "int64", value: Number("9223372036854775807") } };
    expect(parseOpenPayload(raw)).toMatchObject({ ok: false, error: { code: "contract.integer_precision" } });
  });
});
