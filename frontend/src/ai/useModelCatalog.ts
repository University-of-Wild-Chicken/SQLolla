/** Explicit, cancellable model-list checks bound to one provider and key snapshot. */
import { useEffect, useMemo, useRef, useState } from "react";
import { listModels, type ModelCatalog } from "./models";
import type { AiProviderSettings } from "./summary";

/** One check's outcome for the provider and key that produced it. */
interface CatalogState {
  readonly basis: object;
  readonly catalog?: ModelCatalog;
  readonly error?: string;
  readonly busy: boolean;
}

/** List models only on request; changing provider or key discards older or in-flight results. */
export function useModelCatalog(settings: AiProviderSettings) {
  const basis = useMemo(() => ({}), [settings.mode, settings.apiKey]);
  const pending = useRef<AbortController | null>(null);
  const [state, setState] = useState<CatalogState | null>(null);
  useEffect(() => () => pending.current?.abort(), [basis]);
  const check = async (): Promise<void> => {
    pending.current?.abort();
    const controller = new AbortController();
    pending.current = controller;
    setState({ basis, busy: true });
    try {
      const catalog = await listModels(settings, controller.signal);
      if (!controller.signal.aborted) setState({ basis, catalog, busy: false });
    } catch (error) {
      if (!controller.signal.aborted) setState({ basis, error: error instanceof Error ? error.message : "The model check failed.", busy: false });
    }
  };
  return { current: state?.basis === basis ? state : null, check };
}
