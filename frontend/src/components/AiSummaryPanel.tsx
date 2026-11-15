/**
 * Untrusted demo explanation with claim-level fact citations. A missing or failed
 * explanation never blocks graph/table review or navigation.
 */
import { useEffect, useState, type JSX } from "react";
import { runMockExplanation, type ExplanationOutcome } from "../ai/provider";
import type { AiProviderSettings } from "../ai/summary";
import type { DemoClaim, DemoSummary, EvidenceView, FactModel, FixtureScenario } from "../analysis/types";
import { LiveAssistancePanel } from "../ai/LiveAssistancePanel";

interface AiSummaryPanelProps {
  readonly scenario: FixtureScenario;
  readonly evidence: EvidenceView | null;
  readonly settings: AiProviderSettings;
  readonly onOpenSettings: () => void;
  readonly onInspectFact: (reference: string) => void;
  readonly bundleId?: string | null;
  readonly onApplyProposal?: (sql: string) => void;
}

/** Resolve only exact supplied JSON Pointer facts for clickable citations. */
function citationsFor(claimReferences: readonly string[], facts: readonly FactModel[]): { readonly found: readonly FactModel[]; readonly missing: number } {
  const byReference = new Map(facts.map((fact) => [fact.reference, fact]));
  const found = claimReferences.flatMap((reference) => {
    const fact = byReference.get(reference);
    return fact ? [fact] : [];
  });
  return { found, missing: claimReferences.length - found.length };
}

/** Run the mock-only provider whenever the visible evidence or demo mode changes. */
function useExplanation(evidence: EvidenceView | null, scenario: FixtureScenario, settings: AiProviderSettings): ExplanationOutcome | null {
  const requestKey = `${evidence?.instance.instanceId ?? "none"}|${scenario.id}|${settings.mode}|${settings.simulateFailure}`;
  const [result, setResult] = useState<{ key: string; outcome: ExplanationOutcome } | null>(null);
  useEffect(() => {
    let cancelled = false;
    void runMockExplanation(evidence, scenario.summarySeed, settings).then((outcome) => {
      if (!cancelled) setResult({ key: requestKey, outcome });
    });
    return () => {
      cancelled = true;
    };
  }, [evidence, scenario, settings, requestKey]);
  return result?.key === requestKey ? result.outcome : null;
}

interface ExplanationHeaderProps {
  readonly onOpenSettings: () => void;
}

/** Render the explanation heading and settings affordance. */
function ExplanationHeader({ onOpenSettings }: ExplanationHeaderProps): JSX.Element {
  return (
    <div className="rail-heading rail-heading--row">
      <div>
        <span className="mono-label">EXPLANATION</span>
        <h2>What this case shows</h2>
      </div>
      <button type="button" className="icon-button icon-button--small" onClick={onOpenSettings} aria-label="Open explanation provider settings">
        ⚙
      </button>
    </div>
  );
}

interface ExplanationFailureProps {
  readonly outcome: Extract<ExplanationOutcome, { ok: false }>;
  readonly evidence: EvidenceView | null;
  readonly onOpenSettings: () => void;
}

/** Show a failed explanation while explicitly preserving evidence access. */
function ExplanationFailure({ outcome, evidence, onOpenSettings }: ExplanationFailureProps): JSX.Element {
  return (
    <div className="inline-alert inline-alert--danger" role="alert">
      <strong>{outcome.label} unavailable</strong>
      <span>{outcome.message}</span>
      <span>{evidence ? "Graph, table, facts, feedback and next-case navigation remain available." : "Evidence review remains available."}</span>
      <button type="button" className="text-button" onClick={onOpenSettings}>
        Review provider settings
      </button>
    </div>
  );
}

interface CitationButtonsProps {
  readonly facts: readonly FactModel[];
  readonly onInspectFact: (reference: string) => void;
}

/** Render citation buttons for one factual claim. */
function CitationButtons({ facts, onInspectFact }: CitationButtonsProps): JSX.Element {
  return (
    <div className="citations" aria-label="Evidence citations">
      {facts.map((fact) => (
        <button type="button" className="citation" key={fact.reference} onClick={() => onInspectFact(fact.reference)} title={fact.reference}>
          {fact.kind} · {fact.reference}
        </button>
      ))}
    </div>
  );
}

interface ClaimCardProps {
  readonly claim: DemoClaim;
  readonly evidence: EvidenceView;
  readonly onInspectFact: (reference: string) => void;
}

