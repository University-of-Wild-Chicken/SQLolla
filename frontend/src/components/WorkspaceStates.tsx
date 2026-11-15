import { stringifyJson } from "../analysis/json";
/**
 * Honest center-column states for loading, terminal enumeration, failures and
 * locally edited drafts awaiting analysis.
 */
import type { JSX } from "react";
import { classifyAnalysisError, describeEnumerationStatus, type StatusDescriptor } from "../analysis/status";
import type { AdapterFailure, EnumerationError, EnumerationStatus, FixtureScenario, TransportFailure } from "../analysis/types";
import type { DraftState, PlanStep } from "../builder/model";
import { draftErrors, readablePlan, renderDraftSql } from "../builder/render";

/** Accessible skeleton while a stored revision is opening. */
export function OpeningState({ scenario }: { readonly scenario: FixtureScenario }): JSX.Element {
  return (
    <section className="center-state" aria-live="polite" aria-busy="true">
      <span className="mono-label">OPENING STORED REVISION</span>
      <h1>{scenario.title}</h1>
      <p>Loading immutable fixture evidence. No solver is being drained and no SQL is executed.</p>
      <div className="skeleton-stack" aria-hidden="true">
        <span />
        <span />
        <span />
      </div>
    </section>
  );
}

/** Render an optional raw payload for local inspection. */
function RawDetails({ raw }: { readonly raw?: unknown }): JSX.Element | null {
  if (!raw) return null;
  return (
    <details className="raw-details">
      <summary>Original response JSON</summary>
      <pre className="code-block code-block--tall">{stringifyJson(raw, 2)}</pre>
    </details>
  );
}

interface FailureIntroProps {
  readonly scenario: FixtureScenario;
  readonly failure: AdapterFailure | EnumerationError | TransportFailure;
  readonly descriptor: StatusDescriptor;
  readonly retryable: boolean;
  readonly onRetry: () => void;
}

/** Render the failure heading, scope caveat and retry action. */
function FailureIntro({ scenario, failure, descriptor, retryable, onRetry }: FailureIntroProps): JSX.Element {
  return (
    <>
      <div className="state-heading">
        <span className="mono-label">STOPPED · {failure.kind.toUpperCase()}</span>
        <span className="status-stamp status-stamp--caution">{descriptor.label}</span>
      </div>
      <h1>{scenario.title}</h1>
      <p className="state-headline">{descriptor.headline}</p>
      <p>{descriptor.explanation}</p>
      <p className="state-caveat">This state does not mean the query is safe and does not mean no counterexamples exist.</p>
      <div className="state-actions">
        <button type="button" className="primary-button" onClick={onRetry} disabled={!retryable}>
          {retryable ? "Retry stored open" : "Retry not available"}
        </button>
        <span className="fixture-pill">{scenario.statusLabel}</span>
      </div>
    </>
  );
}

interface FailureDetailsProps {
  readonly failure: AdapterFailure | EnumerationError | TransportFailure;
  readonly scenario: FixtureScenario;
}

/** Render stable failure code/source/provenance details. */
function FailureDetails({ failure, scenario }: FailureDetailsProps): JSX.Element {
  return (
    <>
      <dl className="detail-grid">
        <div>
          <dt>Code</dt>
          <dd>
            <code>{failure.code}</code>
          </dd>
        </div>
        <div>
          <dt>Source</dt>
          <dd>{failure.kind}</dd>
        </div>
        <div>
          <dt>Provenance</dt>
          <dd>{scenario.provenance.label}</dd>
        </div>
      </dl>
      <p className="state-provenance">{scenario.provenance.note}</p>
    </>
  );
}

interface FailureStateProps {
  readonly scenario: FixtureScenario;
  readonly failure: AdapterFailure | EnumerationError | TransportFailure;
  readonly onRetry: () => void;
  readonly raw?: unknown;
}

/** Display an open/transport/contract failure without inferring safety. */
export function FailureState({ scenario, failure, onRetry, raw }: FailureStateProps): JSX.Element {
  const descriptor = classifyAnalysisError(failure);
  const retryable = failure.kind === "transport" ? failure.retryable : true;
  return (
    <section className={`center-state state-tone state-tone--${descriptor.tone}`} aria-live="polite">
      <FailureIntro scenario={scenario} failure={failure} descriptor={descriptor} retryable={retryable} onRetry={onRetry} />
      <FailureDetails failure={failure} scenario={scenario} />
      <RawDetails raw={raw} />
    </section>
  );
}

interface TerminalStateProps {
  readonly scenario: FixtureScenario;
  readonly status: EnumerationStatus;
  readonly raw?: unknown;
}

