/**
 * Plain-language proposal path. This is UI orchestration only: deterministic
 * validation is shown as unavailable in the fixture workspace, and no stage is
 * presented as checked without a new analysis revision.
 */
import type { JSX } from "react";

type StageState = "done" | "current" | "blocked" | "pending";

interface Stage {
  readonly number: string;
  readonly title: string;
  readonly detail: string;
  readonly state: StageState;
}

const INTENT_STAGE: Stage = {
  number: "01",
  title: "Intent",
  detail: "Fixture scenario intent",
  state: "done",
};

const VALIDATION_STAGE: Stage = {
  number: "03",
  title: "Deterministic validation",
  detail: "Host compiler bridge not connected",
  state: "blocked",
};

/** Build the relational-plan stage. */
function planStage(hasDraft: boolean): Stage {
  return {
    number: "02",
    title: "Relational plan",
    detail: hasDraft ? "Card-built draft; unvalidated" : "Stored example plan available",
    state: hasDraft ? "current" : "done",
  };
}

/** Build the bounded-analysis stage. */
function analysisStage(hasEvidence: boolean): Stage {
  return {
    number: "04",
    title: "Bounded analysis",
    detail: hasEvidence ? "Stored bounded instance loaded" : "No current instance",
    state: hasEvidence ? "done" : "pending",
  };
}

/** Build the plan and validation stages from the current UI context. */
function planStages(hasDraft: boolean, hasEvidence: boolean): readonly Stage[] {
  return [INTENT_STAGE, planStage(hasDraft), VALIDATION_STAGE, analysisStage(hasEvidence)];
}

/** Build the review and proposed-revision stages. */
function reviewStages(hasFeedback: boolean, hasRejected: boolean): readonly Stage[] {
  return [
    {
      number: "05",
      title: "Instance review",
      detail: hasFeedback ? "Intent journaled" : "Awaiting an explicit label",
      state: hasFeedback ? "done" : "current",
    },
    {
      number: "06",
      title: "Proposed revision",
      detail: hasRejected ? "Exclusion requirements captured; not applied" : "No exclusion requirement yet",
      state: "pending",
    },
  ];
}

const STATE_LABEL: Readonly<Record<StageState, string>> = {
  done: "stored",
  current: "current",
  blocked: "blocked",
  pending: "not run",
};

/** Render one stage row with accessible state text. */
function StageRow({ stage }: { readonly stage: Stage }): JSX.Element {
  return (
    <li data-state={stage.state}>
      <span className="stage-number">{stage.number}</span>
      <div>
        <strong>{stage.title}</strong>
        <span>{stage.detail}</span>
      </div>
      <span className="stage-state">{STATE_LABEL[stage.state]}</span>
    </li>
  );
}

interface StageFlowProps {
  readonly hasDraft: boolean;
  readonly hasEvidence: boolean;
  readonly hasFeedback: boolean;
  readonly hasRejected: boolean;
}

/** Render the vertical stage rail with accessible state text. */
export function StageFlow({ hasDraft, hasEvidence, hasFeedback, hasRejected }: StageFlowProps): JSX.Element {
  const stages = [...planStages(hasDraft, hasEvidence), ...reviewStages(hasFeedback, hasRejected)];
  return (
    <section className="stage-flow" aria-label="Proposal path">
      <div className="rail-heading">
        <span className="mono-label">PROPOSAL PATH</span>
        <h2>From intent to a checked revision</h2>
        <p>UI orchestration, not a browser compiler or agent run.</p>
      </div>
      <ol>
        {stages.map((stage) => (
          <StageRow key={stage.number} stage={stage} />
        ))}
      </ol>
    </section>
  );
}
