/** Model selection tests use synthetic host replies only; no provider or credential is contacted. */
import { beforeEach, describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/react";
import { parseOpenPayload } from "../analysis/adapter";
import { scenarioById } from "../analysis/fixtures";
import { hostJson } from "../analysis/host";
import { AiSettingsDialog } from "../components/AiSettingsDialog";
import { aiStatusText } from "../components/AiAssistant";
import { admitsModel, byot, liveTarget, providers } from "./config";
import { listModels } from "./models";
import { requestAssistance } from "./provider";
import { defaultAiSettings, type AiProviderSettings } from "./summary";

vi.mock("../analysis/host", () => ({ hostJson: vi.fn() }));
const host = vi.mocked(hostJson);
const bundleId = "b".repeat(64);
const anthropic: AiProviderSettings = { ...defaultAiSettings, mode: "anthropic_compatible", apiKey: "synthetic-test-token", redactValues: false };

/** Obtain immutable evidence from a committed fixture. */
function evidence() {
  const parsed = parseOpenPayload(scenarioById("tenant-boundary")!.openPayload);
  if (!parsed.ok || !parsed.value.evidence) throw new Error("Missing test fixture");
  return parsed.value.evidence;
}

/** A strict explanation envelope reporting the given provider and model. */
function explanation(model: string) {
  const current = evidence();
  const identity = { instance_id: current.instance.instanceId, model_revision_sha256: current.instance.modelRevisionSha256, context_sha256: "c".repeat(64) };
  return {
    schema: "sqlolla.ai.explanation.v1",
    status: "ok",
    provider: "anthropic",
    model,
    authority: "untrusted_assistance",
    bundle_id: bundleId,
    redact_values: false,
    ...identity,
    document: { ...identity, title: "Why", claims: [{ text: "A row exists.", fact_references: [current.facts[0]!.reference] }], assumptions: [], sql: null },
  };
}

/** A synthetic model listing as the host would return it. */
function listing(models: readonly string[], provider = "anthropic") {
  return { schema: "sqlolla.ai.models.v1", status: "ok", provider, recommended: providers.anthropic_compatible.model, models };
}

beforeEach(() => {
  host.mockReset();
});

describe("explicit model choice", () => {
  it("defaults to each provider's tested model and sends the choice as a header", async () => {
    expect(byot(anthropic).headers["X-SQLolla-Model"]).toBe("claude-sonnet-5-5");
    const chosen = { ...anthropic, model: "claude-opus-5-5" };
    host.mockResolvedValue(explanation("claude-opus-5-5"));
    await requestAssistance(bundleId, evidence(), chosen, "Why?", new AbortController().signal);
    const [, , options] = host.mock.calls[0]!;
    expect(options?.headers).toMatchObject({ "X-SQLolla-Provider": "anthropic", "X-SQLolla-Model": "claude-opus-5-5" });
    expect(liveTarget(chosen)).toEqual({ id: "anthropic", label: "Anthropic", model: "claude-opus-5-5" });
  });

  it("rejects a reply from any model other than the one selected", async () => {
    host.mockResolvedValue(explanation("claude-sonnet-5-5"));
    const chosen = { ...anthropic, model: "claude-opus-5-5" };
    await expect(requestAssistance(bundleId, evidence(), chosen, "Why?", new AbortController().signal)).rejects.toThrow();
  });

  it("refuses foreign-family or malformed models before any request", async () => {
    expect(admitsModel("anthropic_compatible", "gpt-6.1-sol")).toBe(false);
    expect(admitsModel("gemini_compatible", "../models/x")).toBe(false);
    expect(admitsModel("openai_compatible", "o4-mini")).toBe(true);
    const foreign = { ...anthropic, model: "gpt-6.1-sol" };
    await expect(requestAssistance(bundleId, evidence(), foreign, "Why?", new AbortController().signal)).rejects.toThrow(/model/);
    expect(host).not.toHaveBeenCalled();
  });

  it("names the provider and model in status text without the key", () => {
    expect(aiStatusText(defaultAiSettings)).toBe("AI: not connected");
    const text = aiStatusText({ ...anthropic, model: "claude-haiku-5-5" });
    expect(text).toBe("AI: Anthropic · claude-haiku-5-5");
    expect(text).not.toContain(anthropic.apiKey);
  });
});

describe("live model listing", () => {
  it("sends only routing and the key, then validates the provider's list", async () => {
    host.mockResolvedValue(listing(["claude-haiku-5-5", "claude-opus-5-5"]));
    const catalog = await listModels(anthropic, new AbortController().signal);
    const [path, body, options] = host.mock.calls[0]!;
    expect(path).toBe("/api/ai/byot/models");
    expect(body).toEqual({});
    expect(options?.headers).toMatchObject({ Authorization: "Bearer synthetic-test-token", "X-SQLolla-Provider": "anthropic" });
    expect(catalog.models).toEqual(["claude-haiku-5-5", "claude-opus-5-5"]);
  });

  it("rejects listings for another provider or with unroutable names", async () => {
    host.mockResolvedValueOnce(listing(["gemini-4-hydrogen"], "gemini")).mockResolvedValueOnce(listing(["claude-ok", "Claude Evil"]));
    await expect(listModels(anthropic, new AbortController().signal)).rejects.toThrow();
    await expect(listModels(anthropic, new AbortController().signal)).rejects.toThrow(/unsupported/);
    await expect(listModels(defaultAiSettings, new AbortController().signal)).rejects.toThrow();
  });
});

describe("AI settings dialog", () => {
  /** Render the dialog with a spy for the saved settings. */
  function open(settings: AiProviderSettings = defaultAiSettings) {
    const onSave = vi.fn();
    render(<AiSettingsDialog open onClose={() => undefined} settings={settings} evidence={null} onSave={onSave} onDisconnect={() => undefined} />);
    return onSave;
  }

  it("lists live models on request and saves the explicit choice", async () => {
    const onSave = open();
    fireEvent.click(screen.getByRole("radio", { name: "Anthropic" }));
    expect(screen.getByLabelText("Model")).toHaveValue("claude-sonnet-5-5");
    expect(screen.getByRole("button", { name: "Check models for this key" })).toBeDisabled();
    fireEvent.change(screen.getByLabelText("API key (memory only)"), { target: { value: "synthetic-test-token" } });
    host.mockResolvedValue(listing(["claude-haiku-5-5", "claude-opus-5-5", "claude-sonnet-5-5"]));
    fireEvent.click(screen.getByRole("button", { name: "Check models for this key" }));
    expect(await screen.findByText(/3 text models available to this key/)).toBeInTheDocument();
    fireEvent.change(screen.getByLabelText("Model"), { target: { value: "claude-opus-5-5" } });
    fireEvent.click(screen.getByRole("button", { name: "Save in this tab · no send" }));
    expect(onSave).toHaveBeenCalledWith(expect.objectContaining({ mode: "anthropic_compatible", model: "claude-opus-5-5" }));
  });

  it("returns to the tested default and clears the key when the provider changes", () => {
    const onSave = open({ ...anthropic, model: "claude-opus-5-5" });
    expect(screen.getByLabelText("Model")).toHaveValue("claude-opus-5-5");
    fireEvent.click(screen.getByRole("radio", { name: "Gemini" }));
    expect(screen.getByLabelText("Model")).toHaveValue("gemini-4-hydrogen");
    expect(screen.getByLabelText("API key (memory only)")).toHaveValue("");
    expect(screen.getByRole("button", { name: "Save in this tab · no send" })).toBeDisabled();
    expect(onSave).not.toHaveBeenCalled();
    expect(host).not.toHaveBeenCalled();
  });
});
