import { act, renderHook } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { useNativeAnalysis } from "./native";
import type { AnalysisSession } from "../analysis/session";
import { parseJson } from "../analysis/json";

describe("native request drafts", () => {
  it("keeps goal and limit edits separate from already validated evidence", async () => {
    const startRequest = vi.fn(async () => true);
    const { result } = renderHook(() => useNativeAnalysis({ startRequest } as unknown as AnalysisSession));
    expect(result.current.sql).toBe("");
    act(() => result.current.editSql("SELECT 1"));
    await act(async () => {
      await result.current.start();
    });
    expect(result.current.dirty).toBe(false);
    act(() => result.current.setKind("example"));
    expect(result.current.dirty).toBe(true);
    await act(async () => {
      await result.current.start();
    });
    act(() => result.current.setMaxInstances(0));
    await act(async () => {
      expect(await result.current.start()).toBe(false);
    });
    expect(startRequest).toHaveBeenCalledTimes(2);
    expect(result.current.error).toContain("1–256");
  });

  it("stages only SQL changes while retaining the exact large integer context", () => {
    const { result } = renderHook(() => useNativeAnalysis({} as AnalysisSession));
    const context = parseJson('{"sql":"SELECT $1","parameters":{"1":{"type":"int64","value":9223372036854775807}}}');
    act(() => result.current.editSql("SELECT $1 AS id", context));
    expect(result.current.sql).toBe("SELECT $1 AS id");
    expect(result.current.text).toContain("9223372036854775807");
    expect(result.current.dirty).toBe(true);
  });
});
