/** Synthetic provider tests never contact a network service or use a real credential. */
import { beforeEach, describe, expect, it, vi } from "vitest";
import { act, fireEvent, render, renderHook, screen, waitFor } from "@testing-library/react";
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
function response(kind: "explain" | "propose" = "explain") {
  const current = evidence();
  const identity = { instance_id: current.instance.instanceId, model_revision_sha256: current.instance.modelRevisionSha256, context_sha256: "c".repeat(64) };
  return {
    schema: kind === "explain" ? "sqlolla.ai.explanation.v1" : "sqlolla.ai.proposal.v1",
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
      sql: kind === "propose" ? "SELECT 1" : null,
    },
    ...(kind === "propose" ? { validation: { status: "ok", analysis: "COUNTEREXAMPLE" } } : {}),
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
    await act(async () => hook.result.current.run("explain", "Explain rows"));
    expect(hook.result.current.state?.result).toBeDefined();
    expect(JSON.stringify(hook.result.current.state)).not.toContain(settings.apiKey);
    hook.rerender({ config: { ...settings, apiKey: "" } });
    expect(hook.result.current.state).toBeNull();
  });

  it("sends canonical IDs and a request header, never a token or frontend facts in the body", async () => {
    host.mockResolvedValue(response());
    await requestAssistance("explain", bundleId, evidence(), settings, "Explain rows", new AbortController().signal);
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

  it("refuses other models, unconnected providers and redacted proposals before transport", async () => {
    for (const changed of [
      { ...settings, model: "other" },
      { ...settings, mode: "gemini_compatible" as const },
      { ...settings, redactValues: true },
    ]) {
      await expect(requestAssistance("propose", bundleId, evidence(), changed, "Change query", new AbortController().signal)).rejects.toThrow();
    }
    expect(host).not.toHaveBeenCalled();
  });

  it("rejects stale identity and invented citations", () => {
    const changed = response();
    changed.document.claims[0]!.fact_references = ["/invented"];
    expect(() => validateAssistance(changed, "explain", bundleId, evidence(), false)).toThrow();
    expect(() => validateAssistance({ ...response(), model: "fallback" }, "explain", bundleId, evidence(), false)).toThrow();
    expect(() => validateAssistance({ ...response(), bundle_id: "d".repeat(64) }, "explain", bundleId, evidence(), false)).toThrow();
  });

  it("requires an explicit click and renders generated HTML as inert plain text", async () => {
    host.mockResolvedValue(response());
    const view = render(<LiveAssistancePanel bundleId={bundleId} evidence={evidence()} settings={settings} onInspectFact={vi.fn()} />);
    expect(host).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Explain this case" }));
    await screen.findByText("<img src=x onerror=alert(1)>");
    expect(view.container.querySelector("img")).toBeNull();
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

  it("uses the original bounded result and applies SQL only after explicit draft acceptance", async () => {
    host.mockResolvedValue(response("propose"));
    const apply = vi.fn();
    render(<LiveAssistancePanel bundleId={bundleId} evidence={evidence()} settings={settings} onInspectFact={vi.fn()} onApplyProposal={apply} />);
    fireEvent.change(screen.getByLabelText("Question or proposed change"), { target: { value: "Add tenant filtering" } });
    fireEvent.click(screen.getByRole("button", { name: "Propose SQL" }));
    await screen.findByText("The retained bounded property still has a counterexample.");
    expect(apply).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Use as SQL draft" }));
    await waitFor(() => expect(apply).toHaveBeenCalledWith("SELECT 1"));
  });
});
