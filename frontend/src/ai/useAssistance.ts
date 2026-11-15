/** Explicit, cancellable provider requests never update canonical evidence. */
import { useEffect, useMemo, useRef, useState } from "react";
import type { EvidenceView } from "../analysis/types";
import { requestAssistance } from "./provider";
import type { AiProviderSettings } from "./summary";
import type { Assistance } from "./validation";

interface ResultState {
  readonly bundleId: string;
  readonly basis: EvidenceView;
  readonly settingsIdentity: object;
  readonly result?: Assistance;
  readonly error?: string;
  readonly busy: boolean;
}

/** Cancel stale requests when evidence, disclosure settings or credentials change. */
export function useAssistance(bundleId: string | null, evidence: EvidenceView, settings: AiProviderSettings) {
  const active = useRef<AbortController | null>(null);
  const settingsIdentity = useMemo(() => ({}), [settings]);
  const [state, setState] = useState<ResultState | null>(null);
  useEffect(() => () => active.current?.abort(), [bundleId, evidence, settings]);
  const run = async (prompt: string): Promise<void> => {
    if (!bundleId) return;
    active.current?.abort();
    const controller = new AbortController();
    active.current = controller;
    setState({ bundleId, basis: evidence, settingsIdentity, busy: true });
    try {
      const result = await requestAssistance(bundleId, evidence, settings, prompt, controller.signal);
      if (!controller.signal.aborted) setState({ bundleId, basis: evidence, settingsIdentity, result, busy: false });
    } catch (error) {
      if (!controller.signal.aborted)
        setState({ bundleId, basis: evidence, settingsIdentity, error: error instanceof Error ? error.message : "Assistance failed.", busy: false });
    }
  };
  return { state: state?.bundleId === bundleId && state?.basis === evidence && state.settingsIdentity === settingsIdentity ? state : null, run };
}
