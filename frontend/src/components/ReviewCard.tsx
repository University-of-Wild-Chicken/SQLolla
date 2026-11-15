/**
 * One stored case in a large, accessible review card. Swiping is an enhancement;
 * labelled buttons and keyboard shortcuts perform the same journal actions.
 */
import { useId, useState, type JSX } from "react";
import { propertySentence, shortHash } from "../analysis/format";
import { classifyAnalysisError, describeAssurance, describeEnumerationStatus, describeEvidence, type StatusDescriptor } from "../analysis/status";
import type { AdapterFailure, EnumerationError, EnumerationStatus, EvidenceView, FixtureScenario, RequestModel, TransportFailure } from "../analysis/types";
import type { FeedbackEvent, ReviewDisposition } from "../feedback/journal";
import { GraphvizView } from "../graph/GraphvizView";
import { ObservationStrip } from "./ObservationStrip";
import { RelationTables } from "./RelationTables";
import { ReviewActions } from "./ReviewActions";
import { useReviewKeyboard } from "./useReviewKeyboard";
import { useSwipeReview } from "./useSwipeReview";

type Failure = AdapterFailure | EnumerationError | TransportFailure;

/** Props for the central review case. */
export interface ReviewCardProps {
  readonly scenario: FixtureScenario;
  readonly evidence: EvidenceView;
  readonly status: EnumerationStatus;
  readonly cursor: string | null;
  readonly pendingNext: boolean;
  readonly nextFailure: Failure | null;
  readonly staleNotice: string | null;
  readonly activeDecision: FeedbackEvent | undefined;
  readonly conflict: boolean;
  readonly onDecision: (disposition: ReviewDisposition, rationale: string) => void;
  readonly onUndo: () => void;
  readonly onNext: () => void;
  readonly onOpenDetails: () => void;
  readonly onOpenJson: () => void;
  readonly onInspectFact: (reference: string) => void;
  readonly onOpenLedger: () => void;
  readonly reopened?: boolean;
  readonly compact?: boolean;
}

/** Render a concise scope sentence without interpreting the SQL text. */
function scopeText(request: RequestModel): string {
  if (request.mode !== "analyze") return "Complete concrete database supplied; this is a modeled statement, not a live execution.";
  const bounds = Object.entries(request.bounds)
    .map(([table, bound]) => `${table} ≤ ${bound}`)
    .join(", ");
  return `${bounds || "No bounds supplied"} · ${request.domain.length} domain value${request.domain.length === 1 ? "" : "s"}.`;
}

/** Display the distinct assurance label without equating replay with SQL proof. */
function AssuranceNote({ evidence }: { readonly evidence: EvidenceView }): JSX.Element {
  const assurance = describeAssurance(evidence.instance.assuranceKind);
  return (
    <div className={assurance.known ? "assurance-note" : "assurance-note assurance-note--unknown"}>
      <span className="mono-label">ASSURANCE</span>
      <strong>{assurance.label}</strong>
      <p>{assurance.caveat}</p>
    </div>
  );
}

/** An explicit failure banner that never replaces current deterministic evidence. */
function NextFailure({ failure }: { readonly failure: Failure }): JSX.Element {
  const status = classifyAnalysisError(failure);
  return (
    <div className="inline-alert inline-alert--danger" role="alert">
      <strong>{status.label}</strong>
      <span>{status.headline}</span>
      <span>Cursor retained. Retry is safe; no new step was committed.</span>
      <code>{failure.code}</code>
    </div>
  );
}

/** Render a terminal-state banner while keeping any last stored evidence visible. */
function TerminalBanner({ status }: { readonly status: EnumerationStatus }): JSX.Element {
  const descriptor = describeEnumerationStatus(status);
  return (
    <div className="inline-alert" role="status">
      <strong>{descriptor.label}</strong>
      <span>{descriptor.explanation}</span>
    </div>
  );
}

interface CardToplineProps {
  readonly scenario: FixtureScenario;
  readonly onOpenDetails: () => void;
}

