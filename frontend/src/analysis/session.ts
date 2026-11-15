/**
 * Pure transport/session reducer plus its React effect boundary. Next requests
 * are serialized, duplicate clicks are ignored, stale responses are discarded,
 * and a failed next click never discards the cursor.
 */
import { useCallback, useReducer, useRef, type Dispatch } from "react";
import { parseNextPayload, parseOpenPayload, parseStartedOpen, startedIdentity } from "./adapter";
import type {
  AdapterFailure,
  AnalysisTransport,
  EnumerationError,
  EnumerationStatus,
  EvidenceBundleModel,
  EvidenceView,
  FixtureScenario,
  NextStep,
  OpenResult,
  TransportFailure,
  StartCommand,
} from "./types";

/** Normalized session status. */
export type SessionPhase = "idle" | "opening" | "ready" | "failed";

/** All review/transport state needed by the interface. */
export interface SessionState {
  readonly scenarioId: string | null;
  readonly native: boolean;
  readonly phase: SessionPhase;
  readonly generation: number;
  readonly bundle: EvidenceBundleModel | null;
  readonly bundleId: string | null;
  readonly evidence: EvidenceView | null;
  readonly status: EnumerationStatus | null;
  readonly enumerationExhausted: boolean;
  readonly cursor: string | null;
  readonly maxInstances: number | null;
  readonly claim: string | null;
  readonly pendingNext: boolean;
  readonly nextRequestId: number;
  readonly nextFailure: AdapterFailure | EnumerationError | TransportFailure | null;
  readonly openFailure: AdapterFailure | EnumerationError | TransportFailure | null;
  readonly staleNotice: string | null;
  readonly openRaw: unknown;
  readonly nextRaw: unknown;
}

/** Actions accepted by the pure review/session reducer. */
export type SessionAction =
  | { readonly type: "open/request"; readonly generation: number; readonly scenarioId: string; readonly native?: boolean }
  | { readonly type: "open/result"; readonly generation: number; readonly result: OpenResult }
  | {
      readonly type: "open/transport-failure";
      readonly generation: number;
      readonly failure: TransportFailure;
    }
  | { readonly type: "next/request"; readonly requestId: number }
  | {
      readonly type: "next/result";
      readonly requestId: number;
      readonly expectedBundleId: string;
      readonly result: ReturnType<typeof parseNextPayload>;
    }
  | {
      readonly type: "next/transport-failure";
      readonly requestId: number;
      readonly failure: TransportFailure;
    }
  | { readonly type: "notice/clear-stale" };

/** Initial idle session state. */
export const initialSessionState: SessionState = {
  scenarioId: null,
  native: false,
  phase: "idle",
  generation: 0,
  bundle: null,
  bundleId: null,
  evidence: null,
  status: null,
  enumerationExhausted: false,
  cursor: null,
  maxInstances: null,
  claim: null,
  pendingNext: false,
  nextRequestId: 0,
  nextFailure: null,
  openFailure: null,
  staleNotice: null,
  openRaw: null,
  nextRaw: null,
};

/** Clear instance-specific state while preserving request generation discipline. */
function opening(action: Extract<SessionAction, { type: "open/request" }>): SessionState {
  return {
    ...initialSessionState,
    scenarioId: action.scenarioId,
    phase: "opening",
    generation: action.generation,
    native: action.native ?? false,
  };
}

/** Apply a validated open result, including a legitimate terminal record. */
function opened(state: SessionState, result: Extract<OpenResult, { ok: true }>): SessionState {
  return {
    ...state,
    phase: "ready",
    bundle: result.value.bundle,
    bundleId: result.value.bundleId,
    evidence: result.value.evidence,
    status: result.value.status,
    enumerationExhausted: result.value.enumerationExhausted,
    cursor: result.value.cursor,
    maxInstances: result.value.maxInstances,
    claim: result.value.claim,
    openFailure: null,
    openRaw: result.value.raw,
  };
}

/** Apply a failed open without fabricating a bundle or cursor. */
function openFailed(state: SessionState, failure: AdapterFailure | EnumerationError | TransportFailure): SessionState {
  return { ...state, phase: "failed", openFailure: failure, evidence: null };
}

/** Check whether a next action still belongs to the live revision and request. */
function acceptsNext(state: SessionState, requestId: number, expectedBundleId: string): boolean {
  return state.pendingNext && state.nextRequestId === requestId && state.bundleId === expectedBundleId;
}

/** Clear pending state and retain a rejected next response without moving the cursor. */
function rejectedNext(state: SessionState, error: AdapterFailure | EnumerationError): SessionState {
  const stale = error.kind === "contract" && error.code === "contract.stale_revision";
  return {
    ...state,
    pendingNext: false,
    nextFailure: stale ? null : error,
    staleNotice: stale ? error.message : state.staleNotice,
    status: state.status === "instance" ? "instance" : state.status,
  };
}

