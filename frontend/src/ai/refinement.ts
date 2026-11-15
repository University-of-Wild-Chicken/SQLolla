import { asArray, asRecord, booleanField, stringArray, stringField } from "../analysis/guards";
import { hostJson } from "../analysis/host";
import type { AnalysisTransport, DemoClaim } from "../analysis/types";
import type { ReviewDisposition } from "../feedback/journal";
import { byot, matchesTarget, type ProviderTarget } from "./config";
import type { AiProviderSettings } from "./summary";

/** The canonical saved case identity and current human decision sent to the local host. */
export interface RefinementLabel {
  readonly bundle_id: string;
  readonly instance_id: string;
  readonly model_revision_sha256: string;
  readonly semantic_sha256: string;
  readonly event_id: string;
  readonly disposition: ReviewDisposition;
  readonly rationale?: string;
}

/** Pin the source analysis and its complete active review snapshot. */
export interface RefinementBasis {
  readonly bundle_id: string;
  readonly model_revision_sha256: string;
  readonly feedback: readonly RefinementLabel[];
}

interface CaseCheck {
  readonly label: RefinementLabel;
  readonly status: "satisfied" | "unsatisfied" | "refused" | "advisory";
  readonly reason?: string;
}

/** A local deterministic result and optional supplemental AI explanation. */
export interface CheckedRevision {
  readonly sql: string;
  readonly candidateRequest: Record<string, unknown> | null;
  readonly property: "COUNTEREXAMPLE" | "NO_COUNTEREXAMPLE_WITHIN_BOUNDS" | null;
  readonly cases: readonly CaseCheck[];
  readonly refusal?: string;
  readonly explanation?: { readonly title: string; readonly claims: readonly DemoClaim[] };
}

const schema = "sqlolla.refinement_response.v1";
const hash = /^[0-9a-f]{64}$/u;

/** Check the returned base, digest format and successful deterministic protocol. */
function checkedBasis(value: Record<string, unknown>, basis: RefinementBasis, expectedSchema: string): string {
  if (value.status === "error") throw new Error(typeof value.message === "string" ? value.message : "Refinement was refused.");
  if (value.schema !== expectedSchema) throw new Error("Unsupported refinement response.");
  if (value.status !== "ok" || value.bundle_id !== basis.bundle_id || value.model_revision_sha256 !== basis.model_revision_sha256) {
    throw new Error("Refinement returned a different source analysis.");
  }
  const digest = stringField(value, "feedback_sha256", "refinement");
  if (!hash.test(digest)) throw new Error("Invalid feedback digest.");
  return digest;
}

/** Check each returned case against its sent identity and its decisive or advisory status. */
function caseCheck(raw: unknown, expected: readonly RefinementLabel[]): CaseCheck {
  const value = asRecord(raw, "case check");
  const label = expected.find((item) => item.event_id === value.event_id);
  if (!label || Object.entries(label).some(([key, identity]) => key !== "rationale" && value[key] !== identity)) {
    throw new Error("A case check does not match the submitted feedback.");
  }
  const status = value.status;
  const reason = typeof value.message === "string" ? value.message : typeof value.reason === "string" ? value.reason.replaceAll("_", " ") : undefined;
  if (label.disposition === "unsure" && status === "advisory") return { label, status };
  if (label.disposition !== "unsure" && (status === "satisfied" || status === "unsatisfied" || status === "refused")) return { label, status, reason };
  throw new Error("A case check uses an unexpected result.");
}

/** Require every reviewed case and the retained bounded property before staging the candidate. */
function checkedResult(raw: unknown, basis: RefinementBasis, sql: string, digest: string): CheckedRevision {
  const value = asRecord(raw, "checked revision");
  if (value.schema === schema && value.status === "error" && value.ready === false) {
    const refusal = typeof value.message === "string" ? value.message : "The candidate could not be checked.";
    return { sql, candidateRequest: null, property: null, cases: [], refusal };
  }
  if (checkedBasis(value, basis, schema) !== digest) throw new Error("The feedback digest changed during refinement.");
  const cases = asArray(value.cases, "case checks").map((item) => caseCheck(item, basis.feedback));
  if (cases.length !== basis.feedback.length || new Set(cases.map((item) => item.label.event_id)).size !== cases.length) {
    throw new Error("Refinement did not return every submitted case.");
  }
  const property = asRecord(value.property_check, "property check").status;
  if (property !== "NO_COUNTEREXAMPLE_WITHIN_BOUNDS" && property !== "COUNTEREXAMPLE") throw new Error("Unknown bounded property result.");
  const ready = property === "NO_COUNTEREXAMPLE_WITHIN_BOUNDS" && cases.every(({ status }) => status === "satisfied" || status === "advisory");
  if (booleanField(value, "ready", "checked revision") !== ready) throw new Error("Refinement readiness disagrees with the checks.");
  const candidateRequest = ready ? asRecord(value.candidate_request, "candidate request") : null;
  if (candidateRequest && candidateRequest.sql !== sql) throw new Error("The checked request changed the candidate SQL.");
  return { sql, candidateRequest, property, cases };
}