/** Render the review topline and details control. */
function CardTopline({ scenario, onOpenDetails }: CardToplineProps): JSX.Element {
  return (
    <div className="card-topline">
      <div className="card-topline__left">
        <span className="step-chip">REVIEW</span>
        <span>{scenario.statusLabel}</span>
      </div>
      <div className="card-topline__right">
        <span className="fixture-pill">{scenario.provenance.label === "Local solver evidence" ? "LOCAL SOLVER EVIDENCE" : "DEMO EVIDENCE"}</span>
        <button type="button" className="text-button" onClick={onOpenDetails}>
          SQL / IR / Alloy
        </button>
      </div>
    </div>
  );
}

interface ReviewHeadlineProps {
  readonly scenario: FixtureScenario;
  readonly descriptor: StatusDescriptor;
  readonly descriptionId: string;
}

/** Render the review masthead and large status stamp. */
function ReviewHeadline({ scenario, descriptor, descriptionId }: ReviewHeadlineProps): JSX.Element {
  return (
    <>
      <header className="review-header">
        <div>
          <p className="eyebrow">{scenario.provenance.label} · stored case</p>
          <h1>{scenario.title}</h1>
          <p className="review-lede">{scenario.description}</p>
        </div>
        <div className={`status-stamp status-stamp--${descriptor.tone}`}>
          <span>{descriptor.label}</span>
          <small>{descriptor.kind.replaceAll("_", " ")}</small>
        </div>
      </header>
      <p className="status-explanation" id={descriptionId}>
        {descriptor.headline} {descriptor.explanation}
      </p>
    </>
  );
}

/** Render the small rule/scope/replay facts row. */
function ReviewFacts({ evidence }: { readonly evidence: EvidenceView }): JSX.Element {
  return (
    <div className="review-facts">
      <div>
        <span className="mono-label">SELECTED RULE</span>
        <p>{propertySentence(evidence.bundle.request.property, evidence.output.columns)}</p>
      </div>
      <div>
        <span className="mono-label">SCOPE</span>
        <p>{scopeText(evidence.bundle.request)}</p>
      </div>
      <div>
        <span className="mono-label">REPLAY</span>
        <p>
          {evidence.instance.validation.replaceAll("_", " ")} · {evidence.instance.completeness.transport ? "transport complete" : "transport incomplete"}
        </p>
      </div>
    </div>
  );
}

interface ViewToolbarProps {
  readonly view: "graph" | "table";
  readonly setView: (view: "graph" | "table") => void;
}

/** Render the graph/table switch and swipe hint. */
function ViewToolbar({ view, setView }: ViewToolbarProps): JSX.Element {
  return (
    <div className="view-toolbar">
      <div className="view-switch" aria-label="Instance view">
        <button type="button" aria-pressed={view === "graph"} onClick={() => setView("graph")}>
          Graph
        </button>
        <button type="button" aria-pressed={view === "table"} onClick={() => setView("table")}>
          Table
        </button>
      </div>
      <div className="swipe-hint">
        <span aria-hidden="true">←</span> swipe or use labelled actions <span aria-hidden="true">→</span>
      </div>
    </div>
  );
}

interface CardEvidenceViewProps {
  readonly view: "graph" | "table";
  readonly evidence: EvidenceView;
  readonly onOpenJson: () => void;
  readonly onInspectFact: (reference: string) => void;
  readonly compact?: boolean;
}

/** Render the selectable evidence visualization. */
function CardEvidenceView({ view, evidence, onOpenJson, onInspectFact, compact }: CardEvidenceViewProps): JSX.Element {
  return (
    <div data-no-swipe>
      {view === "graph" ? (
        <GraphvizView evidence={evidence} onOpenJson={onOpenJson} onInspectFact={onInspectFact} compact={compact} />
      ) : (
        <RelationTables evidence={evidence} onInspectFact={onInspectFact} />
      )}
    </div>
  );
}

/** Render compact revision/replay/fact identities. */
function EvidenceMeta({ evidence }: { readonly evidence: EvidenceView }): JSX.Element {
  return (
    <div className="evidence-strip">
      <span>
        Revision <code>{shortHash(evidence.instance.modelRevisionSha256)}</code>
      </span>
      <span>
        Semantic <code>{shortHash(evidence.instance.semanticSha256)}</code>
      </span>
      <span>
        Facts <code>{evidence.facts.length}</code>
      </span>
      <span>
        Typed NULLs <code>{evidence.typedNullCount}</code>
      </span>
      <span>
        Duplicate rows <code>{evidence.duplicateInputRows}</code>
      </span>
    </div>
  );
}

