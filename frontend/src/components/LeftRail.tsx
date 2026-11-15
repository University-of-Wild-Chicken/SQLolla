/**
 * Left workspace rail: stored scenario library and the local draft entry point.
 * The typed relation builder lives in a separate module.
 */
import type { Dispatch, JSX } from "react";
import type { FixtureScenario } from "../analysis/types";
import type { DraftAction, DraftState } from "../builder/model";
import { BuilderPanel } from "./BuilderPanel";
import { NativeAnalysisPanel } from "./NativeAnalysisPanel";
import type { NativeAnalysis } from "../workspace/native";

/** Tabs available in the left rail. */
export type LeftTab = "library" | "builder" | "analysis";

/** Props shared by the left rail and its library/builder views. */
export interface LeftRailProps {
  readonly scenarios: readonly FixtureScenario[];
  readonly selectedId: string;
  readonly activeTab: LeftTab;
  readonly draft: DraftState | null;
  readonly onTab: (tab: LeftTab) => void;
  readonly onSelectScenario: (id: string) => void;
  readonly onStartDraft: () => void;
  readonly onDiscardDraft: () => void;
  readonly dispatchDraft: Dispatch<DraftAction>;
  readonly native: NativeAnalysis;
  readonly onCheckDraft: (draft: DraftState) => Promise<unknown>;
  readonly onStageDraft: (draft: DraftState) => void;
  readonly onCloseDrawer: () => void;
  readonly drawerOpen?: boolean;
  readonly inert?: boolean;
}

/** Render the mobile drawer head with close control. */
function RailMobileHead({ onClose }: { readonly onClose: () => void }): JSX.Element {
  return (
    <div className="rail-mobile-head">
      <span className="mono-label">CASES & DRAFT</span>
      <button type="button" className="icon-button" onClick={onClose} aria-label="Close cases and builder panel">
        ×
      </button>
    </div>
  );
}

interface LeftTabsProps {
  readonly activeTab: LeftTab;
  readonly onTab: (tab: LeftTab) => void;
}

/** Render the two-mode left tab switch. */
function LeftTabs({ activeTab, onTab }: LeftTabsProps): JSX.Element {
  return (
    <div className="left-tabs" aria-label="Left workspace mode">
      <button type="button" aria-pressed={activeTab === "library"} onClick={() => onTab("library")}>
        01 / Cases
      </button>
      <button type="button" aria-pressed={activeTab === "builder"} onClick={() => onTab("builder")}>
        02 / Query studio
      </button>
      <button type="button" aria-pressed={activeTab === "analysis"} onClick={() => onTab("analysis")}>
        03 / Analyze
      </button>
    </div>
  );
}

/** Render the library heading. */
function LibraryHeader(): JSX.Element {
  return (
    <div className="rail-heading">
      <span className="mono-label">SCENARIO LIBRARY</span>
      <h2>Start from a stored case</h2>
      <p>Open immutable example, counterexample and terminal-state evidence. Fixture data is labelled everywhere.</p>
    </div>
  );
}

interface ScenarioHeadProps {
  readonly label: string;
  readonly selected: boolean;
}

/** Render one scenario selection button with explicit status and fixture provenance. */
function ScenarioHead({ label, selected }: ScenarioHeadProps): JSX.Element {
  return (
    <span className="scenario-card__head">
      <span className="scenario-status">{label}</span>
      {selected ? <span className="selected-mark">OPEN</span> : null}
    </span>
  );
}

interface ScenarioCardProps {
  readonly scenario: FixtureScenario;
  readonly selected: boolean;
  readonly onSelect: () => void;
  readonly compact?: boolean;
}

function ScenarioCard({ scenario, selected, onSelect, compact = false }: ScenarioCardProps): JSX.Element {
  return (
    <li>
      <button
        type="button"
        data-scenario-id={scenario.id}
        className={`scenario-card ${selected ? "is-selected" : ""} ${compact ? "scenario-card--compact" : ""}`}
        onClick={onSelect}
        aria-current={selected ? "true" : undefined}
      >
        <ScenarioHead label={scenario.statusLabel} selected={selected} />
        <strong>{compact ? scenario.shortTitle : scenario.title}</strong>
        {!compact ? <span>{scenario.description}</span> : null}
        <small>{scenario.provenance.label}</small>
      </button>
    </li>
  );
}

interface ScenarioListProps {
  readonly scenarios: readonly FixtureScenario[];
  readonly selectedId: string;
  readonly onSelect: (id: string) => void;
  readonly compact?: boolean;
}

