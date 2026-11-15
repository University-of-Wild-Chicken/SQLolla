/**
 * Local workspace orchestration for scenarios, drawers, dialogs, draft state and
 * review intent. Explicit native actions use the injected local host transport.
 */
import { useCallback, useEffect, useMemo, useReducer, useRef, useState } from "react";
import { NATIVE_SCENARIO, STARTER_CATALOG } from "../analysis/starter";
import { useAnalysisSession, type AnalysisSession } from "../analysis/session";
import type { AnalysisTransport, EvidenceView, FixtureScenario } from "../analysis/types";
import { defaultAiSettings, type AiProviderSettings } from "../ai/summary";
import { draftReducer, type DraftAction, type DraftState } from "../builder/model";
import type { DetailsTab } from "../components/DetailsDrawer";
import type { LeftTab } from "../components/LeftRail";
import { activeDecisions, activeDecisionFor, detectConflicts, type FeedbackEvent, type ReviewDisposition } from "../feedback/journal";
import { useFeedbackJournal, type FeedbackJournal } from "../feedback/useFeedbackJournal";
import { useMediaQuery } from "../hooks/useMediaQuery";
import type { Dispatch } from "react";
import { nativeScenario, useNativeAnalysis, type NativeAnalysis } from "./native";
import { draftRequest } from "../builder/request";
import { useArchivedReview } from "./archivedReview";
import { useRefinement } from "./refinement";

/** Navigation and local draft state. */
export interface WorkspaceNavigation {
  readonly initial: FixtureScenario;
  readonly selectedId: string;
  readonly selectedScenario: FixtureScenario;
  readonly leftTab: LeftTab;
  readonly draft: DraftState | null;
  readonly pendingScenario: string | null;
  readonly setSelectedId: (id: string) => void;
  readonly setLeftTab: (tab: LeftTab) => void;
  readonly dispatchDraft: Dispatch<DraftAction>;
  readonly setPendingScenario: (id: string | null) => void;
}

/** Responsive drawer state and safe close actions. */
export interface WorkspaceDrawers {
  readonly leftIsDrawer: boolean;
  readonly rightIsDrawer: boolean;
  readonly leftOpen: boolean;
  readonly rightOpen: boolean;
  readonly setLeftOpen: (open: boolean) => void;
  readonly setRightOpen: (open: boolean) => void;
  readonly close: () => void;
}

interface DetailsState {
  readonly open: boolean;
  readonly tab: DetailsTab;
  readonly fact: string | null;
}

/** Dialog and in-memory BYOT settings state. */
export interface WorkspaceDialogs {
  readonly details: DetailsState;
  readonly settingsOpen: boolean;
  readonly aiSettings: AiProviderSettings;
  readonly showAiSetup: boolean;
  readonly dismissAiSetup: () => void;
  readonly openDetails: (tab?: DetailsTab) => void;
  readonly inspectFact: (reference: string) => void;
  readonly closeDetails: () => void;
  readonly openSettings: () => void;
  readonly closeSettings: () => void;
  readonly closeAll: () => void;
  readonly saveAiSettings: (settings: AiProviderSettings) => void;
  readonly disconnectAi: () => void;
}

/** Derived feedback selectors for the current card. */
export interface ReviewSelectors {
  readonly target: ReturnType<typeof targetFor>;
  readonly activeDecision: FeedbackEvent | undefined;
  readonly currentConflict: boolean;
  readonly hasFeedback: boolean;
  readonly hasRejected: boolean;
}

/** Complete controller consumed by the presentational workspace. */
export interface Workspace {
  readonly transport: AnalysisTransport;
  readonly fixtures: readonly FixtureScenario[];
  readonly session: AnalysisSession;
  readonly native: NativeAnalysis;
  readonly journal: FeedbackJournal;
  readonly navigation: WorkspaceNavigation;
  readonly drawers: WorkspaceDrawers;
  readonly dialogs: WorkspaceDialogs;
  readonly selectors: ReviewSelectors;
  readonly currentEvidence: EvidenceView | null;
  readonly archive: ReturnType<typeof useArchivedReview>;
  readonly refinement: ReturnType<typeof useRefinement>;
  readonly chooseScenario: (id: string) => void;
  readonly confirmDiscard: () => void;
  readonly startDraft: () => void;
  readonly discardDraft: () => void;
  readonly recordDecision: (disposition: ReviewDisposition, rationale: string) => void;
  readonly undoDecision: () => void;
  readonly openLedger: () => void;
  readonly checkDraft: (draft: DraftState) => Promise<unknown>;
  readonly stageDraft: (draft: DraftState) => void;
}