interface NextCopyProps {
  readonly scenario: FixtureScenario;
  readonly status: EnumerationStatus;
}

/** Render the explicit-next copy. */
function NextCopy({ scenario, status }: NextCopyProps): JSX.Element {
  return (
    <div>
      <span className="mono-label">EXPLICIT NEXT</span>
      <p>{status === "instance" ? "No auto-advance. Request another stored case when you are ready." : describeEnumerationStatus(status).explanation}</p>
      <p className="fixture-provenance">{scenario.provenance.note}</p>
    </div>
  );
}

interface NextFooterProps {
  readonly scenario: FixtureScenario;
  readonly status: EnumerationStatus;
  readonly pendingNext: boolean;
  readonly nextFailure: Failure | null;
  readonly staleNotice: string | null;
  readonly nextLabel: string;
  readonly nextDisabled: boolean;
  readonly onNext: () => void;
}

/** Render explicit, non-automatic advancement. */
function NextFooter({ scenario, status, pendingNext, nextFailure, staleNotice, nextLabel, nextDisabled, onNext }: NextFooterProps): JSX.Element {
  const label = pendingNext ? "Loading next case…" : nextFailure || staleNotice ? `Retry ${nextLabel.toLowerCase()}` : nextLabel;
  return (
    <footer className="next-footer">
      <NextCopy scenario={scenario} status={status} />
      <button type="button" className="next-button" onClick={onNext} disabled={nextDisabled} aria-busy={pendingNext}>
        {label}
      </button>
    </footer>
  );
}

interface CardAlertsProps {
  readonly status: EnumerationStatus;
  readonly staleNotice: string | null;
  readonly nextFailure: Failure | null;
}

/** Render stale/terminal/next-failure alerts above the evidence. */
function CardAlerts({ status, staleNotice, nextFailure }: CardAlertsProps): JSX.Element {
  return (
    <>
      {staleNotice ? (
        <div className="inline-alert inline-alert--danger" role="alert">
          <strong>Stale revision discarded</strong>
          <span>{staleNotice}</span>
        </div>
      ) : null}
      {status !== "instance" ? <TerminalBanner status={status} /> : null}
      {nextFailure ? <NextFailure failure={nextFailure} /> : null}
    </>
  );
}

/** Bind both swipe and keyboard gestures to the same explicit journal actions. */
function useReviewGestures(props: ReviewCardProps, rationale: string) {
  const swipe = useSwipeReview({
    disabled: props.conflict,
    onAccept: () => props.onDecision("accepted_use_case", rationale),
    onReject: () => props.onDecision("unintended_behavior", rationale),
  });
  useReviewKeyboard({
    enabled: true,
    onAccept: () => !props.conflict && props.onDecision("accepted_use_case", rationale),
    onReject: () => !props.conflict && props.onDecision("unintended_behavior", rationale),
    onUnsure: () => !props.conflict && props.onDecision("unsure", rationale),
    onUndo: () => !props.conflict && props.onUndo(),
    onNext: () => !props.pendingNext && !props.reopened && props.onNext(),
  });
  return swipe;
}

/** Keep unsaved rationale edits scoped to the current active label. */
function useRationale(decision: FeedbackEvent | undefined) {
  const eventId = decision?.event_id;
  const [edit, setEdit] = useState<{ eventId: string | undefined; value: string }>();
  const rationale = edit && edit.eventId === eventId ? edit.value : (decision?.rationale ?? "");
  return { rationale, setRationale: (value: string) => setEdit({ eventId, value }) };
}

/** Manage the card view and attach review controls to the exact case. */
function useCardReview(props: ReviewCardProps) {
  const [view, setView] = useState<"graph" | "table">("graph");
  const notes = useRationale(props.activeDecision);
  const statusDescriptionId = useId();
  const swipe = useReviewGestures(props, notes.rationale);
  return {
    view,
    setView,
    ...notes,
    statusDescriptor: describeEvidence(props.evidence),
    statusDescriptionId,
    swipe,
    nextLabel: props.evidence.instance.kind === "counterexample" ? "Next counterexample" : "Next example",
    nextDisabled: props.pendingNext || !props.cursor || props.status !== "instance",
  };
}