/** Obtain the canonical digest locally and submit the exact candidate for deterministic checking. */
export async function checkRevision(transport: AnalysisTransport, basis: RefinementBasis, sql: string, signal: AbortSignal): Promise<CheckedRevision> {
  const command = transport.command;
  if (!command) throw new Error("The local refinement checker is unavailable.");
  const context = await command({ action: "refinement.context", ...basis });
  if (!context.ok) throw new Error(context.error.message);
  const digest = checkedBasis(asRecord(context.payload, "refinement context"), basis, schema);
  signal.throwIfAborted();
  const result = await command({ action: "refinement.check", ...basis, sql, feedback_sha256: digest });
  if (!result.ok) throw new Error(result.error.message);
  return checkedResult(result.payload, basis, sql, digest);
}

/** Model citations may name only pointers the local host issued for the canonical packet. */
function groundedClaims(raw: unknown, allowed: ReadonlySet<string>): readonly DemoClaim[] {
  const claims = asArray(raw, "refinement claims");
  if (claims.length > 8) throw new Error("Too many refinement claims.");
  return claims.map((value) => {
    const claim = asRecord(value, "refinement claim");
    const text = stringField(claim, "text", "refinement claim");
    const refs = stringArray(claim.fact_references, "refinement fact references");
    if (!refs.length || refs.some((ref) => !allowed.has(ref))) throw new Error("Refinement cites an unavailable fact.");
    return { text, factReferences: refs };
  });
}

/** Accept supplemental model text only from the fixed provider and canonical context. */
function generatedRevision(raw: unknown, basis: RefinementBasis, target: ProviderTarget): CheckedRevision {
  const value = asRecord(raw, "generated revision");
  const digest = checkedBasis(value, basis, "sqlolla.ai.refinement.v1");
  if (!matchesTarget(value, target) || value.authority !== "untrusted_assistance" || value.redact_values !== false) {
    throw new Error("The refinement provider or disclosure settings changed.");
  }
  const document = asRecord(value.document, "refinement document");
  const context = stringField(value, "context_sha256", "generated revision");
  const decisiveAnchor = basis.feedback.some((label) => label.disposition !== "unsure" && label.instance_id === document.instance_id);
  if (
    !hash.test(context) ||
    !decisiveAnchor ||
    document.instance_id !== value.instance_id ||
    document.model_revision_sha256 !== basis.model_revision_sha256 ||
    document.context_sha256 !== context
  ) {
    throw new Error("The generated revision does not match the canonical context.");
  }
  const sql = stringField(document, "sql", "refinement document");
  if (!sql.trim() || sql.length > 16384) throw new Error("Invalid generated SQL.");
  const checked = checkedResult(value.validation, basis, sql, digest);
  const title = stringField(document, "title", "refinement document");
  const allowed = new Set(stringArray(value.references, "refinement context references"));
  return { ...checked, explanation: { title, claims: groundedClaims(document.claims, allowed) } };
}

/** Regenerate only from a deliberate action with the exact retained instruction and feedback. */
export async function generateRevision(
  basis: RefinementBasis,
  instruction: string,
  settings: AiProviderSettings,
  signal: AbortSignal,
): Promise<CheckedRevision> {
  if (settings.redactValues) throw new Error("Allow unredacted context in provider settings to regenerate.");
  const { target, headers } = byot(settings);
  const payload = await hostJson(
    "/api/ai/byot/refine",
    { ...basis, instruction, redact_values: false },
    {
      signal,
      headers,
    },
  );
  return generatedRevision(payload, basis, target);
}