/** Derive a stable target identity for journal labels. */
function targetFor(evidence: EvidenceView | null, bundleId: string | null) {
  return evidence
    ? {
        ...(bundleId ? { bundleId } : {}),
        instanceId: evidence.instance.instanceId,
        modelRevisionSha256: evidence.instance.modelRevisionSha256,
        semanticSha256: evidence.instance.semanticSha256,
      }
    : null;
}

/** Initialise scenario selection, rail mode and draft state. */
function useNavigation(fixtures: readonly FixtureScenario[], initialScenarioId?: string): WorkspaceNavigation {
  const initial = fixtures.find(({ id }) => id === initialScenarioId) ?? fixtures[0] ?? NATIVE_SCENARIO;
  const [selectedId, setSelectedId] = useState(initial.id);
  const [leftTab, setLeftTab] = useState<LeftTab>(fixtures.length ? "library" : "analysis");
  const [draft, dispatchDraft] = useReducer(draftReducer, null);
  const [pendingScenario, setPendingScenario] = useState<string | null>(null);
  return {
    initial,
    selectedId,
    selectedScenario: fixtures.find(({ id }) => id === selectedId) ?? initial,
    leftTab,
    draft,
    pendingScenario,
    setSelectedId,
    setLeftTab,
    dispatchDraft,
    setPendingScenario,
  };
}

