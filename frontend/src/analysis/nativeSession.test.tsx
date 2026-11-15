import { act, renderHook } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { scenarioById } from "./fixtures";
import { useAnalysisSession } from "./session";
import { createFixtureTransport } from "./transports";
import type { AnalysisTransport, StartCommand, TransportResult } from "./types";

const scenario = scenarioById("tenant-boundary")!;
const command: StartCommand = { action: "instances.start", request: {}, kind: "example", max_instances: 8 };

/** Construct a manually released transport operation for race tests. */
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}

/** Keep fixtures as explicit test data while exercising the native start sequence. */
function transport(overrides: Partial<AnalysisTransport> = {}): AnalysisTransport {
  return { ...createFixtureTransport(), start: vi.fn(async () => ({ ok: true as const, payload: scenario.openPayload })), ...overrides };
}

describe("native analysis session", () => {
  it("requires start then canonical open, and never drains next automatically", async () => {
    const open = vi.fn(createFixtureTransport().open);
    const next = vi.fn(createFixtureTransport().next);
    const source = transport({ open, next });
    const { result } = renderHook(() => useAnalysisSession(source));
    await act(async () => {
      expect(await result.current.startRequest(command)).toBe(true);
    });
    expect(source.start).toHaveBeenCalledWith(command);
    expect(open).toHaveBeenCalledExactlyOnceWith({ action: "instances.open", bundle_id: scenario.bundleId });
    expect(next).not.toHaveBeenCalled();
    expect(result.current.state.native).toBe(true);
    expect(result.current.state.phase).toBe("ready");
    expect(result.current.state.evidence).not.toBeNull();
  });

  it("refuses a mismatched reopen and exposes no evidence", async () => {
    const other = scenarioById("inventory-update")!;
    const source = transport({ open: async () => ({ ok: true, payload: other.openPayload }) });
    const { result } = renderHook(() => useAnalysisSession(source));
    await act(async () => {
      expect(await result.current.startRequest(command)).toBe(false);
    });
    expect(result.current.state.openFailure?.code).toBe("contract.stale_revision");
    expect(result.current.state.evidence).toBeNull();
  });

  it("ignores late starts after a different case has been selected", async () => {
    const delayed = deferred<TransportResult>();
    const source = transport({ start: () => delayed.promise });
    const { result } = renderHook(() => useAnalysisSession(source));
    let pending!: Promise<boolean>;
    act(() => {
      pending = result.current.startRequest(command);
    });
    const other = scenarioById("inventory-update")!;
    await act(async () => {
      await result.current.openScenario(other);
    });
    await act(async () => {
      delayed.resolve({ ok: true, payload: scenario.openPayload });
      expect(await pending).toBe(false);
    });
    expect(result.current.state.bundleId).toBe(other.bundleId);
    expect(result.current.state.native).toBe(false);
  });

  it("does not let an older next completion unlock a newer pending next", async () => {
    const first = deferred<TransportResult>();
    const second = deferred<TransportResult>();
    const next = vi.fn().mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise);
    const source = transport({ next });
    const { result } = renderHook(() => useAnalysisSession(source));
    await act(async () => {
      await result.current.openScenario(scenario);
    });
    let one!: Promise<void>;
    let two!: Promise<void>;
    act(() => {
      one = result.current.requestNext();
    });
    await act(async () => {
      await result.current.openScenario(scenario);
    });
    act(() => {
      two = result.current.requestNext();
    });
    const failure: TransportResult = { ok: false, error: { kind: "transport", code: "test", message: "offline", retryable: true } };
    await act(async () => {
      first.resolve(failure);
      await one;
    });
    await act(async () => {
      await result.current.requestNext();
    });
    expect(next).toHaveBeenCalledTimes(2);
    await act(async () => {
      second.resolve(failure);
      await two;
    });
  });
});
