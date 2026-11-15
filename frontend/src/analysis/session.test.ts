import { describe, expect, it } from "vitest";
import { parseNextPayload, parseOpenPayload } from "./adapter";
import { scenarioById } from "./fixtures";
import { initialSessionState, sessionReducer, type SessionState } from "./session";
import type { OpenedAnalysis } from "./types";

function openedState(): { state: SessionState; opened: OpenedAnalysis } {
  const scenario = scenarioById("tenant-boundary")!;
  const parsed = parseOpenPayload(scenario.openPayload);
  if (!parsed.ok) throw new Error(parsed.error.message);
  let state = sessionReducer(initialSessionState, {
    type: "open/request",
    generation: 1,
    scenarioId: scenario.id,
  });
  state = sessionReducer(state, { type: "open/result", generation: 1, result: parsed });
  return { state, opened: parsed.value };
}

describe("review/session reducer", () => {
  it("serializes next requests and retains the committed cursor while pending", () => {
    const { state, opened } = openedState();
    expect(state.phase).toBe("ready");
    expect(state.cursor).toBe(opened.cursor);
    const pending = sessionReducer(state, { type: "next/request", requestId: 1 });
    expect(pending.pendingNext).toBe(true);
    const duplicate = sessionReducer(pending, { type: "next/request", requestId: 2 });
    expect(duplicate).toBe(pending);
    expect(duplicate.cursor).toBe(opened.cursor);
  });

  it("preserves cursor and current evidence on a failed next step", () => {
    const { state, opened } = openedState();
    const initialEvidence = state.evidence;
    const pending = sessionReducer(state, { type: "next/request", requestId: 1 });
    const actual = sessionReducer(pending, {
      type: "next/transport-failure",
      requestId: 1,
      failure: {
        kind: "transport",
        code: "transport.timeout",
        message: "No response",
        retryable: true,
      },
    });
    expect(actual.cursor).toBe(opened.cursor);
    expect(actual.evidence).toBe(initialEvidence);
    expect(actual.pendingNext).toBe(false);
    expect(actual.nextFailure?.code).toBe("transport.timeout");
    const retry = sessionReducer(actual, { type: "next/request", requestId: 2 });
    expect(retry.pendingNext).toBe(true);
    expect(retry.cursor).toBe(opened.cursor);
  });

  it("discards responses from stale request ids and stale model revisions", () => {
    const { state, opened } = openedState();
    const pending = sessionReducer(state, { type: "next/request", requestId: 4 });
    const terminalRaw = scenarioById("tenant-boundary")!.nextByCursor[opened.cursor!]!;
    const terminal = parseNextPayload(terminalRaw, opened.bundle, opened.bundleId);
    const staleId = sessionReducer(pending, {
      type: "next/result",
      requestId: 3,
      expectedBundleId: opened.bundleId,
      result: terminal,
    });
    expect(staleId).toBe(pending);
    const staleError = {
      ok: false as const,
      error: {
        kind: "contract" as const,
        code: "contract.stale_revision",
        message: "different revision",
        raw: null,
      },
    };
    const afterStale = sessionReducer(pending, {
      type: "next/result",
      requestId: 4,
      expectedBundleId: opened.bundleId,
      result: staleError,
    });
    expect(afterStale.cursor).toBe(opened.cursor);
    expect(afterStale.staleNotice).toBe("different revision");
    expect(afterStale.evidence).toBe(opened.evidence);
  });

  it("keeps the last stored evidence visible when the next response is terminal", () => {
    const { state, opened } = openedState();
    const scenario = scenarioById("tenant-boundary")!;
    const next = parseNextPayload(scenario.nextByCursor[opened.cursor!]!, opened.bundle, opened.bundleId);
    if (!next.ok) throw new Error(next.error.message);
    const pending = sessionReducer(state, { type: "next/request", requestId: 1 });
    const advanced = sessionReducer(pending, {
      type: "next/result",
      requestId: 1,
      expectedBundleId: opened.bundleId,
      result: next,
    });
    expect(advanced.evidence?.instance.instanceId).not.toBe(opened.evidence?.instance.instanceId);
    expect(advanced.cursor).toBe(next.value.cursor);
    const terminal = parseNextPayload(scenario.nextByCursor[next.value.cursor!]!, opened.bundle, opened.bundleId);
    if (!terminal.ok) throw new Error(terminal.error.message);
    const pendingTerminal = sessionReducer(advanced, { type: "next/request", requestId: 2 });
    const ended = sessionReducer(pendingTerminal, {
      type: "next/result",
      requestId: 2,
      expectedBundleId: opened.bundleId,
      result: terminal,
    });
    expect(ended.status).toBe("exhausted");
    expect(ended.cursor).toBeNull();
    expect(ended.evidence).toBe(advanced.evidence);
    expect(ended.enumerationExhausted).toBe(true);
  });

  it("ignores an older open generation and exposes an open failure as a failure state", () => {
    let state = sessionReducer(initialSessionState, {
      type: "open/request",
      generation: 2,
      scenarioId: "new",
    });
    const old = parseOpenPayload(scenarioById("tenant-boundary")!.openPayload);
    if (!old.ok) throw new Error("fixture");
    state = sessionReducer(state, { type: "open/result", generation: 1, result: old });
    expect(state.phase).toBe("opening");
    state = sessionReducer(state, { type: "open/request", generation: 3, scenarioId: "newer" });
    state = sessionReducer(state, {
      type: "open/transport-failure",
      generation: 3,
      failure: { kind: "transport", code: "transport.gap", message: "No host", retryable: false },
    });
    expect(state.phase).toBe("failed");
    expect(state.openFailure?.code).toBe("transport.gap");
  });
});
