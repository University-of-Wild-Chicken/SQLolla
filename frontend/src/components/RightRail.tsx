/**
 * Right side evidence rail: staged path, grounded demo explanation, revision
 * requirements and the separate feedback ledger.
 */
import type { JSX } from "react";
import type { AiProviderSettings } from "../ai/summary";
import type { EvidenceView, FixtureScenario } from "../analysis/types";
import type { FeedbackJournal } from "../feedback/useFeedbackJournal";
import { AiSummaryPanel } from "./AiSummaryPanel";
import { JournalPanel } from "./JournalPanel";
import { RevisionPanel } from "./RevisionPanel";
import { StageFlow } from "./StageFlow";

interface RightRailProps {
  readonly scenario: FixtureScenario;
  readonly evidence: EvidenceView | null;
  readonly draftActive: boolean;
  readonly settings: AiProviderSettings;
  readonly journal: FeedbackJournal;
  readonly hasFeedback: boolean;
  readonly hasRejected: boolean;
  readonly onOpenSettings: () => void;
  readonly onInspectFact: (reference: string) => void;
  readonly onWithdraw: (eventIds: readonly string[]) => void;
  readonly onCloseDrawer: () => void;
  readonly drawerOpen?: boolean;
  readonly inert?: boolean;
  readonly bundleId?: string | null;
  readonly onApplyProposal?: (sql: string) => void;
}

/** Render the scrollable explanation, revision and ledger contents. */
function DraftModeNotice(): JSX.Element {
  return (
    <div className="inline-alert">
      <strong>Draft mode</strong>
      <span>The prior stored case is set aside. Explanations and AI citations no longer apply to the unvalidated draft.</span>
    </div>
  );
}

/** Render explanation and proposal path. */
function RightRailExplain(props: RightRailProps): JSX.Element {
  return (
    <>
      <StageFlow hasDraft={props.draftActive} hasEvidence={!!props.evidence} hasFeedback={props.hasFeedback} hasRejected={props.hasRejected} />
      <AiSummaryPanel
        scenario={props.scenario}
        evidence={props.draftActive ? null : props.evidence}
        settings={props.settings}
        onOpenSettings={props.onOpenSettings}
        onInspectFact={props.onInspectFact}
        bundleId={props.bundleId}
        onApplyProposal={props.onApplyProposal}
      />
    </>
  );
}

/** Render revision requirements and the separate review ledger. */
function RightRailRequirements(props: RightRailProps): JSX.Element {
  return (
    <>
      <RevisionPanel scenario={props.scenario} evidence={props.draftActive ? null : props.evidence} events={props.journal.events} />
      <div id="review-ledger">
        <JournalPanel journal={props.journal} currentRevision={props.evidence?.instance.modelRevisionSha256 ?? null} onWithdraw={props.onWithdraw} />
      </div>
    </>
  );
}

/** Render the scrollable explanation, revision and ledger contents. */
function RightRailBody(props: RightRailProps): JSX.Element {
  return (
    <div className="rail-scroll rail-scroll--right">
      {props.draftActive ? <DraftModeNotice /> : null}
      <RightRailExplain {...props} />
      <RightRailRequirements {...props} />
    </div>
  );
}

/** Render the scrollable explanation, revision and ledger rail. */
export function RightRail(props: RightRailProps): JSX.Element {
  return (
    <aside
      className={`rail rail--right ${props.drawerOpen ? "is-open" : ""}`}
      aria-label="Explanation, revision and ledger"
      inert={props.inert ? true : undefined}
    >
      <div className="rail-mobile-head">
        <span className="mono-label">EXPLAIN & REFINE</span>
        <button type="button" className="icon-button" onClick={props.onCloseDrawer} aria-label="Close explanation and ledger panel">
          ×
        </button>
      </div>
      <RightRailBody {...props} />
    </aside>
  );
}
