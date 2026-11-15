import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { App } from "./App";
import { providers } from "./ai/config";
import type { RefinementBasis } from "./ai/refinement";
import { scenarioById } from "./analysis/fixtures";
import { hostJson } from "./analysis/host";
import demoRequest from "../e2e/tenant-request.json";
import { createFixtureTransport } from "./analysis/transports";
import { activeDecisions, JOURNAL_STORAGE_KEY, parseJournal } from "./feedback/journal";

vi.mock("./analysis/host", () => ({ hostJson: vi.fn() }));

const host = vi.mocked(hostJson);
const fixture = scenarioById("tenant-boundary")!;
const property = { status: "NO_COUNTEREXAMPLE_WITHIN_BOUNDS" };
const digest = "d".repeat(64);
const context = "e".repeat(64);
const candidate = "SELECT tenant_id FROM orders WHERE tenant_id = $1";
const instruction = "  Retain intended tenant reads.\nExclude reviewed leakage.  ";

/** Construct a consistent synthetic local response with all exact submitted case identities. */
function coreResponse(basis: RefinementBasis, sql?: string) {
  return {
    schema: "sqlolla.refinement_response.v1",
    status: "ok",
    bundle_id: basis.bundle_id,
    model_revision_sha256: basis.model_revision_sha256,
    feedback_sha256: digest,
    ...(sql
      ? {
          ready: true,
          property_check: property,
          candidate_request: { ...demoRequest, sql },
          cases: basis.feedback.map((label) => ({ ...label, status: label.disposition === "unsure" ? "advisory" : "satisfied" })),
        }
      : {}),
  };
}

/** Test the same host envelope without including evidence content or credentials in its body. */
function generatedResponse(basis: RefinementBasis) {
  const instanceId = basis.feedback.find((label) => label.disposition !== "unsure")!.instance_id;
  const reference = "/feedback/0/observation";
  return {
    schema: "sqlolla.ai.refinement.v1",
    status: "ok",
    provider: "openai",
    authority: "untrusted_assistance",
    model: providers.openai_compatible.model,
    bundle_id: basis.bundle_id,
    model_revision_sha256: basis.model_revision_sha256,
    feedback_sha256: digest,
    context_sha256: context,
    instance_id: instanceId,
    redact_values: false,
    references: [reference],
    document: {
      instance_id: instanceId,
      model_revision_sha256: basis.model_revision_sha256,
      context_sha256: context,
      title: "Untrusted model proposal",
      claims: [{ text: "This is an archived observation.", fact_references: [reference] }],
      assumptions: [],
      sql: candidate,
    },
    validation: coreResponse(basis, candidate),
  };
}

/** Open a simulated native analysis through the real workspace start/open flow. */
async function openNative() {
  const command = vi.fn(async (raw: unknown) => {
    const input = raw as RefinementBasis & { action: string; sql?: string };
    return { ok: true as const, payload: coreResponse(input, input.sql) };
  });
  render(<App transport={{ ...createFixtureTransport(), kind: "local-host", command, start: async () => ({ ok: true, payload: fixture.openPayload }) }} />);
  await screen.findByRole("heading", { name: "Orders without a tenant filter" });
  fireEvent.click(screen.getByRole("button", { name: "03 / Analyze" }));
  fireEvent.change(screen.getByLabelText("SQL query"), { target: { value: demoRequest.sql } });
  fireEvent.click(screen.getByRole("button", { name: "Analyze query" }));
  await screen.findByRole("heading", { name: "Your local SQL analysis" });
  return command;
}

/** Enable explicit unredacted generation using a synthetic tab-only provider token. */
function connectProvider() {
  fireEvent.click(screen.getByRole("button", { name: "Provider settings" }));
  fireEvent.click(screen.getByRole("radio", { name: "OpenAI" }));
  fireEvent.change(screen.getByLabelText("API key (memory only)"), { target: { value: "synthetic-refinement-key" } });
  fireEvent.click(screen.getByRole("checkbox", { name: /Redact SQL/ }));
  fireEvent.click(screen.getByRole("button", { name: /Save in this tab/ }));
}

function bin(name: "Yes" | "No" | "Unsure") {
  return within(screen.getByRole("region", { name }));
}

function labelKeys() {
  return activeDecisions(parseJournal(window.localStorage.getItem(JOURNAL_STORAGE_KEY)!));
}

beforeEach(() => {
  window.localStorage.clear();
  host.mockReset();
});