/** Display a terminal open response with no instance and no safety claim. */
export function TerminalState({ scenario, status, raw }: TerminalStateProps): JSX.Element {
  const descriptor = describeEnumerationStatus(status);
  return (
    <section className="center-state" aria-live="polite">
      <span className="mono-label">TERMINAL · NO INSTANCE</span>
      <h1>{scenario.title}</h1>
      <p className="state-headline">{descriptor.headline}</p>
      <p>{descriptor.explanation}</p>
      <div className="terminal-rule" aria-hidden="true">
        {status === "exhausted" ? "UNSAT · exact finite model only" : "STOPPED · configured capacity only"}
      </div>
      <p className="state-caveat">No instance was supplied. Nothing here establishes global safety or the absence of counterexamples beyond these bounds.</p>
      <p className="state-provenance">{scenario.provenance.note}</p>
      <RawDetails raw={raw} />
    </section>
  );
}

interface DraftSummaryProps {
  readonly draft: DraftState;
  readonly errors: readonly string[];
}

/** Render intent/schema/readiness facts for the local draft. */
function DraftSummary({ draft, errors }: DraftSummaryProps): JSX.Element {
  return (
    <div className="draft-summary-grid">
      <div>
        <span className="mono-label">INTENT</span>
        <p>{draft.intent.trim() || "No business intent written yet."}</p>
        <span className="mono-label">SCHEMA</span>
        <p>{Object.keys(draft.catalog.tables).length} explicit tables in this draft context.</p>
      </div>
      <div>
        <span className="mono-label">READINESS</span>
        <p>
          {errors.length === 0 ? "The draft has the required fields for a compiler check." : `${errors.length} draft issues to resolve in the query builder.`}
        </p>
      </div>
    </div>
  );
}

/** Render the readable draft plan. */
function DraftPlan({ plan }: { readonly plan: readonly PlanStep[] }): JSX.Element {
  return (
    <ol className="draft-plan">
      {plan.map((step) => (
        <li key={step.key}>
          <strong>{step.title}</strong>
          <span>{step.detail}</span>
        </li>
      ))}
    </ol>
  );
}

interface DraftPreviewProps {
  readonly sql: string | null;
  readonly errors: readonly string[];
}

/** Render the unexecuted SQL preview or its first blocking reason. */
function DraftPreview({ sql, errors }: DraftPreviewProps): JSX.Element {
  return (
    <section className="draft-sql">
      <span className="mono-label">DRAFT SQL PREVIEW · NOT EXECUTED</span>
      {sql ? (
        <pre className="code-block">{sql}</pre>
      ) : (
        <div className="inline-alert">
          <strong>Preview unavailable</strong>
          <span>{errors[0] ?? "Finish the draft in the query builder."}</span>
        </div>
      )}
      {errors.length > 0 ? (
        <ul className="draft-errors">
          {errors.map((error) => (
            <li key={error}>{error}</li>
          ))}
        </ul>
      ) : null}
    </section>
  );
}

/** Explain the draft's current status and how to check it. */
function DraftIntro(): JSX.Element {
  return (
    <>
      <div className="state-heading">
        <span className="mono-label">LOCAL DRAFT · AWAITING ANALYSIS</span>
        <span className="status-stamp status-stamp--caution">Not validated</span>
      </div>
      <h1>Draft awaiting deterministic analysis</h1>
      <p>Check this draft in the query builder to see the generated IR and Alloy model. Start an analysis to create a review case.</p>
    </>
  );
}

interface DraftStatePanelProps {
  readonly scenario: FixtureScenario;
  readonly draft: DraftState;
  readonly onEdit: () => void;
  readonly onDiscard: () => void;
}

/** Render the draft and its preview before a new analysis creates evidence. */
export function DraftStatePanel({ scenario, draft, onEdit, onDiscard }: DraftStatePanelProps): JSX.Element {
  const errors = draftErrors(draft);
  return (
    <section className="center-state draft-state" aria-live="polite">
      <DraftIntro />
      <DraftSummary draft={draft} errors={errors} />
      <DraftPlan plan={readablePlan(draft)} />
      <DraftPreview sql={renderDraftSql(draft)} errors={errors} />
      <DraftFooter scenario={scenario} onEdit={onEdit} onDiscard={onDiscard} />
    </section>
  );
}

interface DraftFooterProps {
  readonly scenario: FixtureScenario;
  readonly onEdit: () => void;
  readonly onDiscard: () => void;
}

/** Render draft navigation actions and provenance. */
function DraftFooter({ scenario, onEdit, onDiscard }: DraftFooterProps): JSX.Element {
  return (
    <>
      <div className="state-actions">
        <button type="button" className="primary-button" onClick={onEdit}>
          Continue in query builder
        </button>
        <button type="button" className="ghost-button" onClick={onDiscard}>
          Discard draft & return to stored evidence
        </button>
      </div>
      <p className="state-provenance">
        Draft seed for {scenario.title}. Parameters keep numbered identity; typed NULLs are shown explicitly. No rows are executed.
      </p>
    </>
  );
}
