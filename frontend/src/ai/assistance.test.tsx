/** Synthetic provider tests never contact a network service or use a real credential. */
import { beforeEach, describe, expect, it, vi } from "vitest";
import { act, fireEvent, render, renderHook, screen } from "@testing-library/react";
import { parseOpenPayload } from "../analysis/adapter";
import { scenarioById } from "../analysis/fixtures";
import { hostJson } from "../analysis/host";
import { AI_MODEL } from "./config";
import { LiveAssistancePanel } from "./LiveAssistancePanel";
import { requestAssistance } from "./provider";
import { defaultAiSettings } from "./summary";
import { validateAssistance } from "./validation";
import { useAssistance } from "./useAssistance";

vi.mock("../analysis/host", () => ({ hostJson: vi.fn() }));
const host = vi.mocked(hostJson);
const bundleId = "b".repeat(64);
const settings = { ...defaultAiSettings, mode: "openai_compatible" as const, apiKey: "synthetic-test-token", redactValues: false };

/** Obtain immutable evidence from a committed fixture, never from provider prose. */
function evidence() {
  const parsed = parseOpenPayload(scenarioById("tenant-boundary")!.openPayload);
  if (!parsed.ok || !parsed.value.evidence) throw new Error("Missing test fixture");
  return parsed.value.evidence;
}

/** Produce one strict, evidence-bound fake provider envelope. */
function response() {
  const current = evidence();
  const identity = { instance_id: current.instance.instanceId, model_revision_sha256: current.instance.modelRevisionSha256, context_sha256: "c".repeat(64) };
  return {
    schema: "sqlolla.ai.explanation.v1",
    status: "ok",
    provider: "openai",
    model: AI_MODEL,
    authority: "untrusted_assistance",
    bundle_id: bundleId,
    redact_values: false,
    ...identity,
    document: {
      ...identity,
      title: "<img src=x onerror=alert(1)>",
      claims: [{ text: "An output exists.", fact_references: [current.facts[0]!.reference] }],
      assumptions: ["Finite declared bounds only."],
      sql: null,
    },
  };
}

beforeEach(() => {
  host.mockReset();
});

describe("explicit fixed-model assistance", () => {
  it("retains only an opaque settings identity in results and hides results after disconnect", async () => {
    host.mockResolvedValue(response());
    const current = evidence();
    const hook = renderHook(({ config }) => useAssistance(bundleId, current, config), { initialProps: { config: settings } });
    await act(async () => hook.result.current.run("Explain rows"));
    expect(hook.result.current.state?.result).toBeDefined();
    expect(JSON.stringify(hook.result.current.state)).not.toContain(settings.apiKey);
    hook.rerender({ config: { ...settings, apiKey: "" } });
    expect(hook.result.current.state).toBeNull();
  });

  it("sends canonical IDs and a request header, never a token or frontend facts in the body", async () => {
    host.mockResolvedValue(response());
    await requestAssistance(bundleId, evidence(), settings, "Explain rows", new AbortController().signal);
    const [path, body, options] = host.mock.calls[0]!;
    expect(path).toBe("/api/ai/explain");
    expect(body).toEqual({
      bundle_id: bundleId,
      instance_id: evidence().instance.instanceId,
      model_revision_sha256: evidence().instance.modelRevisionSha256,
      redact_values: false,
      question: "Explain rows",
    });
    expect(JSON.stringify(body)).not.toContain(settings.apiKey);
    expect(options?.headers?.Authorization).toBe(`Bearer ${settings.apiKey}`);
  });

  it("refuses other models and unconnected providers before transport", async () => {
    for (const changed of [
      { ...settings, model: "other" },
      { ...settings, mode: "gemini_compatible" as const },
    ]) {
      await expect(requestAssistance(bundleId, evidence(), changed, "Explain rows", new AbortController().signal)).rejects.toThrow();
    }
    expect(host).not.toHaveBeenCalled();
  });

  it("rejects stale identity and invented citations", () => {
    const changed = response();
    changed.document.claims[0]!.fact_references = ["/invented"];
    expect(() => validateAssistance(changed, bundleId, evidence(), false)).toThrow();
    expect(() => validateAssistance({ ...response(), model: "fallback" }, bundleId, evidence(), false)).toThrow();
    expect(() => validateAssistance({ ...response(), bundle_id: "d".repeat(64) }, bundleId, evidence(), false)).toThrow();
  });

  it("requires an explicit click and renders generated HTML as inert plain text", async () => {
    host.mockResolvedValue(response());
    const inspect = vi.fn();
    const current = evidence();
    const view = render(<LiveAssistancePanel bundleId={bundleId} evidence={current} settings={settings} onInspectFact={inspect} />);
    expect(host).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Explain this case" }));
    await screen.findByText("<img src=x onerror=alert(1)>");
    expect(view.container.querySelector("img")).toBeNull();
    const citation = screen.getByRole("button", { name: "Input row 1" });
    expect(citation).toHaveAttribute("title", current.facts[0]!.reference);
    fireEvent.click(citation);
    expect(inspect).toHaveBeenCalledWith(current.facts[0]!.reference);
  });

  it("discards an in-flight response when the canonical bundle changes", async () => {
    let resolve!: (value: unknown) => void;
    host.mockImplementation(
      () =>
        new Promise((done) => {
          resolve = done;
        }),
    );
    const current = evidence();
    const view = render(<LiveAssistancePanel bundleId={bundleId} evidence={current} settings={settings} onInspectFact={vi.fn()} />);
    fireEvent.click(screen.getByRole("button", { name: "Explain this case" }));
    view.rerender(<LiveAssistancePanel bundleId={"d".repeat(64)} evidence={current} settings={settings} onInspectFact={vi.fn()} />);
    await act(async () => resolve(response()));
    expect(screen.queryByText("<img src=x onerror=alert(1)>")).toBeNull();
  });

  it("rejects SQL in an explanation and offers no unconstrained proposal action", () => {
    const changed = response();
    expect(() => validateAssistance({ ...changed, document: { ...changed.document, sql: "SELECT 1" } }, bundleId, evidence(), false)).toThrow();
    render(<LiveAssistancePanel bundleId={bundleId} evidence={evidence()} settings={settings} onInspectFact={vi.fn()} />);
    expect(screen.queryByRole("button", { name: "Propose SQL" })).toBeNull();
    expect(host).not.toHaveBeenCalled();
  });
});