/** Render one untrusted claim and its validated citations. */
function ClaimCard({ claim, evidence, onInspectFact }: ClaimCardProps): JSX.Element {
  const citations = citationsFor(claim.factReferences, evidence.facts);
  return (
    <div className="ai-claim">
      <p>{claim.text}</p>
      {citations.missing > 0 ? (
        <p className="citation-warning" role="alert">
          This claim cites {citations.missing} fact(s) that were not supplied. Do not treat it as grounded.
        </p>
      ) : null}
      {citations.found.length > 0 ? <CitationButtons facts={citations.found} onInspectFact={onInspectFact} /> : null}
    </div>
  );
}

interface ClaimsListProps {
  readonly summary: DemoSummary;
  readonly evidence: EvidenceView;
  readonly onInspectFact: (reference: string) => void;
}

/** Render the summary claims. */
function ClaimsList({ summary, evidence, onInspectFact }: ClaimsListProps): JSX.Element {
  return (
    <div className="ai-claims">
      {summary.claims.length === 0 ? <p>No natural-language claims are shown because no supplied facts were available to cite.</p> : null}
      {summary.claims.map((claim, index) => (
        <ClaimCard key={`${index}-${claim.text.slice(0, 12)}`} claim={claim} evidence={evidence} onInspectFact={onInspectFact} />
      ))}
    </div>
  );
}

interface AssumptionsBlockProps {
  readonly assumptions: readonly string[];
}

/** Render assumptions and untrusted-text boundary for a successful summary. */
function AssumptionsBlock({ assumptions }: AssumptionsBlockProps): JSX.Element {
  return (
    <>
      <div className="assumption-block">
        <span className="mono-label">ASSUMPTIONS & SCOPE</span>
        <ul>
          {assumptions.map((assumption) => (
            <li key={assumption}>{assumption}</li>
          ))}
        </ul>
      </div>
      <p className="ai-untrusted">Rendered as untrusted text. It cannot change solver status, bounds, evidence or feedback.</p>
    </>
  );
}

interface ExplanationSuccessProps {
  readonly outcome: Extract<ExplanationOutcome, { ok: true }>;
  readonly evidence: EvidenceView;
  readonly onInspectFact: (reference: string) => void;
}

/** Render a successful fixture explanation with exact fact citations. */
function ExplanationSuccess({ outcome, evidence, onInspectFact }: ExplanationSuccessProps): JSX.Element {
  const summary = outcome.summary;
  return (
    <div className="ai-content">
      <div className="demo-content-tag">
        <span>DEMO CONTENT</span>
        <span>{outcome.label} · fixture explanation</span>
      </div>
      <h3>{summary.title}</h3>
      <ClaimsList summary={summary} evidence={evidence} onInspectFact={onInspectFact} />
      <AssumptionsBlock assumptions={summary.assumptions} />
    </div>
  );
}

interface PanelStatusProps {
  readonly evidence: EvidenceView | null;
  readonly outcome: ExplanationOutcome | null;
  readonly onOpenSettings: () => void;
  readonly onInspectFact: (reference: string) => void;
}

/** Render the active explanation state or its unavailable fallback. */
function PanelStatus({ evidence, outcome, onOpenSettings, onInspectFact }: PanelStatusProps): JSX.Element {
  return (
    <>
      {!evidence ? <p className="empty-note">Open a stored case to see a fixture explanation. The explanation layer never replaces evidence.</p> : null}
      {evidence && !outcome ? (
        <div className="ai-loading" role="status">
          Preparing mock explanation…
        </div>
      ) : null}
      {outcome && !outcome.ok ? <ExplanationFailure outcome={outcome} evidence={evidence} onOpenSettings={onOpenSettings} /> : null}
      {outcome?.ok && evidence ? <ExplanationSuccess outcome={outcome} evidence={evidence} onInspectFact={onInspectFact} /> : null}
    </>
  );
}

/** Render the demo summary, its citations, assumptions and failure state. */
export function AiSummaryPanel({ scenario, evidence, settings, onOpenSettings, onInspectFact, bundleId, onApplyProposal }: AiSummaryPanelProps): JSX.Element {
  const outcome = useExplanation(evidence, scenario, settings);
  return (
    <section className="ai-panel" aria-label="Explanation">
      <ExplanationHeader onOpenSettings={onOpenSettings} />
      {settings.mode === "openai_compatible" && evidence ? (
        <LiveAssistancePanel
          bundleId={bundleId ?? null}
          evidence={evidence}
          settings={settings}
          onInspectFact={onInspectFact}
          onApplyProposal={onApplyProposal}
        />
      ) : (
        <PanelStatus evidence={evidence} outcome={outcome} onOpenSettings={onOpenSettings} onInspectFact={onInspectFact} />
      )}
      <p className="ai-disclaimer">
        {settings.mode === "offline_demo"
          ? "Offline fixture content. No provider request is made. "
          : "Live requests require an explicit action and use the fixed model. "}
        Citations prove that a supplied fact exists, not that prose is true.
      </p>
    </section>
  );
}
