/**
 * BYOT routing: the three equal provider choices, each provider's tested default model and
 * the user's explicit model choice. The host enforces the same admission rule.
 */
import type { AiProviderSettings } from "./summary";

/** Display choices and the tested default model for each supported BYOT provider. */
export const providers = {
  openai_compatible: { id: "openai", label: "OpenAI", model: "gpt-6.1-sol" },
  anthropic_compatible: { id: "anthropic", label: "Anthropic", model: "claude-sonnet-5-5" },
  gemini_compatible: { id: "gemini", label: "Gemini", model: "gemini-4-hydrogen" },
} as const;

export type LiveProviderMode = keyof typeof providers;

/** One request's exact destination: provider identity plus the selected model. */
export interface ProviderTarget {
  readonly id: string;
  readonly label: string;
  readonly model: string;
}

const MODEL_NAME = /^[a-z0-9][a-z0-9.-]{0,63}$/u;

/** Model-name families each provider may receive, mirroring the host's admission rule. */
const FAMILIES: Record<LiveProviderMode, RegExp> = {
  openai_compatible: /^(?:gpt-|chatgpt-|o\d)/u,
  anthropic_compatible: /^claude-/u,
  gemini_compatible: /^gemini-/u,
};

/** Report whether a model name is admissible for the provider before any request is built. */
export function admitsModel(mode: LiveProviderMode, model: string): boolean {
  return MODEL_NAME.test(model) && FAMILIES[mode].test(model);
}

/** Return the model a live request would use: the explicit choice, else the tested default. */
export function selectedModel(settings: AiProviderSettings): string {
  if (settings.mode === "offline_demo") return "";
  return settings.model || providers[settings.mode].model;
}

/** Describe the active live destination, or null when AI is offline or has no key. */
export function liveTarget(settings: AiProviderSettings): ProviderTarget | null {
  if (settings.mode === "offline_demo" || !settings.apiKey.trim()) return null;
  const { id, label } = providers[settings.mode];
  return { id, label, model: selectedModel(settings) };
}

/** Resolve a request's routing, model and tab-only credential from one settings snapshot. */
export function byot(settings: AiProviderSettings) {
  const target = liveTarget(settings);
  if (!target || settings.mode === "offline_demo") throw new Error("Choose an AI provider and add its API key first.");
  if (!admitsModel(settings.mode, target.model)) throw new Error("Choose a model offered by the selected provider.");
  const headers = {
    Authorization: `Bearer ${settings.apiKey.trim()}`,
    "X-SQLolla-Provider": target.id,
    "X-SQLolla-Model": target.model,
  };
  return { target, headers };
}

/** Only the exact selected provider and model may supply the displayed proposal or explanation. */
export function matchesTarget(value: Record<string, unknown>, target: ProviderTarget): boolean {
  return value.provider === target.id && value.model === target.model;
}
