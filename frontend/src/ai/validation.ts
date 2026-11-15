/** Validate the host's AI envelope independently before rendering any generated text or draft. */
import type { DemoClaim, EvidenceView } from "../analysis/types";
import { AI_MODEL } from "./config";

export interface Assistance {
  readonly title: string;
  readonly claims: readonly DemoClaim[];
  readonly assumptions: readonly string[];
}

/** Reject arrays and primitives where a strict JSON object is expected. */
function object(value: unknown): Record<string, unknown> {
  if (!value || typeof value !== "object" || Array.isArray(value)) throw new Error("Invalid assistance response.");
  return value as Record<string, unknown>;
}

/** Plain text is bounded independently of the model's structured-output schema. */
function text(value: unknown, limit: number): string {
  if (typeof value !== "string" || !value.trim() || value.length > limit || /[\u0000-\u0008\u000b\u000c\u000e-\u001f]/u.test(value)) {
    throw new Error("Invalid assistance text.");
  }
  return value;
}

/** A bounded string list cannot conceal an arbitrary provider object in the UI. */
function strings(value: unknown, limit: number): readonly string[] {
  if (!Array.isArray(value) || value.length > 8) throw new Error("Invalid assistance list.");
  return value.map((item) => text(item, limit));
}

/** Citations must resolve to the exact current instance, preserving separate row occurrences. */
function claims(value: unknown, evidence: EvidenceView): readonly DemoClaim[] {
  if (!Array.isArray(value) || value.length > 8) throw new Error("Invalid assistance claims.");
  const known = new Set(evidence.facts.map((fact) => fact.reference));
  return value.map((item) => {
    const claim = object(item);
    const refs = strings(claim.fact_references, 4096);
    if (!refs.length || refs.some((ref) => !known.has(ref))) throw new Error("Assistance cites an unavailable fact.");
    return { text: text(claim.text, 1200), factReferences: refs };
  });
}

/** Validate identities before handling the model-controlled document. */
function identity(value: Record<string, unknown>, bundleId: string, evidence: EvidenceView, redacted: boolean): void {
  if (
    value.schema !== "sqlolla.ai.explanation.v1" ||
    value.status !== "ok" ||
    value.model !== AI_MODEL ||
    value.provider !== "openai" ||
    value.authority !== "untrusted_assistance" ||
    value.bundle_id !== bundleId ||
    value.redact_values !== redacted ||
    value.instance_id !== evidence.instance.instanceId ||
    value.model_revision_sha256 !== evidence.instance.modelRevisionSha256 ||
    typeof value.context_sha256 !== "string" ||
    !/^[a-f0-9]{64}$/u.test(value.context_sha256)
  ) {
    throw new Error("Assistance does not match the current evidence.");
  }
}

/** No successful provider reply can replace the authoritative status or immutable evidence object. */
export function validateAssistance(raw: unknown, bundleId: string, evidence: EvidenceView, redacted: boolean): Assistance {
  const value = object(raw);
  identity(value, bundleId, evidence, redacted);
  const doc = object(value.document);
  if (doc.instance_id !== value.instance_id || doc.model_revision_sha256 !== value.model_revision_sha256 || doc.context_sha256 !== value.context_sha256) {
    throw new Error("Assistance document identity changed.");
  }
  if (doc.sql !== null) throw new Error("Explanation unexpectedly contains a SQL proposal.");
  return {
    title: text(doc.title, 160),
    claims: claims(doc.claims, evidence),
    assumptions: strings(doc.assumptions, 512),
  };
}