/** Apply one successful next step, including a terminal response with no instance. */
function appliedNext(state: SessionState, step: NextStep): SessionState {
  return {
    ...state,
    pendingNext: false,
    nextFailure: null,
    evidence: step.evidence ?? state.evidence,
    status: step.status,
    enumerationExhausted: step.enumerationExhausted,
    cursor: step.cursor,
    maxInstances: step.maxInstances ?? state.maxInstances,
    claim: step.claim ?? state.claim,
    nextRaw: step.raw,
  };
}

/** Apply one next response, preserving the cursor on every failure. */
function nextResult(state: SessionState, action: Extract<SessionAction, { type: "next/result" }>): SessionState {
  if (!acceptsNext(state, action.requestId, action.expectedBundleId)) return state;
  if (!action.result.ok) return rejectedNext(state, action.result.error);
  return appliedNext(state, action.result.value);
}

/** Mark one serialized next request without moving the cursor. */
function requestedNext(state: SessionState, action: Extract<SessionAction, { type: "next/request" }>): SessionState {
  if (!state.cursor || state.pendingNext) return state;
  return {
    ...state,
    pendingNext: true,
    nextRequestId: action.requestId,
    nextFailure: null,
    staleNotice: null,
  };
}

/** The pure reducer for review transport state. */
export function sessionReducer(state: SessionState, action: SessionAction): SessionState {
  switch (action.type) {
    case "open/request":
      return action.generation >= state.generation ? opening(action) : state;
    case "open/result":
      if (action.generation !== state.generation) return state;
      return action.result.ok ? opened(state, action.result) : openFailed(state, action.result.error);
    case "open/transport-failure":
      if (action.generation !== state.generation) return state;
      return openFailed(state, action.failure);
    case "next/request":
      return requestedNext(state, action);
    case "next/result":
      return nextResult(state, action);
    case "next/transport-failure":
      if (!acceptsNext(state, action.requestId, state.bundleId ?? "")) return state;
      return { ...state, pendingNext: false, nextFailure: action.failure };
    case "notice/clear-stale":
      return { ...state, staleNotice: null };
    default:
      return state;
  }
}

/** The public hook contract used by the app and tests. */
export interface AnalysisSession {
  readonly state: SessionState;
  readonly openScenario: (scenario: FixtureScenario) => Promise<void>;
  readonly startRequest: (command: StartCommand) => Promise<boolean>;
  readonly requestNext: () => Promise<void>;
  readonly clearStaleNotice: () => void;
}

/** Dispatch a transport or contract open failure without fabricating evidence. */
function dispatchOpenFailure(error: TransportFailure, generation: number, dispatch: Dispatch<SessionAction>): void {
  dispatch({ type: "open/transport-failure", generation, failure: error });
}

/** Fail closed when fixture metadata does not identify a stored bundle. */
function dispatchMissingBundle(generation: number, dispatch: Dispatch<SessionAction>): void {
  dispatch({
    type: "open/result",
    generation,
    result: { ok: false, error: contractMissingBundle() },
  });
}

/** Load and parse one fixture/host open response. */
async function loadScenario(transport: AnalysisTransport, scenario: FixtureScenario, generation: number, dispatch: Dispatch<SessionAction>): Promise<void> {
  if (!scenario.bundleId) return dispatchMissingBundle(generation, dispatch);
  try {
    const transported = await transport.open({
      action: "instances.open",
      bundle_id: scenario.bundleId,
    });
    if (transported.ok) {
      dispatch({
        type: "open/result",
        generation,
        result: parseOpenPayload(transported.payload),
      });
    } else {
      dispatchOpenFailure(transported.error, generation, dispatch);
    }
  } catch (error) {
    dispatchOpenFailure(thrownTransport(error), generation, dispatch);
  }
}

/** Load and parse one cursor-addressed next response. */
async function loadNext(
  transport: AnalysisTransport,
  cursor: string,
  requestId: number,
  expectedBundleId: string,
  bundle: EvidenceBundleModel,
  dispatch: Dispatch<SessionAction>,
): Promise<void> {
  try {
    const transported = await transport.next({ action: "instances.next", cursor });
    if (transported.ok) {
      dispatch({
        type: "next/result",
        requestId,
        expectedBundleId,
        result: parseNextPayload(transported.payload, bundle, expectedBundleId),
      });
    } else {
      dispatch({ type: "next/transport-failure", requestId, failure: transported.error });
    }
  } catch (error) {
    dispatch({ type: "next/transport-failure", requestId, failure: thrownTransport(error) });
  }
}

