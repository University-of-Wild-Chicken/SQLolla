/** A link to active review controls and the optional historical fixture suggestion. */
import type { JSX } from "react";
import type { EvidenceView, FixtureScenario } from "../analysis/types";
import { CopySqlButton } from "./CopySqlButton";

/** Render one explicitly unchecked proposal. */
function ProposalCard({ scenario }: { readonly scenario: FixtureScenario }): JSX.Element | null {
  const edit = scenario.summarySeed?.proposedEdit;
  if (!edit) return null;
  return (
    <div className="proposal-card">
      <span className="mono-label">UNCHECKED FIXTURE PROPOSAL</span>
      <h3>{scenario.summarySeed?.proposedEditLabel ?? "Proposed edit"}</h3>
      <pre>{edit}</pre>
      <CopySqlButton sql={edit} />
    </div>
  );
}

interface RevisionPanelProps {
  readonly scenario: FixtureScenario;
  readonly evidence: EvidenceView | null;
  readonly onOpenBins: () => void;
}

/** Link to the single active review list while preserving the optional fixture suggestion. */
export function RevisionPanel({ scenario, evidence, onOpenBins }: RevisionPanelProps): JSX.Element {
  return (
    <section className="revision-panel" aria-label="Refinement navigation">
      <div className="rail-heading">
        <h2>Refine from review</h2>
        <a href="#review-bins" onClick={onOpenBins}>
          Go to review bins
        </a>
      </div>
      {evidence ? <ProposalCard scenario={scenario} /> : null}
    </section>
  );
}
