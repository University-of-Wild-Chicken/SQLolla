/**
 * Live model availability for a BYOT key. Listing sends only the key (through the host) to the
 * selected provider; no SQL, evidence, prompt or review note leaves the browser.
 */
import { asRecord, stringArray, stringField } from "../analysis/guards";
import { hostJson } from "../analysis/host";
import { admitsModel, byot, type LiveProviderMode } from "./config";
import type { AiProviderSettings } from "./summary";

/** Provider-reported models for one key, plus the default SQLolla is tested with. */
export interface ModelCatalog {
  readonly recommended: string;
  readonly models: readonly string[];
}

/** Reject listings for another provider or names the host would refuse to route. */
function parseCatalog(raw: unknown, providerId: string, mode: LiveProviderMode): ModelCatalog {
  const value = asRecord(raw, "model list");
  if (value.schema !== "sqlolla.ai.models.v1" || value.status !== "ok" || value.provider !== providerId) {
    throw new Error("Unexpected model list response.");
  }
  const models = stringArray(value.models, "model list");
  const recommended = stringField(value, "recommended", "model list");
  if (models.length > 200 || ![recommended, ...models].every((model) => admitsModel(mode, model))) {
    throw new Error("The model list contained an unsupported model name.");
  }
  return { recommended, models };
}

/** Ask the selected provider which text models this key can use. */
export async function listModels(settings: AiProviderSettings, signal: AbortSignal): Promise<ModelCatalog> {
  if (settings.mode === "offline_demo") throw new Error("Choose an AI provider first.");
  const { target, headers } = byot({ ...settings, model: "" });
  const raw = await hostJson("/api/ai/byot/models", {}, { signal, headers });
  return parseCatalog(raw, target.id, settings.mode);
}