interface Ref<T> {
  current: T;
}

/** Build the serialized open callback and invalidate older next operations. */
function useOpenScenario(
  transport: AnalysisTransport,
  generation: Ref<number>,
  nextInFlight: Ref<number | null>,
  dispatch: Dispatch<SessionAction>,
): (scenario: FixtureScenario) => Promise<void> {
  return useCallback(
    async (scenario: FixtureScenario): Promise<void> => {
      const current = generation.current + 1;
      generation.current = current;
      nextInFlight.current = null;
      dispatch({ type: "open/request", generation: current, scenarioId: scenario.id });
      await loadScenario(transport, scenario, current, dispatch);
    },
    [transport, generation, nextInFlight, dispatch],
  );
}

/** Build the serialized next callback, preserving cursor state on all failures. */
function useRequestNext(
  transport: AnalysisTransport,
  nextToken: Ref<number>,
  nextInFlight: Ref<number | null>,
  live: Ref<SessionState>,
  dispatch: Dispatch<SessionAction>,
): () => Promise<void> {
  return useCallback(async (): Promise<void> => {
    const current = live.current;
    if (nextInFlight.current !== null || !current.cursor || !current.bundle || !current.bundleId) return;
    const requestId = nextToken.current + 1;
    nextInFlight.current = requestId;
    nextToken.current = requestId;
    dispatch({ type: "next/request", requestId });
    try {
      await loadNext(transport, current.cursor, requestId, current.bundleId, current.bundle, dispatch);
    } finally {
      if (nextInFlight.current === requestId) nextInFlight.current = null;
    }
  }, [transport, nextToken, nextInFlight, live, dispatch]);
}

/** Hook the pure reducer to the injectable transport without auto-draining. */
export function useAnalysisSession(transport: AnalysisTransport): AnalysisSession {
  const [state, dispatch] = useReducer(sessionReducer, initialSessionState);
  const generation = useRef(0);
  const nextToken = useRef(0);
  const nextInFlight = useRef<number | null>(null);
  const live = useRef(state);
  live.current = state;
  const openScenario = useOpenScenario(transport, generation, nextInFlight, dispatch);
  const requestNext = useRequestNext(transport, nextToken, nextInFlight, live, dispatch);
  const startRequest = useStartRequest(transport, generation, nextInFlight, live, dispatch);
  const clearStaleNotice = useCallback(() => dispatch({ type: "notice/clear-stale" }), [dispatch]);
  return { state, openScenario, startRequest, requestNext, clearStaleNotice };
}

/** Start explicitly, then reopen the persisted revision so replay and bundle validation remain mandatory. */
async function loadStart(transport: AnalysisTransport, command: StartCommand, generation: number, dispatch: Dispatch<SessionAction>): Promise<boolean> {
  try {
    const started = await transport.start(command);
    if (!started.ok) {
      dispatchOpenFailure(started.error, generation, dispatch);
      return false;
    }
    const identity = startedIdentity(started.payload);
    const opened = await transport.open({ action: "instances.open", bundle_id: identity.bundleId });
    if (!opened.ok) {
      dispatchOpenFailure(opened.error, generation, dispatch);
      return false;
    }
    const result = parseStartedOpen(opened.payload, identity);
    dispatch({ type: "open/result", generation, result });
    return result.ok;
  } catch (error) {
    dispatchOpenFailure(thrownTransport(error), generation, dispatch);
    return false;
  }
}

/** A new native start invalidates old requests without letting their completion release newer locks. */
function useStartRequest(
  transport: AnalysisTransport,
  generation: Ref<number>,
  nextInFlight: Ref<number | null>,
  live: Ref<SessionState>,
  dispatch: Dispatch<SessionAction>,
): AnalysisSession["startRequest"] {
  return useCallback(
    async (command: StartCommand): Promise<boolean> => {
      const current = ++generation.current;
      nextInFlight.current = null;
      dispatch({ type: "open/request", generation: current, scenarioId: live.current.scenarioId ?? "native", native: true });
      const success = await loadStart(transport, command, current, dispatch);
      return success && generation.current === current;
    },
    [transport, generation, nextInFlight, live, dispatch],
  );
}

/** Build a fail-closed contract error when fixture metadata lacks a bundle. */
function contractMissingBundle(): AdapterFailure {
  return {
    kind: "contract",
    code: "contract.missing_context",
    message: "This scenario does not identify an opened evidence bundle.",
    raw: null,
  };
}

/** Wrap unexpected host-backed promises as transport failures, not solver errors. */
function thrownTransport(error: unknown): TransportFailure {
  return {
    kind: "transport",
    code: "transport.exception",
    message: error instanceof Error ? error.message : "The analysis transport failed unexpectedly.",
    retryable: true,
  };
}