/** Render a list of scenario cards. */
function ScenarioList({ scenarios, selectedId, onSelect, compact }: ScenarioListProps): JSX.Element {
  return (
    <ul className={compact ? "scenario-list scenario-list--states" : "scenario-list"}>
      {scenarios.map((scenario) => (
        <ScenarioCard key={scenario.id} scenario={scenario} selected={scenario.id === selectedId} onSelect={() => onSelect(scenario.id)} compact={compact} />
      ))}
    </ul>
  );
}

interface TerminalLibraryProps {
  readonly scenarios: readonly FixtureScenario[];
  readonly selectedId: string;
  readonly onSelect: (id: string) => void;
}

/** Render collapsible demo terminal/error fixtures. */
function TerminalLibrary({ scenarios, selectedId, onSelect }: TerminalLibraryProps): JSX.Element {
  return (
    <details className="state-library">
      <summary>
        Demo terminal & error states <span>{scenarios.length}</span>
      </summary>
      <p>
        These fixtures exercise unsupported features, missing context, SQL errors, replay mismatches, resource limits, stale revisions, archive failures and a
        transport gap.
      </p>
      <ScenarioList scenarios={scenarios} selectedId={selectedId} onSelect={onSelect} compact />
    </details>
  );
}

interface DraftEntryProps {
  readonly draft: DraftState | null;
  readonly onStartDraft: () => void;
  readonly onTab: (tab: LeftTab) => void;
}

/** Render the local draft entry point. */
function DraftEntry({ draft, onStartDraft, onTab }: DraftEntryProps): JSX.Element {
  return (
    <div className="draft-entry">
      <span className="mono-label">REFINE THE QUESTION</span>
      <h3>Build a variation</h3>
      <p>Use typed cards for tables, columns, filters, joins and limits. Your draft is local, unvalidated, and sets the stored case aside.</p>
      {draft ? <span className="draft-inline-note">A draft is active. Open Query studio to continue or discard it.</span> : null}
      <DraftEntryAction draft={draft} onStartDraft={onStartDraft} onTab={onTab} />
    </div>
  );
}

interface DraftEntryActionProps {
  readonly draft: DraftState | null;
  readonly onStartDraft: () => void;
  readonly onTab: (tab: LeftTab) => void;
}

/** Render the draft entry action. */
function DraftEntryAction({ draft, onStartDraft, onTab }: DraftEntryActionProps): JSX.Element {
  return (
    <button type="button" className="primary-button" onClick={draft ? () => onTab("builder") : onStartDraft}>
      {draft ? "Continue draft" : "Start a local draft"}
    </button>
  );
}

/** Render the library footer's review boundary. */
function RailFootnote(): JSX.Element {
  return (
    <div className="rail-footnote">
      <strong>Evidence ≠ safety.</strong>
      <p>Fixture cases and swipes demonstrate the review loop. They are not solver-produced proof, a live query run, or deployment permission.</p>
    </div>
  );
}

/** Render the primary scenario cards and collapsible terminal demos. */
function Library({ scenarios, selectedId, draft, onSelectScenario, onStartDraft, onTab }: LeftRailProps): JSX.Element {
  const primary = scenarios.filter((scenario) => scenario.group === "primary");
  const states = scenarios.filter((scenario) => scenario.group === "states");
  return (
    <div className="rail-scroll">
      <LibraryHeader />
      <ScenarioList scenarios={primary} selectedId={selectedId} onSelect={onSelectScenario} />
      <TerminalLibrary scenarios={states} selectedId={selectedId} onSelect={onSelectScenario} />
      <DraftEntry draft={draft} onStartDraft={onStartDraft} onTab={onTab} />
      <RailFootnote />
    </div>
  );
}

/** Render the two-mode left rail with drawer close affordance. */
export function LeftRail(props: LeftRailProps): JSX.Element {
  return (
    <aside className={`rail rail--left ${props.drawerOpen ? "is-open" : ""}`} aria-label="Cases and query builder" inert={props.inert ? true : undefined}>
      <RailMobileHead onClose={props.onCloseDrawer} />
      <LeftTabs activeTab={props.activeTab} onTab={props.onTab} />
      {props.activeTab === "library" ? (
        <Library {...props} />
      ) : props.activeTab === "analysis" ? (
        <NativeAnalysisPanel analysis={props.native} />
      ) : (
        <BuilderPanel {...props} />
      )}
    </aside>
  );
}
