/**
 * Separate offline fixtures from explicit fixed-model host requests. Live requests
 * use request-lifetime authorization headers and never fall back to fixture prose.
 */
import { buildDemoSummary, providerLabel, type AiProviderSettings } from "./summary";
import type { DemoSummary, DemoSummarySeed, EvidenceView } from "../analysis/types";
import { hostJson } from "../analysis/host";
import { byot } from "./config";
import { validateAssistance, type Assistance } from "./validation";

/** Result of the mock explanation adapter. */
export type ExplanationOutcome =
  { readonly ok: true; readonly summary: DemoSummary; readonly label: string } | { readonly ok: false; readonly label: string; readonly message: string };

/** Return a demo explanation without sending evidence or credentials anywhere. */
export async function runMockExplanation(
  evidence: EvidenceView | null,
  seed: DemoSummarySeed | null,
  settings: AiProviderSettings,
): Promise<ExplanationOutcome> {
  const label = providerLabel(settings.mode);
  if (settings.mode !== "offline_demo") {
    return { ok: false, label, message: "This provider requires an explicit live request. No evidence was modified." };
  }
  if (!evidence || !seed) {
    return {
      ok: false,
      label,
      message: "No fixture explanation is supplied for this case. Deterministic evidence remains fully usable.",
    };
  }
  if (settings.simulateFailure) {
    return {
      ok: false,
      label,
      message: "The mock provider is unavailable by request. No evidence was modified.",
    };
  }
  return { ok: true, summary: buildDemoSummary(evidence, seed), label };
}

/** Send only canonical IDs and explicit user intent; the host reloads all model-visible facts. */
export async function requestAssistance(
  bundleId: string,
  evidence: EvidenceView,
  settings: AiProviderSettings,
  prompt: string,
  signal: AbortSignal,
): Promise<Assistance> {
  const { target, headers } = byot(settings);
  const body = {
    bundle_id: bundleId,
    instance_id: evidence.instance.instanceId,
    model_revision_sha256: evidence.instance.modelRevisionSha256,
    redact_values: settings.redactValues,
    question: prompt,
  };
  const response = await hostJson("/api/ai/byot/explain", body, { signal, headers });
  return validateAssistance(response, bundleId, evidence, settings.redactValues, target);
}