/** Install Escape handling for responsive drawers. */
function useDrawerEscape(close: () => void): void {
  useEffect(() => {
    const onKey = (event: KeyboardEvent): void => {
      if (event.key === "Escape") close();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [close]);
}

/** Lock document scrolling while a mobile drawer is open. */
function useBodyLock(locked: boolean): void {
  useEffect(() => {
    document.body.classList.toggle("drawer-locked", locked);
    return () => document.body.classList.remove("drawer-locked");
  }, [locked]);
}

/** Manage responsive rails without retaining inaccessible focus targets. */
function useDrawers(fixtureMode: boolean): WorkspaceDrawers {
  const leftIsDrawer = useMediaQuery("(max-width: 899px)") && fixtureMode;
  const rightIsDrawer = useMediaQuery("(max-width: 1199px)") && fixtureMode;
  const [leftOpen, setLeftOpen] = useState(false);
  const [rightOpen, setRightOpen] = useState(false);
  const close = useCallback(() => {
    setLeftOpen(false);
    setRightOpen(false);
  }, []);
  useDrawerEscape(close);
  useBodyLock(leftOpen || rightOpen);
  return { leftIsDrawer, rightIsDrawer, leftOpen, rightOpen, setLeftOpen, setRightOpen, close };
}

/** Manage the details drawer and fact focus. */
function useDetailsDialog(closeDrawers: () => void) {
  const [details, setDetails] = useState<DetailsState>({ open: false, tab: "sql", fact: null });
  const openDetails = (tab: DetailsTab = "sql"): void => {
    closeDrawers();
    setDetails({ open: true, tab, fact: null });
  };
  const inspectFact = (reference: string): void => {
    closeDrawers();
    setDetails({ open: true, tab: "facts", fact: reference });
  };
  const closeDetails = (): void => setDetails((current) => ({ ...current, open: false }));
  return { details, openDetails, inspectFact, closeDetails };
}

const AI_SETUP_PROMPT_KEY = "sqlolla.ai.setup-prompt.v1";

/** Read only the onboarding marker. Browser storage never supplies provider settings. */
function firstAiVisit(): boolean {
  try {
    return window.localStorage.getItem(AI_SETUP_PROMPT_KEY) !== "seen";
  } catch {
    return true;
  }
}

/** Manage in-memory provider settings and remember only the user's onboarding choice. */
function useProviderDialog(closeDrawers: () => void) {
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [aiSettings, setAiSettings] = useState<AiProviderSettings>(defaultAiSettings);
  const [showAiSetup, setShowAiSetup] = useState(firstAiVisit);
  const dismissAiSetup = (): void => {
    setShowAiSetup(false);
    try {
      window.localStorage.setItem(AI_SETUP_PROMPT_KEY, "seen");
    } catch {
      // The choice still applies to the mounted workspace when storage is unavailable.
    }
  };
  const openSettings = (): void => {
    closeDrawers();
    dismissAiSetup();
    setSettingsOpen(true);
  };
  return {
    settingsOpen,
    aiSettings,
    showAiSetup,
    dismissAiSetup,
    openSettings,
    closeSettings: () => setSettingsOpen(false),
    saveAiSettings: setAiSettings,
    disconnectAi: () => setAiSettings(defaultAiSettings),
  };
}

/** Manage details, provider settings and memory-only credentials. */
function useDialogs(closeDrawers: () => void): WorkspaceDialogs {
  const detailsDialog = useDetailsDialog(closeDrawers);
  const providerDialog = useProviderDialog(closeDrawers);
  return {
    ...detailsDialog,
    ...providerDialog,
    closeAll: () => {
      closeDrawers();
      detailsDialog.closeDetails();
      providerDialog.closeSettings();
    },
  };
}

/** Open the selected stored revision once per selection. */
function useAutoOpen(session: AnalysisSession, scenario: FixtureScenario, enabled: boolean): void {
  const last = useRef<string | null>(null);
  useEffect(() => {
    if (!enabled || last.current === scenario.id) return;
    last.current = scenario.id;
    void session.openScenario(scenario);
  }, [enabled, scenario, session.openScenario]);
}

/** Derive current intent labels and contradiction state. */
function useReviewSelectors(journal: FeedbackJournal, evidence: EvidenceView | null, bundleId: string | null): ReviewSelectors {
  const target = targetFor(evidence, bundleId);
  const activeDecision = target ? activeDecisionFor(journal.events, target) : undefined;
  const currentConflict = evidence
    ? detectConflicts(journal.events).some(
        (conflict) => conflict.revision === evidence.instance.modelRevisionSha256 && conflict.semantic === evidence.instance.semanticSha256,
      )
    : false;
  const decisions = activeDecisions(journal.events);
  return {
    target,
    activeDecision,
    currentConflict,
    hasFeedback: decisions.length > 0,
    hasRejected: decisions.some((event) => event.disposition === "unintended_behavior"),
  };
}

/** Resolve an explicit scenario selection or stage a discard confirmation. */
function useChooseScenario(navigation: WorkspaceNavigation, drawers: WorkspaceDrawers, session: AnalysisSession): (id: string) => void {
  return (id: string): void => {
    if (navigation.draft && id !== navigation.selectedId) {
      navigation.setPendingScenario(id);
      return;
    }
    if (id === navigation.selectedId && (session.state.phase === "failed" || session.state.native)) {
      void session.openScenario(navigation.selectedScenario);
    }
    if (id !== navigation.selectedId) navigation.setSelectedId(id);
    navigation.setLeftTab("library");
    drawers.close();
  };
}

/** Confirm a draft-discard action and open the queued scenario. */
function useConfirmDiscard(navigation: WorkspaceNavigation, drawers: WorkspaceDrawers): () => void {
  return (): void => {
    const next = navigation.pendingScenario;
    navigation.setPendingScenario(null);
    navigation.dispatchDraft({ type: "reset" });
    navigation.setLeftTab("library");
    if (next) navigation.setSelectedId(next);
    drawers.close();
  };
}

/** Start and discard local drafts while closing unrelated detail surfaces. */
function useDraftActions(
  navigation: WorkspaceNavigation,
  dialogs: WorkspaceDialogs,
): {
  readonly startDraft: () => void;
  readonly discardDraft: () => void;
} {
  const startDraft = (): void => {
    navigation.dispatchDraft({
      type: "start",
      catalog: navigation.selectedScenario.builderCatalog ?? STARTER_CATALOG,
    });
    navigation.setLeftTab("builder");
    dialogs.closeAll();
  };
  const discardDraft = (): void => {
    navigation.dispatchDraft({ type: "reset" });
    navigation.setLeftTab(navigation.initial.id === NATIVE_SCENARIO.id ? "analysis" : "library");
  };
  return { startDraft, discardDraft };
}

/** Dispatch review decisions into the append-only journal. */
function useFeedbackActions(journal: FeedbackJournal, selectors: ReviewSelectors) {
  const recordDecision = (disposition: ReviewDisposition, rationale: string): void => {
    if (selectors.target) journal.recordDecision(selectors.target, disposition, rationale);
  };
  const undoDecision = (): void => {
    if (selectors.target) journal.undo(selectors.target);
  };
  return { recordDecision, undoDecision };
}

/** Scroll to and, when required, open the responsive review ledger. */
function useOpenLedger(drawers: WorkspaceDrawers): () => void {
  return (): void => {
    if (drawers.rightIsDrawer) drawers.setRightOpen(true);
    window.setTimeout(() => {
      const ledger = document.getElementById("review-ledger");
      if (ledger instanceof HTMLDetailsElement) ledger.open = true;
      ledger?.scrollIntoView?.({ block: "start" });
    }, 0);
  };
}

/** Compose shared editor, evidence and drawer state before attaching explicit actions. */
function useWorkspaceFrame(transport: AnalysisTransport, initialScenarioId?: string) {
  const fixtures = transport.fixtures ?? [];
  const originalNavigation = useNavigation(fixtures, initialScenarioId);
  const drawers = useDrawers(fixtures.length > 0);
  const dialogs = useDialogs(drawers.close);
  const session = useAnalysisSession(transport);
  const native = useNativeAnalysis(session);
  const navigation = { ...originalNavigation, selectedScenario: nativeScenario(session, originalNavigation.selectedScenario) };
  const journal = useFeedbackJournal();
  useAutoOpen(session, navigation.selectedScenario, fixtures.length > 0);
  const scope = useMemo(() => ({}), [navigation.selectedId, session.state.generation, !!navigation.draft]);
  const archive = useArchivedReview(transport, scope);
  const draftActive = !!navigation.draft || (navigation.leftTab === "analysis" && native.dirty);
  const saved = archive.visible?.result;
  const currentEvidence = saved?.evidence ?? (draftActive ? null : session.state.evidence);
  const selectors = useReviewSelectors(journal, currentEvidence, saved?.bundleId ?? session.state.bundleId);
  return { transport, fixtures, session, native, journal, navigation, drawers, dialogs, selectors, currentEvidence, archive };
}

/** Compose the complete local workspace controller. */
export function useWorkspace(transport: AnalysisTransport, initialScenarioId?: string): Workspace {
  const frame = useWorkspaceFrame(transport, initialScenarioId);
  const { navigation, drawers, session, dialogs, journal, selectors, native } = frame;
  const chooseScenario = useChooseScenario(navigation, drawers, session);
  const confirmDiscard = useConfirmDiscard(navigation, drawers);
  const actions = {
    ...useDraftActions(navigation, dialogs),
    ...useFeedbackActions(journal, selectors),
    ...useNativeActions(transport, session, navigation, native),
    openLedger: useOpenLedger(drawers),
  };
  return {
    ...frame,
    chooseScenario,
    confirmDiscard,
    refinement: useWorkspaceRefinement(frame),
    ...actions,
  };
}

/** Stage only the checked request, leaving persisted evidence and labels available for reconsideration. */
function useWorkspaceRefinement(frame: ReturnType<typeof useWorkspaceFrame>) {
  const { transport, session, navigation, native, archive, dialogs, drawers, journal } = frame;
  const onAdopt = (request: Record<string, unknown>): void => {
    archive.close();
    native.stage(request);
    navigation.setLeftTab("analysis");
    drawers.setLeftOpen(drawers.leftIsDrawer);
    window.setTimeout(() => document.getElementById("review-center")?.scrollIntoView?.({ block: "start" }), 0);
  };
  return useRefinement({
    transport,
    session: session.state,
    draftActive: !!navigation.draft || native.dirty,
    journal,
    archiveErrors: archive.errors,
    settings: dialogs.aiSettings,
    onAdopt,
  });
}

/** Cards and AI proposals stage explicit requests; only the local backend can create canonical evidence. */
function useNativeActions(transport: AnalysisTransport, session: AnalysisSession, navigation: WorkspaceNavigation, native: NativeAnalysis) {
  const checkDraft = async (draft: DraftState): Promise<unknown> => {
    if (!transport.command) throw new Error("Connect the analysis server to check this draft.");
    const result = await transport.command(draftRequest(draft, session.state.evidence));
    if (!result.ok) throw new Error(result.error.message);
    return result.payload;
  };
  const stageDraft = (draft: DraftState): void => {
    native.stage(draftRequest(draft, session.state.evidence));
    native.setKind("example");
    navigation.dispatchDraft({ type: "reset" });
    navigation.setLeftTab("analysis");
  };
  return { checkDraft, stageDraft };
}
