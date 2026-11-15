import { describe, expect, it } from "vitest";
import { parseNextPayload, parseOpenPayload } from "./adapter";
import { DEMO_SCENARIOS, nextPayloadFor, scenarioById } from "./fixtures";
import type { OpenedAnalysis } from "./types";

function parsedScenario(id: string): OpenedAnalysis {
  const scenario = scenarioById(id);
  expect(scenario).toBeDefined();
  const result = parseOpenPayload(scenario!.openPayload);
  expect(result.ok).toBe(true);
  if (!result.ok) throw new Error(result.error.message);
  return result.value;
}

function clone<T>(value: T): T {
  return JSON.parse(JSON.stringify(value)) as T;
}

describe("versioned analysis adapter", () => {
  it("gives every fixture scenario a unique open bundle identity", () => {
    const ids = DEMO_SCENARIOS.map((scenario) => scenario.bundleId);
    expect(new Set(ids).size).toBe(ids.length);
    for (const id of ids) expect(id).toMatch(/^[0-9a-f]{64}$/);
  });

  it("parses the transformed tenant counterexample without collapsing evidence", () => {
    const opened = parsedScenario("tenant-boundary");
    expect(opened.status).toBe("instance");
    expect(opened.evidence).not.toBeNull();
    const evidence = opened.evidence!;
    expect(evidence.instance.kind).toBe("counterexample");
    expect(evidence.duplicateInputRows).toBe(1);
    expect(evidence.typedNullCount).toBeGreaterThan(0);
    expect(evidence.inputTables[0]?.rows).toHaveLength(5);
    expect(evidence.output.rows).toHaveLength(5);
    expect(evidence.invalidFacts).toEqual([]);
    expect(JSON.stringify(evidence.instance.semantic.raw)).toContain("\u6771\u4eac");
    expect(JSON.stringify(evidence.instance.semantic.raw)).toContain("<script>");
  });

  it("keeps before/after, output and an effect visible for the mutation fixture", () => {
    const evidence = parsedScenario("inventory-update").evidence!;
    expect(evidence.statement.outcome).toBe("succeeded");
    expect(evidence.statement.affected).toBe(1);
    expect(evidence.statement.poststateTables[0]?.rows).toHaveLength(2);
    expect(evidence.output.rows).toHaveLength(1);
    expect(evidence.statement.effects[0]?.kind).toBe("sequence_value");
    expect(evidence.statement.effects[0]?.reference).toBe("/semantic/outcome/outcome/effects/0");
  });

  it("keeps SQL errors as instances rather than analysis exhaustion", () => {
    const opened = parsedScenario("state-sql-error");
    expect(opened.current.status).toBe("instance");
    expect(opened.evidence?.statement.outcome).toBe("sql_error");
    expect(opened.evidence?.statement.error?.sqlstate).toBe("23514");
    expect(opened.enumerationExhausted).toBe(false);
  });

  it("parses terminal states without inventing an instance", () => {
    const exhausted = parsedScenario("state-exhausted");
    expect(exhausted.status).toBe("exhausted");
    expect(exhausted.enumerationExhausted).toBe(true);
    expect(exhausted.cursor).toBeNull();
    expect(exhausted.evidence).toBeNull();
    const limit = parsedScenario("state-limit");
    expect(limit.status).toBe("limit");
    expect(limit.enumerationExhausted).toBe(false);
  });

  it("returns structured enumeration errors for unsupported and missing-context states", () => {
    for (const id of ["state-unsupported", "state-missing-context", "state-archive", "state-replay-mismatch", "state-timeout"]) {
      const scenario = scenarioById(id)!;
      const result = parseOpenPayload(scenario.openPayload);
      expect(result.ok).toBe(false);
      if (!result.ok) {
        expect(result.error.kind).toBe("enumeration");
        if (result.error.kind === "enumeration") expect(result.error.kind === "enumeration").toBe(true);
      }
    }
  });

  it("rejects a response whose model revision differs from its immutable bundle", () => {
    const result = parseOpenPayload(scenarioById("state-stale")!.openPayload);
    expect(result.ok).toBe(false);
    if (!result.ok) {
      expect(result.error.kind).toBe("contract");
      if (result.error.kind === "contract") expect(result.error.code).toBe("contract.stale_revision");
    }
  });

  it("rejects unsupported schema versions without rewriting evidence", () => {
    const raw = clone(scenarioById("tenant-boundary")!.openPayload) as Record<string, unknown>;
    raw["schema"] = "sqlolla.enumeration_response.v99";
    const before = JSON.stringify(raw);
    const result = parseOpenPayload(raw);
    expect(result.ok).toBe(false);
    if (!result.ok) {
      expect(result.error.kind).toBe("contract");
      if (result.error.kind === "contract") expect(result.error.code).toBe("contract.unsupported_schema");
    }
    expect(JSON.stringify(raw)).toBe(before);
  });

  it("rejects an error response that tries to claim exhaustion", () => {
    const raw = clone(scenarioById("state-unsupported")!.openPayload) as Record<string, unknown>;
    raw["enumeration_exhausted"] = true;
    const result = parseOpenPayload(raw);
    expect(result.ok).toBe(false);
  });

  it("reads distinct next steps and a real terminal record from the same cursor twice", () => {
    const opened = parsedScenario("tenant-boundary");
    const firstCursor = opened.cursor!;
    const nextRaw = nextPayloadFor(scenarioById("tenant-boundary")!, firstCursor);
    const first = parseNextPayload(nextRaw, opened.bundle, opened.bundleId);
    const second = parseNextPayload(nextRaw, opened.bundle, opened.bundleId);
    expect(first.ok).toBe(true);
    expect(second.ok).toBe(true);
    if (!first.ok || !second.ok) return;
    expect(first.value.current.ordinal).toBe(1);
    expect(first.value.evidence?.instance.instanceId).toBe(second.value.evidence?.instance.instanceId);
    expect(first.value.evidence?.instance.instanceId).not.toBe(opened.evidence?.instance.instanceId);
    const terminal = parseNextPayload(nextPayloadFor(scenarioById("tenant-boundary")!, first.value.cursor!), opened.bundle, opened.bundleId);
    expect(terminal.ok).toBe(true);
    if (terminal.ok) {
      expect(terminal.value.status).toBe("exhausted");
      expect(terminal.value.enumerationExhausted).toBe(true);
      expect(terminal.value.evidence).toBeNull();
    }
  });

  it("rejects a next response from another bundle identity", () => {
    const opened = parsedScenario("tenant-boundary");
    const stale = parseNextPayload(nextPayloadFor(scenarioById("tenant-boundary")!, opened.cursor!), opened.bundle, "0".repeat(64));
    expect(stale.ok).toBe(false);
    if (!stale.ok) {
      expect(stale.error.kind).toBe("contract");
      if (stale.error.kind === "contract") expect(stale.error.code).toBe("contract.stale_revision");
    }
  });

  it("rejects an unsupported executable property", () => {
    const raw = clone(scenarioById("tenant-boundary")!.openPayload) as {
      bundle: { request: { property: unknown } };
    };
    raw.bundle.request.property = { kind: "policy_text", text: "please be safe" };
    const result = parseOpenPayload(raw);
    expect(result.ok).toBe(false);
  });
});
