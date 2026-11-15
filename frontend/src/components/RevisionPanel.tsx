/**
 * Future-revision requirements derived from active review labels. It never
 * implies that a backend agent applied, checked or promoted a change.
 */
import type { JSX } from "react";
import { shortHash } from "../analysis/format";
import type { EvidenceView, FixtureScenario } from "../analysis/types";
import { activeDecisions, detectConflicts, staleDecisions, type FeedbackConflict, type FeedbackEvent } from "../feedback/journal";

const DISPOSITION_LABEL: Readonly<Record<Exclude<FeedbackEvent["disposition"], "withdrawn">, string>> = {
  accepted_use_case: "Preserve",
  unintended_behavior: "Exclude",
};

/** Render one requirement row with instance/revision identity. */
function Requirement({ event }: { readonly event: FeedbackEvent }): JSX.Element {
  return (
    <li>
      <span className={`requirement-tag requirement-tag--${event.disposition}`}>
        {event.disposition === "withdrawn" ? "Withdrawn" : DISPOSITION_LABEL[event.disposition]}
      </span>
      <div>
        <strong>{event.disposition === "accepted_use_case" ? "Accepted use case" : "Unintended behavior"}</strong>
        <span>
          Instance {shortHash(event.instance_id)} · revision {shortHash(event.model_revision_sha256)}
        </span>
        {event.rationale ? <p>{event.rationale}</p> : null}
      </div>
    </li>
  );
}

interface RequirementColumnProps {
  readonly title: string;
  readonly events: readonly FeedbackEvent[];
  readonly empty: string;
}

/** Render one preservation or exclusion requirement column. */
function RequirementColumn({ title, events, empty }: RequirementColumnProps): JSX.Element {
  return (
    <div>
      <span className="mono-label">
        {title} · {events.length}
      </span>
      <ul>{events.length === 0 ? <li className="empty-note">{empty}</li> : events.map((event) => <Requirement key={event.event_id} event={event} />)}</ul>
    </div>
  );
}

interface RevisionWarningsProps {
  readonly conflicts: readonly FeedbackConflict[];
  readonly staleCount: number;
}

/** Render conflict and stale-revision notices for review intent. */
function RevisionWarnings({ conflicts, staleCount }: RevisionWarningsProps): JSX.Element {
  return (
    <>
      {conflicts.length > 0 ? (
        <div className="inline-alert" role="alert">
          <strong>Contradictory intent blocks promotion</strong>
          <span>
            {conflicts.length} observation{conflicts.length === 1 ? "" : "s"} carry both labels. Resolve them in the review ledger.
          </span>
        </div>
      ) : null}
      {staleCount > 0 ? (
        <div className="inline-alert">
          <strong>Stale-revision labels present</strong>
          <span>{staleCount} active label(s) target another revision and are not applied to this case.</span>
        </div>
      ) : null}
    </>
  );
}

/** Render one explicitly unchecked proposal. */
function ProposalCard({ scenario }: { readonly scenario: FixtureScenario }): JSX.Element {
  const edit = scenario.summarySeed?.proposedEdit;
  return (
    <div className="proposal-card">
      <div className="proposal-card__head">
        <span className="mono-label">UNCHECKED PROPOSAL</span>
        <span>Not applied</span>
      </div>
      {edit ? (
        <>
          <h3>{scenario.summarySeed?.proposedEditLabel ?? "Proposed edit"}</h3>
          <pre>{edit}</pre>
          <p>A deterministic compile and fresh bounded analysis are required before this can be displayed as checked.</p>
        </>
      ) : (
        <p>No proposed edit is supplied for this case. Requirements above are still only review intent.</p>
      )}
    </div>
  );
}

interface RevisionPanelProps {
  readonly scenario: FixtureScenario;
  readonly evidence: EvidenceView | null;
  readonly events: readonly FeedbackEvent[];
}

/** Render preservation/exclusion requirements and one explicitly unchecked proposal. */
export function RevisionPanel({ scenario, evidence, events }: RevisionPanelProps): JSX.Element {
  const decisions = activeDecisions(events);
  const accepted = decisions.filter((event) => event.disposition === "accepted_use_case");
  const rejected = decisions.filter((event) => event.disposition === "unintended_behavior");
  const conflicts = detectConflicts(events);
  const stale = staleDecisions(events, evidence?.instance.modelRevisionSha256 ?? null);
  return (
    <section className="revision-panel" aria-label="Future revision requirements">
      <div className="rail-heading">
        <span className="mono-label">NEXT REVISION</span>
        <h2>Requirements, not a fix</h2>
        <p>Accepted observations become preservation requirements. Rejected ones become exclusion requirements for a future checked revision.</p>
      </div>
      <RevisionWarnings conflicts={conflicts} staleCount={stale.length} />
      <div className="requirement-columns">
        <RequirementColumn title="PRESERVE" events={accepted} empty="No accepted use cases yet." />
        <RequirementColumn title="EXCLUDE" events={rejected} empty="No exclusion requirements yet." />
      </div>
      <ProposalCard scenario={scenario} />
      <p className="revision-footnote">
        No backend agent has run. A left swipe does not fix a query; it requests a revision and preserves the excluded observation in the evidence view.
      </p>
    </section>
  );
}
