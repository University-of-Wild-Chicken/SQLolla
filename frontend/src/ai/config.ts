import type { AiProviderSettings } from "./summary";

/** Display choices and exact response identities for supported BYOT requests. */
export const providers = {
  openai_compatible: { id: "openai", label: "OpenAI", model: "gpt-6.1-sol" },
  anthropic_compatible: { id: "anthropic", label: "Anthropic", model: "claude-sonnet-5-5" },
  gemini_compatible: { id: "gemini", label: "Gemini", model: "gemini-4-hydrogen" },
} as const;

export type LiveProviderMode = keyof typeof providers;
export type ProviderTarget = (typeof providers)[LiveProviderMode];

/** Resolve a request's routing and tab-only credential from the same settings snapshot. */
export function byot(settings: AiProviderSettings) {
  if (settings.mode === "offline_demo" || !settings.apiKey.trim()) throw new Error("Choose an AI provider and add its API key first.");
  const target = providers[settings.mode];
  const headers = { Authorization: `Bearer ${settings.apiKey.trim()}`, "X-SQLolla-Provider": target.id };
  return { target, headers };
}

/** Only the exact selected provider and model may supply the displayed proposal or explanation. */
export function matchesTarget(value: Record<string, unknown>, target: ProviderTarget): boolean {
  return value.provider === target.id && value.model === target.model;
}
