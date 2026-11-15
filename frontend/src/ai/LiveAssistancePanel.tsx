/** Live explanations are explicit requests and render only untrusted, cited prose. */
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

/** Render generated strings as plain React text, never HTML or executable Markdown. */
function Result({ result, onInspectFact }: { readonly result: Assistance } & Pick<Props, "onInspectFact">): JSX.Element {
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
      <p className="ai-untrusted">Citations confirm supplied facts exist. They do not prove the prose is true.</p>
    </div>
  );
}

/** Explain only after a deliberate button press with the current disclosure setting. */
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
        Question about this case
        <textarea maxLength={4096} value={prompt} onChange={(event) => setPrompt(event.target.value)} />
      </label>
      <button type="button" className="button" disabled={disabled} onClick={() => void run(prompt)}>
        Explain this case
      </button>
      {state?.busy ? <p role="status">Requesting explanation…</p> : null}
      {state?.error ? (
        <div className="inline-alert inline-alert--danger" role="alert">
          {state.error} Deterministic evidence remains available.
        </div>
      ) : null}
      {state?.result ? <Result result={state.result} onInspectFact={props.onInspectFact} /> : null}
    </div>
  );
}
