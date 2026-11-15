import { useId, type JSX } from "react";
import type { CheckedRevision } from "../ai/refinement";
import { shortHash } from "../analysis/format";
import { REVIEW_LABELS } from "../feedback/journal";
import type { useRefinement } from "../workspace/refinement";
import { CopySqlButton } from "./CopySqlButton";

interface PanelProps {
  readonly refinement: ReturnType<typeof useRefinement>;
  readonly onOpenSettings: () => void;
}

/** Keep the retained prompt visible and require an explicit opt-in generation action. */
function GenerateField({ refinement, onOpenSettings }: PanelProps): JSX.Element {
  const inputId = useId();
  return (
    <div className="refinement-field">
      <label htmlFor={inputId}>Original prompt</label>
      <textarea id={inputId} rows={3} maxLength={4096} value={refinement.instruction} onChange={(event) => refinement.setInstruction(event.target.value)} />
      <p>Regeneration uses this prompt and sends canonical SQL, review cases and notes to your AI provider.</p>
      <div className="refinement-actions">
        <button
          type="button"
          className="primary-button"
          disabled={!refinement.basis || !!refinement.generationBlocked || !!refinement.current?.busy}
          onClick={() => void refinement.generate()}
        >
          Regenerate SQL
        </button>
        <button type="button" className="text-button" onClick={onOpenSettings}>
          Provider settings
        </button>
      </div>
      {refinement.generationBlocked ? <p>{refinement.generationBlocked}</p> : null}
    </div>
  );
}

/** Check pasted or edited SQL locally without contacting an AI provider. */
function ManualField({ refinement }: Pick<PanelProps, "refinement">): JSX.Element {
  const overLimit = (refinement.basis?.feedback.length ?? 0) > 256;
  const inputId = useId();
  return (
    <div className="refinement-field">
      <label htmlFor={inputId}>Candidate SQL</label>
      <textarea
        id={inputId}
        className="code-block"
        rows={5}
        maxLength={16384}
        value={refinement.sql}
        onChange={(event) => refinement.setSql(event.target.value)}
      />
      <div className="refinement-actions">
        <button
          type="button"
          className="ghost-button"
          disabled={!refinement.basis || !refinement.sql.trim() || !!refinement.current?.busy || overLimit}
          onClick={() => void refinement.check()}
        >
          Check candidate SQL locally
        </button>
      </div>
      {overLimit ? <p>The local check supports up to 256 active labels.</p> : null}
    </div>
  );
}

const caseStatus = { satisfied: "Satisfied", unsatisfied: "Unsatisfied", refused: "Refused", advisory: "Advisory" } as const;

/** Report deterministic checks per observation and the original finite property. */
function Checks({ result }: { readonly result: CheckedRevision }): JSX.Element {
  if (result.refusal) return <p role="alert">The local checker refused this SQL. {result.refusal}</p>;
  return (
    <>
      <p>
        {result.property === "COUNTEREXAMPLE"
          ? "The original bounded property has a counterexample."
          : "No counterexample found for the original property within the declared bounds."}
      </p>
      <ul className="refinement-checks">
        {result.cases.map(({ label, status, reason }) => (
          <li key={label.event_id}>
            <strong>
              {REVIEW_LABELS[label.disposition]} · {shortHash(label.instance_id)}
            </strong>{" "}
            · {caseStatus[status]}
            {reason ? ` · ${reason}` : ""}
          </li>
        ))}
      </ul>
    </>
  );
}

/** Render optional AI text as plain text beside the separate deterministic findings. */
function Explanation({ explanation }: { readonly explanation: NonNullable<CheckedRevision["explanation"]> }): JSX.Element {
  return (
    <details>
      <summary>AI explanation</summary>
      <p>{explanation.title}</p>
      <ul>
        {explanation.claims.map((claim, index) => (
          <li key={index}>
            {claim.text} <code>{claim.factReferences.join(" · ")}</code>
          </li>
        ))}
      </ul>
    </details>
  );
}

/** Keep checked SQL copyable and stage it only after the user chooses adoption. */
function RevisionResult({ result, adopt }: { readonly result: CheckedRevision; readonly adopt: () => void }): JSX.Element {
  return (
    <div className="refinement-result">
      <h4>{result.explanation ? "Generated candidate SQL" : "Checked candidate SQL"}</h4>
      <pre className="code-block">{result.sql}</pre>
      <CopySqlButton sql={result.sql} />
      <Checks result={result} />
      {result.explanation ? <Explanation explanation={result.explanation} /> : null}
      <button type="button" className="primary-button" disabled={!result.candidateRequest} onClick={adopt}>
        Use checked SQL as new draft
      </button>
    </div>
  );
}

/** Render feedback-gated regeneration, offline checking and current completion status. */
export function RefinementPanel(props: PanelProps): JSX.Element {
  const { refinement } = props;
  return (
    <section className="review-refinement" aria-labelledby="review-refinement-title">
      <h3 id="review-refinement-title">Refine the query</h3>
      <p>Checks cover the exact labeled observations and the original bounded property.</p>
      {refinement.blocked ? (
        <p className="refinement-notice" role="status">
          {refinement.blocked}
        </p>
      ) : null}
      {refinement.basis && !refinement.hasYes ? <p>No Yes cases are protected by this check.</p> : null}
      <div className="refinement-fields">
        <GenerateField {...props} />
        <ManualField refinement={refinement} />
      </div>
      {refinement.current?.busy ? (
        <p role="status">{refinement.current.busy === "generate" ? "Generating and checking SQL…" : "Checking SQL locally…"}</p>
      ) : null}
      {refinement.stale ? <p role="status">The prompt, candidate, evidence or feedback changed. Run a new check to use a revision.</p> : null}
      {refinement.current?.error ? (
        <p className="inline-alert inline-alert--danger" role="alert">
          {refinement.current.error}
        </p>
      ) : null}
      {refinement.current?.result ? <RevisionResult result={refinement.current.result} adopt={refinement.adopt} /> : null}
    </section>
  );
}
