/** First-draft proposals use the local host and keep the current request context intact. */
import { asRecord, stringArray, stringField } from "../analysis/guards";
import { parseCatalog } from "../analysis/adapter";
import { hostJson } from "../analysis/host";
import { editableRequest } from "../workspace/native";
import { AI_MODEL } from "./config";
import type { AiProviderSettings } from "./summary";

export interface SqlDraft {
  readonly sql: string;
  readonly assumptions: readonly string[];
  readonly refusal: string | null;
  readonly modelRequest: Record<string, unknown> | null;
}

export type DraftMode = "query" | "model";

/** Require the fixed provider and the host-owned compile state before presenting a draft. */
function result(raw: unknown, mode: DraftMode): SqlDraft {
  const value = asRecord(raw, "SQL draft");
  const schema = mode === "query" ? "sqlolla.ai.initial_draft.v1" : "sqlolla.ai.model_draft.v1";
  if (value.schema !== schema || value.status !== "ok" || value.model !== AI_MODEL || value.provider !== "openai" || value.authority !== "untrusted_assistance")
    throw new Error("Unexpected initial draft response.");
  const sql = stringField(value, "sql", "SQL draft");
  if (!sql.trim() || sql.length > 16384) throw new Error("Invalid proposed SQL.");
  const assessment = asRecord(value.validation, "draft compilation");
  if (assessment.status !== "compiled" && assessment.status !== "refused") throw new Error("Missing local compiler result.");
  return {
    sql,
    assumptions: stringArray(value.assumptions, "draft assumptions"),
    refusal: assessment.status === "refused" ? stringField(assessment, "message", "draft compilation") : null,
    modelRequest: mode === "model" ? checkedModel(value.model_request, sql) : null,
  };
}

/** Require the exact model reviewed by the compiler before replacing the editable request. */
function checkedModel(raw: unknown, sql: string): Record<string, unknown> {
  const request = asRecord(raw, "proposed model");
  if (request.mode !== "analyze" || request.sql !== sql || !request.property) throw new Error("The proposed model did not match its SQL.");
  parseCatalog(request.catalog, "proposed catalog");
  return request;
}

/** Only the local host sees the analysis request; the provider gets its declared schema projection. */
export async function requestDraft(mode: DraftMode, text: string, instruction: string, settings: AiProviderSettings, signal: AbortSignal): Promise<SqlDraft> {
  const path = mode === "query" ? "/api/ai/draft" : "/api/ai/model-draft";
  const body = mode === "query" ? { instruction, request: { ...editableRequest(text), sql: "" }, share_schema: true } : { instruction, share_prompt: true };
  const value = await hostJson(path, body, { signal, headers: { Authorization: `Bearer ${settings.apiKey.trim()}` } });
  return result(value, mode);
}
