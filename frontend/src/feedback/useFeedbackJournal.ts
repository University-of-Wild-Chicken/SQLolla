/**
 * Browser-local feedback journal hook. It stores only feedback events and keeps
 * credentials, provider configuration and evidence outside local storage.
 */
import { useCallback, useEffect, useMemo, useReducer, useRef, type Dispatch, type RefObject } from "react";
import {
  activeDecisionFor,
  exportJournal,
  mergeJournal,
  parseJournal,
  proposeDecision,
  proposeRelabel,
  proposeWithdrawals,
  readStoredJournal,
  writeStoredJournal,
  type FeedbackChange,
  type FeedbackEvent,
  type FeedbackTarget,
  type ImportResult,
  type JournalStorage,
  type ReviewDisposition,
} from "./journal";

interface JournalHookState {
  readonly events: readonly FeedbackEvent[];
  readonly storageError: string | null;
  readonly notice: string | null;
}

type JournalHookAction =
  | {
      readonly type: "events/replace";
      readonly events: readonly FeedbackEvent[];
      readonly notice: string | null;
    }
  | { readonly type: "notice"; readonly message: string | null }
  | { readonly type: "storage-error"; readonly message: string };

interface ResolvedStorage {
  readonly storage: JournalStorage;
  readonly error: string | null;
}

type LiveEvents = RefObject<readonly FeedbackEvent[]>;

/** In-memory fallback used only when browser storage is unavailable. */
function memoryStorage(): JournalStorage {
  const values = new Map<string, string>();
  return {
    getItem: (key) => values.get(key) ?? null,
    setItem: (key, value) => void values.set(key, value),
  };
}

/** Resolve localStorage without leaking an exception or placing content in a URL. */
function resolveStorage(override: JournalStorage | undefined): ResolvedStorage {
  if (override) return { storage: override, error: null };
  try {
    if (typeof window === "undefined" || !window.localStorage) {
      return {
        storage: memoryStorage(),
        error: "Browser storage is unavailable; this journal is in memory only.",
      };
    }
    return { storage: window.localStorage, error: null };
  } catch {
    return {
      storage: memoryStorage(),
      error: "Browser storage is unavailable; this journal is in memory only.",
    };
  }
}

/** Reducer for journal events and non-destructive persistence notices. */
function journalReducer(state: JournalHookState, action: JournalHookAction): JournalHookState {
  switch (action.type) {
    case "events/replace":
      return { ...state, events: action.events, notice: action.notice };
    case "notice":
      return { ...state, notice: action.message };
    case "storage-error":
      return { ...state, storageError: action.message };
    default:
      return state;
  }
}

/** Initial journal state loaded from browser storage with failures kept explicit. */
function initJournal(resolved: ResolvedStorage): JournalHookState {
  const loaded = readStoredJournal(resolved.storage);
  return { events: loaded.events, storageError: resolved.error ?? loaded.error, notice: null };
}

/** Persist the journal after changes unless its store is already known corrupt. */
function useJournalPersistence(state: JournalHookState, storage: JournalStorage, dispatch: Dispatch<JournalHookAction>): void {
  useEffect(() => {
    if (state.storageError) return;
    const error = writeStoredJournal(storage, state.events);
    if (error) dispatch({ type: "storage-error", message: error });
  }, [state.events, state.storageError, storage, dispatch]);
}

/** Dispatch a proposed journal change or its no-op/conflict notice. */
function applyChange(dispatch: Dispatch<JournalHookAction>, change: FeedbackChange, message: string): void {
  if (change.kind === "appended") {
    dispatch({ type: "events/replace", events: change.events, notice: message });
  } else {
    dispatch({ type: "notice", message: change.reason });
  }
}

/** Plain-language confirmation for a journaled decision. */
function decisionMessage(disposition: ReviewDisposition): string {
  if (disposition === "unsure") return "Case moved to Unsure.";
  return disposition === "accepted_use_case"
    ? "Use case preserved. This is not a safety proof."
    : "Side behavior recorded as an exclusion requirement for a future revision. Nothing was fixed.";
}