type ReviewController = ReturnType<typeof useCardReview>;

/** Render the review head, facts and graph/table switch. */
function CardTop({ props, review }: { readonly props: ReviewCardProps; readonly review: ReviewController }): JSX.Element {
  return (
    <>
      {props.compact ? <ResultHeading props={props} review={review} /> : <CardTopline scenario={props.scenario} onOpenDetails={props.onOpenDetails} />}
      {!props.compact ? <ReviewHeadline scenario={props.scenario} descriptor={review.statusDescriptor} descriptionId={review.statusDescriptionId} /> : null}
      <CardAlerts status={props.status} staleNotice={props.staleNotice} nextFailure={props.nextFailure} />
      <ReviewFacts evidence={props.evidence} />
      <ObservationStrip evidence={props.evidence} />
      <ViewToolbar view={review.view} setView={review.setView} />
    </>
  );
}

/** Lead with the solver result and give direct access to its source triple. */
function ResultHeading({ props, review }: { readonly props: ReviewCardProps; readonly review: ReviewController }): JSX.Element {
  const { statusDescriptor } = review;
  return (
    <header className="result-heading">
      <div>
        <span className={`result-kind result-kind--${statusDescriptor.tone}`}>Solver case</span>
        <h2>{statusDescriptor.label}</h2>
        <p id={review.statusDescriptionId}>{statusDescriptor.headline}</p>
      </div>
      <button type="button" className="ghost-button" onClick={props.onOpenDetails}>
        SQL / IR / Alloy
      </button>
    </header>
  );
}

/** Render the selected evidence view and provenance notes. */
function CardBody({ props, review }: { readonly props: ReviewCardProps; readonly review: ReviewController }): JSX.Element {
  return (
    <>
      <CardEvidenceView
        view={review.view}
        evidence={props.evidence}
        onOpenJson={props.onOpenJson}
        onInspectFact={props.onInspectFact}
        compact={props.compact}
      />
      {props.compact ? (
        <details className="result-proof">
          <summary>Replay and revision details</summary>
          <AssuranceNote evidence={props.evidence} />
          <EvidenceMeta evidence={props.evidence} />
        </details>
      ) : (
        <>
          <AssuranceNote evidence={props.evidence} />
          <EvidenceMeta evidence={props.evidence} />
        </>
      )}
    </>
  );
}

/** Render review intent and explicit next controls. */
function CardDecision({ props, review }: { readonly props: ReviewCardProps; readonly review: ReviewController }): JSX.Element {
  return (
    <>
      <ReviewActions
        activeDecision={props.activeDecision}
        conflict={props.conflict}
        rationale={review.rationale}
        onRationaleChange={review.setRationale}
        onDecision={props.onDecision}
        onUndo={props.onUndo}
        onOpenLedger={props.onOpenLedger}
        reopened={props.reopened}
      />
      {props.reopened ? null : <CardNext props={props} review={review} />}
    </>
  );
}

/** Render next controls only for the live enumeration. */
function CardNext({ props, review }: { readonly props: ReviewCardProps; readonly review: ReviewController }): JSX.Element {
  return (
    <NextFooter
      scenario={props.scenario}
      status={props.status}
      pendingNext={props.pendingNext}
      nextFailure={props.nextFailure}
      staleNotice={props.staleNotice}
      nextLabel={review.nextLabel}
      nextDisabled={review.nextDisabled}
      onNext={props.onNext}
    />
  );
}

/** Render one evidence review card, including graph/table and feedback actions. */
export function ReviewCard(props: ReviewCardProps): JSX.Element {
  const review = useCardReview(props);
  return (
    <article
      className={`review-card ${review.swipe.dragging ? "is-dragging" : ""}`}
      style={review.swipe.dragging ? { transform: `translateX(${review.swipe.dragX}px)` } : undefined}
      aria-describedby={review.statusDescriptionId}
      data-instance-id={props.evidence.instance.instanceId}
      data-testid="review-card"
      {...review.swipe.handlers}
    >
      <CardTop props={props} review={review} />
      <CardBody props={props} review={review} />
      <CardDecision props={props} review={review} />
    </article>
  );
}
