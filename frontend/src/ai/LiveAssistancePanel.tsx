/** Live assistance is opt-in untrusted prose; proposed SQL becomes a draft only on explicit acceptance. */
import { useState, type JSX } from "react";
import type { EvidenceView } from "../analysis/types";
import type { AiProviderSettings } from "./summary";
import type { Assistance } from "./validation";
import { useAssistance } from "./useAssistance";
import { AI_MODEL } from "./config";

interface Props {
  readonly bundleId: string | null;
  readonly evidence: EvidenceView;
  readonly settings: AiProviderSettings;
  readonly onInspectFact: (reference: string) => void;
  readonly onApplyProposal?: (sql: string) => void;
}

/** Report the deterministic candidate result without claiming equivalence or universal safety. */
function validationLabel(validation: Record<string, unknown> | null): string {
  if (!validation || validation.status !== "ok") return `Candidate analysis incomplete${typeof validation?.code === "string" ? `: ${validation.code}` : "."}`;
  if (validation.analysis === "COUNTEREXAMPLE") return "The retained bounded property still has a counterexample.";
  if (validation.analysis === "NO_COUNTEREXAMPLE_WITHIN_BOUNDS")
    return "No counterexample found within the unchanged finite bounds. This is not universal safety.";
  return "Candidate bounded analysis completed. Review its deterministic result before using this draft.";
}

/** Both provider actions require explicit interaction; proposals additionally require disclosure and intent. */
function RequestButtons({
  disabled,
  redacted,
  prompt,
  run,
}: {
  readonly disabled: boolean;
  readonly redacted: boolean;
  readonly prompt: string;
  readonly run: (kind: "explain" | "propose", prompt: string) => Promise<void>;
}): JSX.Element {
  return (
    <div className="button-row">
      <button type="button" className="button" disabled={disabled} onClick={() => void run("explain", prompt)}>
        Explain this case
      </button>
      <button type="button" className="button" disabled={disabled || redacted || !prompt.trim()} onClick={() => void run("propose", prompt)}>
        Propose SQL
      </button>
    </div>
  );
}

/** Every prose claim links only to a validated fact from the currently displayed instance. */
function Claims({ result, inspect }: { readonly result: Assistance; readonly inspect: Props["onInspectFact"] }): JSX.Element {
  return (
    <div className="ai-claims">
      {result.claims.map((claim, index) => (
        <div className="ai-claim" key={index}>
          <p>{claim.text}</p>
          <div className="citations">
            {claim.factReferences.map((ref) => (
              <button type="button" className="citation" key={ref} onClick={() => inspect(ref)}>
                {ref}
              </button>
            ))}
          </div>
        </div>
      ))}
    </div>
  );
}

/** A provider proposal never overwrites the saved request, its property, domain or evidence. */
function Proposal({ result, apply }: { readonly result: Assistance; readonly apply?: Props["onApplyProposal"] }): JSX.Element | null {
  if (!result.sql) return null;
  const sql = result.sql;
  return (
    <div className="proposal-card">
      <span className="mono-label">UNTRUSTED SQL PROPOSAL</span>
      <pre className="context-preview">{sql}</pre>
      <p role="status">{validationLabel(result.validation)}</p>
      <p>Business intent and equivalence are not proved. Acceptance opens a draft; it does not execute SQL against an application database.</p>
      <button type="button" className="button button--primary" disabled={!apply || result.validation?.status !== "ok"} onClick={() => apply?.(sql)}>
        Use as SQL draft
      </button>
    </div>
  );
}

/** Render generated strings as plain React text, never HTML or executable Markdown. */
function Result({ result, onInspectFact, onApplyProposal }: { readonly result: Assistance } & Pick<Props, "onInspectFact" | "onApplyProposal">): JSX.Element {
  return (
    <div className="ai-content">
      <div className="demo-content-tag">
        <span>UNTRUSTED AI TEXT</span>
        <span>{AI_MODEL}</span>
      </div>
      <h3>{result.title}</h3>
      <Claims result={result} inspect={onInspectFact} />
      <div className="assumption-block">
        <span className="mono-label">ASSUMPTIONS & SCOPE</span>
        <ul>
          {result.assumptions.map((item, index) => (
            <li key={index}>{item}</li>
          ))}
        </ul>
      </div>
      <Proposal result={result} apply={onApplyProposal} />
      <p className="ai-untrusted">Citations confirm supplied facts exist. They do not prove the prose is true.</p>
    </div>
  );
}

/** Explain or propose only after a deliberate button press with the current disclosure setting. */
export function LiveAssistancePanel(props: Props): JSX.Element {
  const [prompt, setPrompt] = useState("");
  const { state, run } = useAssistance(props.bundleId, props.evidence, props.settings);
  const disabled = !props.bundleId || !props.settings.apiKey.trim() || !!state?.busy;
  return (
    <div className="ai-live-controls">
      <p>
        {AI_MODEL} · {props.settings.redactValues ? "SQL, IR, Alloy and values omitted." : "Canonical SQL, IR, Alloy and values will be sent to OpenAI."}
      </p>
      {!props.bundleId ? <p role="status">Open canonical host evidence before requesting live assistance.</p> : null}
      <label className="field-label">
        Question or proposed change
        <textarea aria-label="Question or proposed change" maxLength={4096} value={prompt} onChange={(event) => setPrompt(event.target.value)} />
      </label>
      <RequestButtons disabled={disabled} redacted={props.settings.redactValues} prompt={prompt} run={run} />
      {props.settings.redactValues ? <p>SQL proposals require turning off redaction in provider settings.</p> : null}
      {state?.busy ? <p role="status">Requesting assistance; proposed SQL also undergoes bounded analysis…</p> : null}
      {state?.error ? (
        <div className="inline-alert inline-alert--danger" role="alert">
          {state.error} Deterministic evidence remains available.
        </div>
      ) : null}
      {state?.result ? <Result result={state.result} onInspectFact={props.onInspectFact} onApplyProposal={props.onApplyProposal} /> : null}
    </div>
  );
}