describe("feedback-driven SQL refinement", () => {
  it("reopens exact older and later fixture cases, moves a label and withdraws it without consuming a cursor", async () => {
    const next = vi.fn(createFixtureTransport().next);
    render(<App transport={{ ...createFixtureTransport(), next }} />);
    const first = await screen.findByTestId("review-card");
    const firstId = first.getAttribute("data-instance-id");
    fireEvent.click(screen.getByRole("button", { name: /Accept use case/ }));
    fireEvent.click(screen.getByRole("button", { name: "Next counterexample" }));
    await waitFor(() => expect(screen.getByTestId("review-card")).not.toHaveAttribute("data-instance-id", firstId));
    const secondId = screen.getByTestId("review-card").getAttribute("data-instance-id");
    fireEvent.click(screen.getByRole("button", { name: "Mark unsure" }));
    fireEvent.click(bin("Yes").getByRole("button", { name: "Open case" }));
    await screen.findByText("Reviewing a saved case");
    expect(screen.getByTestId("review-card")).toHaveAttribute("data-instance-id", firstId);
    expect(screen.queryByRole("button", { name: "Next counterexample" })).toBeNull();
    fireEvent.click(bin("Yes").getByRole("button", { name: "Move to No" }));
    expect(labelKeys()).toEqual(expect.arrayContaining([expect.objectContaining({ instance_id: firstId, disposition: "unintended_behavior" })]));
    fireEvent.click(bin("Unsure").getByRole("button", { name: "Open case" }));
    await waitFor(() => expect(screen.getByTestId("review-card")).toHaveAttribute("data-instance-id", secondId));
    fireEvent.click(screen.getByRole("button", { name: "Return to current workspace" }));
    expect(screen.getByTestId("review-card")).toHaveAttribute("data-instance-id", secondId);
    expect(screen.getByRole("button", { name: "Next counterexample" })).toBeEnabled();
    fireEvent.click(bin("No").getByRole("button", { name: "Withdraw" }));
    expect(labelKeys()).toMatchObject([{ instance_id: secondId, disposition: "unsure" }]);
    expect(next).toHaveBeenCalledTimes(1);
    expect(host).not.toHaveBeenCalled();
  });

  it("gates both routes on Yes or No and explicitly stages the exact locally checked request", async () => {
    const command = await openNative();
    connectProvider();
    const regenerate = screen.getByRole("button", { name: "Regenerate SQL" });
    const check = screen.getByRole("button", { name: "Check candidate SQL locally" });
    fireEvent.change(screen.getByLabelText("Original prompt"), { target: { value: instruction } });
    fireEvent.change(screen.getByLabelText("Candidate SQL"), { target: { value: candidate } });
    expect(regenerate).toBeDisabled();
    expect(check).toBeDisabled();
    fireEvent.keyDown(document.body, { key: "u" });
    expect(regenerate).toBeDisabled();
    expect(check).toBeDisabled();
    expect(host).not.toHaveBeenCalled();
    expect(command).not.toHaveBeenCalled();
    fireEvent.click(bin("Unsure").getByRole("button", { name: "Move to No" }));
    expect(regenerate).toBeEnabled();
    expect(screen.getByText("No Yes cases are protected by this check.")).toBeInTheDocument();
    fireEvent.click(check);
    const adopt = await screen.findByRole("button", { name: "Use checked SQL as new draft" });
    expect(adopt).toBeEnabled();
    expect(command).toHaveBeenCalledTimes(2);
    expect(command.mock.calls[0]?.[0]).toMatchObject({ action: "refinement.context", feedback: labelKeys().map(({ event_id }) => ({ event_id })) });
    expect(command.mock.calls[1]?.[0]).toMatchObject({ action: "refinement.check", feedback_sha256: digest, sql: candidate });
    expect(host).not.toHaveBeenCalled();
    fireEvent.click(adopt);
    expect(await screen.findByRole("heading", { name: "Ready to check a new question" })).toBeInTheDocument();
    expect(screen.getByLabelText("SQL query")).toHaveValue(candidate);
    expect(screen.getByLabelText("Original prompt")).toHaveValue(instruction);
    expect(screen.queryByTestId("review-card")).toBeNull();
    expect(bin("No").getByRole("button", { name: "Open case" })).toBeEnabled();
  });

  it("drops a late AI result when a bin changes and reuses the exact prompt on the next request", async () => {
    await openNative();
    connectProvider();
    fireEvent.change(screen.getByLabelText("Original prompt"), { target: { value: instruction } });
    fireEvent.keyDown(document.body, { key: "r" });
    let resolve!: (value: unknown) => void;
    host.mockImplementationOnce(
      () =>
        new Promise((done) => {
          resolve = done;
        }),
    );
    const regenerate = screen.getByRole("button", { name: "Regenerate SQL" });
    fireEvent.click(regenerate);
    const [path, raw, options] = host.mock.calls[0]!;
    const first = raw as RefinementBasis;
    expect(path).toBe("/api/ai/byot/refine");
    expect(raw).toMatchObject({ instruction, redact_values: false, feedback: [{ disposition: "unintended_behavior" }] });
    expect(JSON.stringify(raw)).not.toContain("synthetic-refinement-key");
    expect(options?.headers?.Authorization).toBe("Bearer synthetic-refinement-key");
    fireEvent.click(bin("No").getByRole("button", { name: "Move to Yes" }));
    expect(options?.signal?.aborted).toBe(true);
    await act(async () => resolve(generatedResponse(first)));
    expect(screen.queryByRole("button", { name: "Use checked SQL as new draft" })).toBeNull();
    host.mockImplementationOnce(async (_path, body) => generatedResponse(body as RefinementBasis));
    fireEvent.click(regenerate);
    const adopt = await screen.findByRole("button", { name: "Use checked SQL as new draft" });
    expect(adopt).toBeEnabled();
    expect(host.mock.calls[1]?.[1]).toMatchObject({ instruction, feedback: [{ disposition: "accepted_use_case" }] });
    expect(within(screen.getByRole("region", { name: "Refine the query" })).getByRole("button", { name: "Copy SQL" })).toBeEnabled();
    fireEvent.click(bin("Yes").getByRole("button", { name: "Move to Unsure" }));
    expect(screen.queryByRole("button", { name: "Use checked SQL as new draft" })).toBeNull();
    expect(regenerate).toBeDisabled();
  });
});
