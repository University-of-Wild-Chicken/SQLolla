import { describe, expect, it } from "vitest";
import { parseOpenPayload } from "../analysis/adapter";
import { scenarioById } from "../analysis/fixtures";
import type { EvidenceView } from "../analysis/types";
import { runMockExplanation } from "./provider";
import { buildContextPreview, buildDemoSummary, defaultAiSettings } from "./summary";

function evidenceFor(id: string): {
  evidence: EvidenceView;
  seed: NonNullable<ReturnType<typeof scenarioById>>["summarySeed"];
} {
  const scenario = scenarioById(id)!;
  const parsed = parseOpenPayload(scenario.openPayload);
  if (!parsed.ok || !parsed.value.evidence) throw new Error("Fixture did not parse");
  return { evidence: parsed.value.evidence, seed: scenario.summarySeed };
}

describe("grounded demo explanations", () => {
  it("grounds every generated claim in an existing supplied fact", () => {
    const { evidence, seed } = evidenceFor("tenant-boundary");
    const summary = buildDemoSummary(evidence, seed!);
    expect(summary.demonstrated).toBe(true);
    expect(summary.claims.length).toBeGreaterThan(2);
    const supplied = new Set(evidence.facts.map((fact) => fact.reference));
    for (const claim of summary.claims) {
      expect(claim.factReferences.length).toBeGreaterThan(0);
      for (const reference of claim.factReferences) expect(supplied.has(reference)).toBe(true);
    }
    expect(summary.assumptions.join(" ")).toContain("Illustrative");
    expect(summary.proposedEdit).toContain("WHERE tenant_id = $1");
  });

  it("redacts supplied values before any future provider send and never reads credentials", () => {
    const { evidence } = evidenceFor("tenant-boundary");
    const settings = {
      ...defaultAiSettings,
      mode: "openai_compatible" as const,
      apiKey: "sk-secret-not-in-preview",
    };
    const preview = buildContextPreview(evidence, settings);
    expect(preview).toContain("Network request: NONE");
    expect(preview).toContain("values redacted");
    expect(preview).toContain("[redacted]");
    expect(preview).not.toContain("Annual renewal");
    expect(preview).not.toContain("sk-secret-not-in-preview");
    const unredacted = buildContextPreview(evidence, { ...settings, redactValues: false });
    expect(unredacted).toContain("Annual renewal");
    expect(unredacted).not.toContain("sk-secret-not-in-preview");
  });

  it("keeps live providers out of the offline fixture adapter", async () => {
    const { evidence, seed } = evidenceFor("tenant-boundary");
    const failed = await runMockExplanation(evidence, seed, {
      ...defaultAiSettings,
      simulateFailure: true,
      mode: "anthropic_compatible",
    });
    expect(failed.ok).toBe(false);
    if (!failed.ok) {
      expect(failed.label).toBe("Anthropic");
      expect(failed.message).toContain("explicit live request");
      expect(failed.message).toContain("No evidence was modified");
    }
    const noSeed = await runMockExplanation(evidence, null, defaultAiSettings);
    expect(noSeed.ok).toBe(false);
  });

  it("labels the explanation as a mock/offline demo without asserting live model availability", async () => {
    const { evidence, seed } = evidenceFor("scoped-orders");
    const outcome = await runMockExplanation(evidence, seed, defaultAiSettings);
    expect(outcome.ok).toBe(true);
    if (outcome.ok) {
      expect(outcome.label).toBe("Offline demo summary");
      expect(outcome.summary.demonstrated).toBe(true);
      expect(JSON.stringify(outcome.summary)).not.toMatch(/GPT-6\.1|DeepSeek-V4\.1/);
    }
  });
});