/** Create the append-only decision and undo callbacks. */
function useDecisionActions(events: LiveEvents, dispatch: Dispatch<JournalHookAction>): Pick<FeedbackJournal, "recordDecision" | "undo" | "relabel"> {
  return {
    recordDecision: useRecordDecision(events, dispatch),
    undo: useUndoDecision(events, dispatch),
    relabel: useCallback(
      (id, disposition) => applyChange(dispatch, proposeRelabel(events.current, id, disposition), decisionMessage(disposition)),
      [events, dispatch],
    ),
  };
}

/** Build the review-decision callback. */
function useRecordDecision(
  events: LiveEvents,
  dispatch: Dispatch<JournalHookAction>,
): (target: FeedbackTarget, disposition: ReviewDisposition, rationale: string) => void {
  return useCallback(
    (target, disposition, rationale): void => {
      applyChange(dispatch, proposeDecision(events.current, target, disposition, rationale), decisionMessage(disposition));
    },
    [events, dispatch],
  );
}

/** Build the superseding-undo callback. */
function useUndoDecision(events: LiveEvents, dispatch: Dispatch<JournalHookAction>): (target: FeedbackTarget) => void {
  return useCallback(
    (target): void => {
      const active = activeDecisionFor(events.current, target);
      if (!active) {
        dispatch({ type: "notice", message: "There is no active label to undo for this case." });
        return;
      }
      applyChange(dispatch, proposeWithdrawals(events.current, [active.event_id]), "Case label withdrawn.");
    },
    [events, dispatch],
  );
}

/** Create the validated journal import callback. */
function useImportText(events: LiveEvents, dispatch: Dispatch<JournalHookAction>): (text: string) => ImportResult {
  return useCallback(
    (text: string): ImportResult => {
      try {
        const merged = mergeJournal(events.current, parseJournal(text));
        if (merged.ok) {
          dispatch({
            type: "events/replace",
            events: merged.events,
            notice: `Imported ${merged.added} event(s); ${merged.duplicates} duplicate(s) ignored.`,
          });
        }
        return merged;
      } catch (error) {
        return {
          ok: false,
          message: error instanceof Error ? error.message : "Journal could not be imported.",
        };
      }
    },
    [events, dispatch],
  );
}

/** Create conflict withdrawals and import/export callbacks. */
function useJournalTransfer(
  events: LiveEvents,
  dispatch: Dispatch<JournalHookAction>,
): Pick<FeedbackJournal, "withdraw" | "importText" | "exportText" | "clearNotice"> {
  const withdraw = useCallback(
    (eventIds: readonly string[]): void => {
      applyChange(dispatch, proposeWithdrawals(events.current, eventIds), "Case label withdrawn.");
    },
    [events, dispatch],
  );
  const importText = useImportText(events, dispatch);
  const exportText = useCallback(() => exportJournal(events.current), [events]);
  const clearNotice = useCallback(() => dispatch({ type: "notice", message: null }), [dispatch]);
  return { withdraw, importText, exportText, clearNotice };
}

/** Public API returned by the feedback journal hook. */
export interface FeedbackJournal {
  readonly events: readonly FeedbackEvent[];
  readonly storageError: string | null;
  readonly notice: string | null;
  readonly recordDecision: (target: FeedbackTarget, disposition: ReviewDisposition, rationale: string) => void;
  readonly relabel: (eventId: string, disposition: ReviewDisposition) => void;
  readonly undo: (target: FeedbackTarget) => void;
  readonly withdraw: (eventIds: readonly string[]) => void;
  readonly importText: (text: string) => ImportResult;
  readonly exportText: () => string;
  readonly clearNotice: () => void;
}

/** Serialize synchronous mutations before React publishes the next journal render. */
function useJournalState(resolved: ResolvedStorage) {
  const [state, update] = useReducer(journalReducer, resolved, initJournal);
  const events = useRef(state.events);
  const dispatch = useCallback((action: JournalHookAction) => {
    if (action.type === "events/replace") events.current = action.events;
    update(action);
  }, []);
  return { state, events, dispatch };
}

/** Append-only feedback journal backed by browser storage. */
export function useFeedbackJournal(storageOverride?: JournalStorage): FeedbackJournal {
  const resolved = useMemo(() => resolveStorage(storageOverride), [storageOverride]);
  const { state, events, dispatch } = useJournalState(resolved);
  useJournalPersistence(state, resolved.storage, dispatch);
  const decisions = useDecisionActions(events, dispatch);
  const transfer = useJournalTransfer(events, dispatch);
  return { ...state, ...decisions, ...transfer };
}
