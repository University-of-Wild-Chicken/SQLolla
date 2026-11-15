/** Checked card drafts remain separate from persisted solver instances until explicitly analyzed. */
import { useEffect, useRef, useState, type JSX } from "react";
import { stringifyJson } from "../analysis/json";
import type { DraftState } from "../builder/model";

interface DraftCheckProps {
  readonly draft: DraftState;
  readonly onCheck: (draft: DraftState) => Promise<unknown>;
  readonly onStage: (draft: DraftState) => void;
  readonly onDiscard: () => void;
}

/** Suppress stale compiler responses whenever any card edit changes the request snapshot. */
function useDraftCheck(draft: DraftState, onCheck: DraftCheckProps["onCheck"]) {
  const [result, setResult] = useState<unknown>(null);
  const [error, setError] = useState<string | null>(null);
  const [pending, setPending] = useState(false);
  const generation = useRef(0);
  useEffect(() => {
    ++generation.current;
    setResult(null);
    setError(null);
    setPending(false);
  }, [draft]);
  const check = async (): Promise<void> => {
    if (pending) return;
    const token = ++generation.current;
    setPending(true);
    setError(null);
    try {
      const value = await onCheck(draft);
      if (token === generation.current) setResult(value);
    } catch (failure) {
      if (token === generation.current) setError(failure instanceof Error ? failure.message : "Compiler request failed");
    } finally {
      if (token === generation.current) setPending(false);
    }
  };
  return { result, error, pending, check };
}

/** Display exact compiler output without converting a compilation result into a safety verdict. */
function CompilerResult({ result }: { readonly result: unknown }): JSX.Element {
  const object = typeof result === "object" && result !== null ? (result as Record<string, unknown>) : {};
  return (
    <div className="inline-alert" role="status">
      <strong>{object["status"] === "ok" ? "Compiled by the deterministic backend" : "Compiler rejected the draft"}</strong>
      <span>{typeof object["message"] === "string" ? object["message"] : "Inspect the canonical IR and Alloy model before creating a new review case."}</span>
      <details>
        <summary>Compiler result</summary>
        <pre className="code-block">{stringifyJson(result, 2)}</pre>
      </details>
    </div>
  );
}

/** Invoke the real compiler and expose an explicit transition to a new native analysis. */
export function DraftCheck({ draft, onCheck, onStage, onDiscard }: DraftCheckProps): JSX.Element {
  const state = useDraftCheck(draft, onCheck);
  const passed = typeof state.result === "object" && state.result !== null && (state.result as Record<string, unknown>)["status"] === "ok";
  return (
    <>
      <button type="button" className="primary-button" disabled={state.pending} onClick={() => void state.check()}>
        Check draft with deterministic compiler
      </button>
      {state.error ? (
        <div role="alert" className="inline-alert">
          <strong>Could not check the draft</strong>
          <span>{state.error}</span>
        </div>
      ) : null}
      {state.result ? <CompilerResult result={state.result} /> : null}
      {passed ? (
        <button type="button" className="primary-button" onClick={() => onStage(draft)}>
          Review as a new analysis
        </button>
      ) : null}
      <button type="button" className="text-button text-button--danger" onClick={onDiscard}>
        Discard draft & return to evidence
      </button>
      <p className="rail-footnote">
        Compilation uses the current displayed input, or explicit empty bags when no input is available. It does not modify a database.
      </p>
    </>
  );
}
