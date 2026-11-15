/**
 * Presentational workspace shell over the local controller. It exposes fixture
 * status, progressive evidence review and explicit local analysis actions.
 */
import type { JSX } from "react";
import { DEMO_SCENARIOS, scenarioById } from "../analysis/fixtures";
import type { EnumerationStatus, EvidenceView } from "../analysis/types";
import { Footer, Header, JourneyBar } from "./AppChrome";
import { AiSettingsDialog } from "./AiSettingsDialog";
import { DetailsDrawer } from "./DetailsDrawer";
import { LeftRail } from "./LeftRail";
import { Modal } from "./Modal";
import { ReviewCard } from "./ReviewCard";
import { RightRail } from "./RightRail";
import { DraftStatePanel, FailureState, OpeningState, TerminalState } from "./WorkspaceStates";
import type { Workspace } from "../workspace/useWorkspace";
import { NativeDraftState } from "./NativeAnalysisPanel";

/** Render the masthead and workflow reminder. */
function Masthead({ workspace }: { readonly workspace: Workspace }): JSX.Element {
  const { drawers, dialogs, currentEvidence, transport } = workspace;
  return (
    <>
      <Header
        transport={transport}
        hasEvidence={!!currentEvidence}
        onOpenLibrary={() => {
          drawers.setRightOpen(false);
          drawers.setLeftOpen(true);
        }}
        onOpenExplain={() => {
          drawers.setLeftOpen(false);
          drawers.setRightOpen(true);
        }}
        onOpenSettings={dialogs.openSettings}
        onOpenDetails={() => dialogs.openDetails("sql")}
      />
      <JourneyBar draftActive={!!workspace.navigation.draft} hasEvidence={!!currentEvidence} />
    </>
  );
}

interface EvidenceCenterProps {
  readonly workspace: Workspace;
  readonly evidence: EvidenceView;
}

/** Render one evidence card bound to the controller. */
function EvidenceCenter({ workspace, evidence }: EvidenceCenterProps): JSX.Element {
  const { session, selectors, dialogs, recordDecision, undoDecision, openLedger } = workspace;
  return (
    <ReviewCard
      key={evidence.instance.instanceId}
      scenario={workspace.navigation.selectedScenario}
      evidence={evidence}
      status={session.state.status ?? "instance"}
      cursor={session.state.cursor}
      pendingNext={session.state.pendingNext}
      nextFailure={session.state.nextFailure}
      staleNotice={session.state.staleNotice}
      activeDecision={selectors.activeDecision}
      conflict={selectors.currentConflict}
      onDecision={recordDecision}
      onUndo={undoDecision}
      onNext={() => void session.requestNext()}
      onOpenDetails={() => dialogs.openDetails("sql")}
      onOpenJson={() => dialogs.openDetails("json")}
      onInspectFact={dialogs.inspectFact}
      onOpenLedger={openLedger}
    />
  );
}

/** Render the draft state and its editor link. */
function DraftCenter({ workspace }: { readonly workspace: Workspace }): JSX.Element {
  const { navigation, drawers, discardDraft } = workspace;
  return (
    <DraftStatePanel
      scenario={navigation.selectedScenario}
      draft={navigation.draft!}
      onEdit={() => {
        navigation.setLeftTab("builder");
        if (drawers.leftIsDrawer) drawers.setLeftOpen(true);
      }}
      onDiscard={discardDraft}
    />
  );
}

/** Render a failed open state with retry. */
function FailureCenter({ workspace }: { readonly workspace: Workspace }): JSX.Element {
  const failure = workspace.session.state.openFailure!;
  const raw = "raw" in failure ? failure.raw : null;
  return (
    <FailureState
      scenario={workspace.navigation.selectedScenario}
      failure={failure}
      raw={raw}
      onRetry={() => void (workspace.session.state.native ? workspace.native.start() : workspace.session.openScenario(workspace.navigation.selectedScenario))}
    />
  );
}

interface TerminalCenterProps {
  readonly workspace: Workspace;
  readonly status: EnumerationStatus;
}

/** Render a terminal open state. */
function TerminalCenter({ workspace, status }: TerminalCenterProps): JSX.Element {
  return <TerminalState scenario={workspace.navigation.selectedScenario} status={status} raw={workspace.session.state.openRaw} />;
}

/** Render the mutually exclusive center state. */
function CenterContent({ workspace }: { readonly workspace: Workspace }): JSX.Element {
  const { navigation, session } = workspace;
  const state = session.state;
  if (navigation.draft) return <DraftCenter workspace={workspace} />;
  if (navigation.leftTab === "analysis" && workspace.native.dirty && !workspace.native.pending && state.phase !== "failed") return <NativeDraftState />;
  if (state.scenarioId !== navigation.selectedId || state.phase === "idle" || state.phase === "opening") {
    return <OpeningState scenario={navigation.selectedScenario} />;
  }
  if (state.openFailure) return <FailureCenter workspace={workspace} />;
  if (!state.evidence && (state.status === "exhausted" || state.status === "limit")) {
    return <TerminalCenter workspace={workspace} status={state.status} />;
  }
  return state.evidence ? <EvidenceCenter workspace={workspace} evidence={state.evidence} /> : <OpeningState scenario={navigation.selectedScenario} />;
}

