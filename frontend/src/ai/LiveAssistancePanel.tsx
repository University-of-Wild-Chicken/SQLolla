/** Live explanations are explicit requests and render only untrusted, cited prose. */
import { useId, useState, type JSX } from "react";
import type { EvidenceView, FactModel } from "../analysis/types";
import type { AiProviderSettings } from "./summary";
import type { Assistance } from "./validation";
import { useAssistance } from "./useAssistance";

interface Props {
  readonly bundleId: string | null;
  readonly evidence: EvidenceView;
  readonly settings: AiProviderSettings;
  readonly onInspectFact: (reference: string) => void;
}

const FACT_LABELS = new Map([
  ["input_row", "Input row"],
  ["output_row", "Returned row"],
  ["poststate_row", "Resulting row"],
  ["effect", "Recorded effect"],
]);

/** Use a readable label while keeping the canonical pointer available on the citation. */
function citationLabel(fact: FactModel | undefined, reference: string): string {
  if (!fact) return reference;
  const category = FACT_LABELS.get(fact.kind);
  if (!category) return reference;
  return fact.occurrence === null ? category : `${category} ${fact.occurrence + 1}`;
}

/** Every prose claim links only to a validated fact from the currently displayed instance. */
function Claims({ result, evidence, onInspectFact }: { readonly result: Assistance } & Pick<Props, "evidence" | "onInspectFact">): JSX.Element {
  const facts = new Map(evidence.facts.map((fact) => [fact.reference, fact]));
  return (
    <div className="ai-claims">
      {result.claims.map((claim, index) => (
        <div className="ai-claim" key={index}>
          <p>{claim.text}</p>
          <div className="citations">
            {claim.factReferences.map((ref) => (
              <button type="button" className="citation" title={ref} key={ref} onClick={() => onInspectFact(ref)}>
                {citationLabel(facts.get(ref), ref)}
              </button>
            ))}
          </div>
        </div>
      ))}
    </div>
  );
}

/** Render generated strings as plain React text, never HTML or executable Markdown. */
function Result({ result, evidence, onInspectFact }: { readonly result: Assistance } & Pick<Props, "evidence" | "onInspectFact">): JSX.Element {
  return (
    <div className="ai-content">
      <p className="ai-result-label">AI-generated</p>
      <h3>{result.title}</h3>
      <Claims result={result} evidence={evidence} onInspectFact={onInspectFact} />
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

interface QuestionProps {
  readonly prompt: string;
  readonly edit: (text: string) => void;
  readonly explain: () => void;
  readonly disabled: boolean;
  readonly redacted: boolean;
}

/** Keep the question, explicit send action, and current sharing details together. */
function Question({ prompt, edit, explain, disabled, redacted }: QuestionProps): JSX.Element {
  const sharingId = useId();
  return (
    <div className="ai-question">
      <label className="field-label">
        Question about this case
        <textarea
          aria-describedby={sharingId}
          rows={2}
          placeholder="What should I check in this query?"
          maxLength={4096}
          value={prompt}
          onChange={(event) => edit(event.target.value)}
        />
      </label>
      <button type="button" className="primary-button" aria-describedby={sharingId} disabled={disabled} onClick={explain}>
        Explain this case
      </button>
      <p id={sharingId} className="ai-sharing-note">
        {redacted
          ? "Shares your question as written and the rule outcome when available. Case SQL, IR, Alloy and values stay local."
          : "Sends your question and the case’s SQL, IR, Alloy and values to your AI provider."}
      </p>
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
      {!props.bundleId ? <p role="status">Open canonical host evidence before requesting live assistance.</p> : null}
      <Question prompt={prompt} edit={setPrompt} explain={() => void run(prompt)} disabled={disabled} redacted={props.settings.redactValues} />
      {state?.busy ? <p role="status">Requesting explanation…</p> : null}
      {state?.error ? (
        <div className="inline-alert inline-alert--danger" role="alert">
          {state.error} Deterministic evidence remains available.
        </div>
      ) : null}
      {state?.result ? <Result result={state.result} evidence={props.evidence} onInspectFact={props.onInspectFact} /> : null}
    </div>
  );
}
