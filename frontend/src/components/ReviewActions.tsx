/**
 * Equal-weight labelled review actions, optional rationale, undo and honest
 * explanations of what a use-case label does (and does not) establish.
 */
import type { JSX } from "react";
import type { FeedbackEvent } from "../feedback/journal";

type Decision = "accepted_use_case" | "unintended_behavior";

/** Decision copy shared by gestures, buttons and the journal. */
export interface ReviewActionsProps {
  readonly activeDecision: FeedbackEvent | undefined;
  readonly conflict: boolean;
  readonly rationale: string;
  readonly onRationaleChange: (rationale: string) => void;
  readonly onDecision: (disposition: Decision, rationale: string) => void;
  readonly onUndo: () => void;
  readonly onOpenLedger: () => void;
}

/** Render the current active label as plain language. */
function activeLabel(decision: FeedbackEvent | undefined): string {
  if (decision?.disposition === "accepted_use_case") return "Preserved use case";
  if (decision?.disposition === "unintended_behavior") return "Unintended behavior";
  return "No active label";
}

/** Render the contradictory-label resolution prompt. */
function ConflictAlert({ onOpenLedger }: { readonly onOpenLedger: () => void }): JSX.Element {
  return (
    <div className="inline-alert" role="alert">
      <strong>Contradictory active labels</strong>
      <span>Automatic promotion is stopped. Resolve the conflict in the review ledger.</span>
      <button type="button" className="text-button" onClick={onOpenLedger}>
        Open review ledger
      </button>
    </div>
  );
}

interface RationaleFieldProps {
  readonly rationale: string;
  readonly onChange: (value: string) => void;
}

/** Render the optional local rationale field. */
function RationaleField({ rationale, onChange }: RationaleFieldProps): JSX.Element {
  return (
    <details className="rationale">
      <summary>Optional note for this decision</summary>
      <label htmlFor="review-rationale">Why does this behavior matter?</label>
      <textarea
        id="review-rationale"
        maxLength={500}
        rows={2}
        value={rationale}
        onChange={(event) => onChange(event.target.value)}
        placeholder="Saved in the local journal with this decision."
      />
    </details>
  );
}

interface DecisionButtonProps {
  readonly variant: "reject" | "accept";
  readonly shortcut: string;
  readonly label: string;
  readonly description: string;
  readonly disabled: boolean;
  readonly onSelect: () => void;
}

/** Render one equally capable labelled intent button. */
function DecisionButton({ variant, shortcut, label, description, disabled, onSelect }: DecisionButtonProps): JSX.Element {
  return (
    <button
      type="button"
      className={`decision-button decision-button--${variant}`}
      disabled={disabled}
      onClick={onSelect}
      aria-keyshortcuts={shortcut === "Left / R" ? "R" : "A"}
      aria-describedby={`${variant}-explanation`}
    >
      <span className="decision-shortcut">{shortcut}</span>
      <strong>{label}</strong>
      <span id={`${variant}-explanation`}>{description}</span>
    </button>
  );
}

interface DecisionButtonsProps {
  readonly conflict: boolean;
  readonly rationale: string;
  readonly onDecision: (disposition: Decision, rationale: string) => void;
}

/** Render the two equally capable labelled intent buttons. */
function DecisionButtons({ conflict, rationale, onDecision }: DecisionButtonsProps): JSX.Element {
  return (
    <div className="decision-buttons">
      <DecisionButton
        variant="reject"
        shortcut="Left / R"
        label="Reject side behavior"
        description="Creates an exclusion requirement. Nothing is fixed."
        disabled={conflict}
        onSelect={() => onDecision("unintended_behavior", rationale)}
      />
      <DecisionButton
        variant="accept"
        shortcut="Right / A"
        label="Accept use case"
        description="Preserves this case as a requirement. Not a safety proof."
        disabled={conflict}
        onSelect={() => onDecision("accepted_use_case", rationale)}
      />
    </div>
  );
}

interface ReviewIntentHeadProps {
  readonly decision: FeedbackEvent | undefined;
  readonly conflict: boolean;
  readonly onUndo: () => void;
}

/** Render the active label head and undo control. */
function ReviewIntentHead({ decision, conflict, onUndo }: ReviewIntentHeadProps): JSX.Element {
  return (
    <div className="review-actions__head">
      <div>
        <span className="mono-label">YOUR INTENT</span>
        <h3>{activeLabel(decision)}</h3>
      </div>
      <button type="button" className="text-button" onClick={onUndo} disabled={!decision || conflict}>
        Undo label
      </button>
    </div>
  );
}

/** Render two explicit intent buttons and an append-only undo action. */
export function ReviewActions({
  activeDecision: decision,
  conflict,
  rationale,
  onRationaleChange,
  onDecision,
  onUndo,
  onOpenLedger,
}: ReviewActionsProps): JSX.Element {
  return (
    <div className="review-actions">
      <ReviewIntentHead decision={decision} conflict={conflict} onUndo={onUndo} />
      <p className="review-actions__copy">
        A right swipe or Accept preserves this observation as a use case. A left swipe or Reject creates an exclusion requirement for a future revision. Neither
        proves safety or claims a fix.
      </p>
      {conflict ? <ConflictAlert onOpenLedger={onOpenLedger} /> : null}
      <RationaleField rationale={rationale} onChange={onRationaleChange} />
      <DecisionButtons conflict={conflict} rationale={rationale} onDecision={onDecision} />
      <p className="keyboard-hint">
        Keyboard: A accept · R reject · Ctrl/Cmd+Z undo · N next. Undo appends a withdrawal event; archived evidence is untouched.
      </p>
    </div>
  );
}