/** Render the left rail with responsive drawer/inert state. */
function WorkspaceLeftRail({ workspace }: { readonly workspace: Workspace }): JSX.Element {
  const { navigation, drawers } = workspace;
  return (
    <LeftRail
      scenarios={DEMO_SCENARIOS}
      selectedId={navigation.selectedId}
      activeTab={navigation.leftTab}
      draft={navigation.draft}
      onTab={navigation.setLeftTab}
      onSelectScenario={workspace.chooseScenario}
      onStartDraft={workspace.startDraft}
      onDiscardDraft={workspace.discardDraft}
      dispatchDraft={navigation.dispatchDraft}
      native={workspace.native}
      onCheckDraft={workspace.checkDraft}
      onStageDraft={workspace.stageDraft}
      onCloseDrawer={() => drawers.setLeftOpen(false)}
      drawerOpen={drawers.leftOpen}
      inert={drawers.leftIsDrawer && !drawers.leftOpen}
    />
  );
}

/** Render the right rail with responsive drawer/inert state. */
function WorkspaceRightRail({ workspace }: { readonly workspace: Workspace }): JSX.Element {
  const { navigation, drawers, dialogs, journal, selectors, currentEvidence } = workspace;
  return (
    <RightRail
      scenario={navigation.selectedScenario}
      evidence={currentEvidence}
      bundleId={workspace.session.state.bundleId}
      onApplyProposal={workspace.applyProposal}
      draftActive={!!navigation.draft}
      settings={dialogs.aiSettings}
      journal={journal}
      hasFeedback={selectors.hasFeedback}
      hasRejected={selectors.hasRejected}
      onOpenSettings={dialogs.openSettings}
      onInspectFact={dialogs.inspectFact}
      onWithdraw={(ids) => journal.withdraw(ids)}
      onCloseDrawer={() => drawers.setRightOpen(false)}
      drawerOpen={drawers.rightOpen}
      inert={drawers.rightIsDrawer && !drawers.rightOpen}
    />
  );
}

/** Render the workspace rails and their exclusive center column. */
function WorkspaceBody({ workspace }: { readonly workspace: Workspace }): JSX.Element {
  const { drawers } = workspace;
  return (
    <main id="review-workspace" className="workspace">
      {drawers.leftOpen || drawers.rightOpen ? (
        <button type="button" className="drawer-scrim" aria-label="Close overlay panel" onClick={drawers.close} />
      ) : null}
      <WorkspaceLeftRail workspace={workspace} />
      <section className="center-column" id="review-center" inert={drawers.leftOpen || drawers.rightOpen ? true : undefined}>
        <CenterContent workspace={workspace} />
      </section>
      <WorkspaceRightRail workspace={workspace} />
    </main>
  );
}

/** Render the local details and provider settings dialogs. */
function SettingsDialogs({ workspace }: { readonly workspace: Workspace }): JSX.Element {
  const { dialogs, session, currentEvidence } = workspace;
  return (
    <>
      <DetailsDrawer
        open={dialogs.details.open}
        onClose={dialogs.closeDetails}
        evidence={currentEvidence}
        initialTab={dialogs.details.tab}
        focusFact={dialogs.details.fact}
        openRaw={session.state.openRaw}
        nextRaw={session.state.nextRaw}
        native={session.state.native}
      />
      <AiSettingsDialog
        open={dialogs.settingsOpen}
        onClose={dialogs.closeSettings}
        settings={dialogs.aiSettings}
        evidence={currentEvidence}
        onSave={dialogs.saveAiSettings}
        onDisconnect={dialogs.disconnectAi}
      />
    </>
  );
}

/** Render draft discard actions. */
function ConfirmDraftActions({ workspace }: { readonly workspace: Workspace }): JSX.Element {
  const { navigation, confirmDiscard } = workspace;
  return (
    <div className="dialog-actions">
      <button type="button" className="ghost-button" onClick={() => navigation.setPendingScenario(null)}>
        Keep draft
      </button>
      <span className="spacer" />
      <button type="button" className="primary-button" onClick={confirmDiscard}>
        Discard & open case
      </button>
    </div>
  );
}

/** Render the explicit confirmation before a draft is discarded. */
function ConfirmDraftDialog({ workspace }: { readonly workspace: Workspace }): JSX.Element {
  const { navigation } = workspace;
  const pendingTitle = navigation.pendingScenario ? scenarioById(navigation.pendingScenario)?.title : "another case";
  return (
    <Modal
      open={navigation.pendingScenario !== null}
      onClose={() => navigation.setPendingScenario(null)}
      title="Discard the local draft?"
      eyebrow="DRAFT IS SEPARATE FROM EVIDENCE"
      className="confirm-dialog"
    >
      <p>
        The draft is unvalidated and was generated in this browser. Opening <strong>{pendingTitle}</strong> discards it and returns to stored evidence.
      </p>
      <ConfirmDraftActions workspace={workspace} />
    </Modal>
  );
}

/** Render dialogs plus announced journal state. */
function WorkspaceDialogs({ workspace }: { readonly workspace: Workspace }): JSX.Element {
  return (
    <>
      <SettingsDialogs workspace={workspace} />
      <ConfirmDraftDialog workspace={workspace} />
      <p className="sr-only" role="status">
        {workspace.journal.notice ?? workspace.journal.storageError ?? ""}
      </p>
      {workspace.navigation.pendingScenario ? <span className="sr-only">Draft discard confirmation pending.</span> : null}
    </>
  );
}

/** Render the complete local workspace. */
export function WorkspaceView({ workspace }: { readonly workspace: Workspace }): JSX.Element {
  return (
    <div className="app-shell">
      <Masthead workspace={workspace} />
      <WorkspaceBody workspace={workspace} />
      <Footer transport={workspace.transport} />
      <WorkspaceDialogs workspace={workspace} />
    </div>
  );
}
