import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { App } from "./App";
import { nextPayloadFor, scenarioById } from "./analysis/fixtures";
import { makeErrorResponse } from "./analysis/fixtureFactory";
import { createFixtureTransport } from "./analysis/transports";
import type { AnalysisTransport, NextCommand, TransportResult } from "./analysis/types";

/** Wrap the deterministic fixture transport with one controlled operation. */
function withNext(next: AnalysisTransport["next"]): AnalysisTransport {
  return { ...createFixtureTransport(), next };
}

function nextButton(): HTMLElement {
  return screen.getByRole("button", { name: /Next counterexample|Retry next counterexample/i });
}

describe("review workspace workflows", () => {
  beforeEach(() => {
    window.localStorage.clear();
  });

  it("serializes next clicks and advances only after resolving one response", async () => {
    const scenario = scenarioById("tenant-boundary")!;
    let resolveNext!: (result: TransportResult) => void;
    const next = vi.fn(
      (command: NextCommand) =>
        new Promise<TransportResult>((resolve) => {
          resolveNext = resolve;
          void command;
        }),
    );
    render(<App transport={withNext(next)} />);
    await screen.findByRole("heading", { name: "Orders without a tenant filter" });
    const beforeId = screen.getByTestId("review-card").getAttribute("data-instance-id");
    const control = nextButton();
    fireEvent.click(control);
    fireEvent.click(control);
    expect(next).toHaveBeenCalledTimes(1);
    const cursor = next.mock.calls[0]![0].cursor;
    const payload = nextPayloadFor(scenario, cursor);
    await act(async () => {
      resolveNext({ ok: true, payload });
    });
    await waitFor(() => expect(screen.getByTestId("review-card")).not.toHaveAttribute("data-instance-id", beforeId));
    expect(screen.getByRole("button", { name: /Next counterexample/i })).toBeEnabled();
  });

  it("retains the cursor after a failed next click and retries the same request", async () => {
    const scenario = scenarioById("tenant-boundary")!;
    const firstPayload = makeErrorResponse("resource.resume_time", "Complete prefix replay exceeded time capacity");
    const secondPayload = nextPayloadFor(scenario, "fixture-cursor-tenant-1");
    const next = vi.fn(async (_command: NextCommand): Promise<TransportResult> => {
      const call = next.mock.calls.length;
      return { ok: true, payload: call === 1 ? firstPayload : secondPayload };
    });
    const user = userEvent.setup();
    render(<App transport={withNext(next)} />);
    await screen.findByRole("heading", { name: "Orders without a tenant filter" });
    const beforeId = screen.getByTestId("review-card").getAttribute("data-instance-id");
    await user.click(nextButton());
    expect(await screen.findByText("Timeout or resource limit")).toBeInTheDocument();
    expect(screen.getByTestId("review-card")).toHaveAttribute("data-instance-id", beforeId!);
    await user.click(screen.getByRole("button", { name: /Retry next counterexample/i }));
    expect(next.mock.calls[1]![0].cursor).toBe(next.mock.calls[0]![0].cursor);
    await waitFor(() => expect(screen.getByTestId("review-card")).not.toHaveAttribute("data-instance-id", beforeId));
  });

  it("discards a stale-revision next response and leaves the current evidence intact", async () => {
    const scenario = scenarioById("tenant-boundary")!;
    const next = vi.fn(async (command: NextCommand): Promise<TransportResult> => {
      const payload = nextPayloadFor(scenario, command.cursor) as Record<string, unknown>;
      return {
        ok: true,
        payload: { ...payload, model_revision_sha256: "9".repeat(64) },
      };
    });
    const user = userEvent.setup();
    render(<App transport={withNext(next)} />);
    await screen.findByRole("heading", { name: "Orders without a tenant filter" });
    const beforeId = screen.getByTestId("review-card").getAttribute("data-instance-id");
    await user.click(nextButton());
    expect(await screen.findByText("Stale revision discarded")).toBeInTheDocument();
    expect(screen.getByTestId("review-card")).toHaveAttribute("data-instance-id", beforeId!);
    await user.click(screen.getByRole("button", { name: /Retry next counterexample/i }));
    expect(next).toHaveBeenCalledTimes(2);
    expect(next.mock.calls[1]![0].cursor).toBe(next.mock.calls[0]![0].cursor);
  });

  it("offers keyboard-equivalent review and an append-only undo without auto-advancing", async () => {
    const next = vi.fn(createFixtureTransport().next);
    render(<App transport={withNext(next)} />);
    await screen.findByRole("heading", { name: "Orders without a tenant filter" });
    await act(async () => {
      await Promise.resolve();
    });
    fireEvent.keyDown(document.body, { key: "a" });
    expect(await screen.findByRole("heading", { name: "Preserved use case" })).toBeInTheDocument();
    expect(next).not.toHaveBeenCalled();
    fireEvent.keyDown(document.body, { key: "z", ctrlKey: true });
    expect(await screen.findByRole("heading", { name: "No active label" })).toBeInTheDocument();
    expect(next).not.toHaveBeenCalled();
    expect(window.localStorage.getItem("sqlolla.feedback.journal.v1")).toContain("withdrawn");
  });

  it("shows duplicate occurrences and typed NULL separately in the table view", async () => {
    const user = userEvent.setup();
    render(<App transport={createFixtureTransport()} />);
    await screen.findByRole("heading", { name: "Orders without a tenant filter" });
    await user.click(screen.getByRole("button", { name: "Table" }));
    const before = screen.getByRole("region", { name: "BEFORE public.orders" });
    expect(within(before).getAllByText("Annual renewal")).toHaveLength(2);
    const typedNulls = before.querySelectorAll(".typed-null");
    expect(typedNulls).toHaveLength(1);
    expect(typedNulls[0]?.textContent).toMatch(/NULL.*int32/);
    expect(within(before).getAllByText(/ORD-1007/)).toHaveLength(2);
  });

  it("keeps deterministic evidence usable when no fixture explanation exists", async () => {
    render(<App transport={createFixtureTransport()} initialScenarioId="state-sql-error" />);
    await screen.findByRole("heading", { name: "A modeled SQL rejection" });
    expect(await screen.findByText("No fixture explanation is supplied for this case. Deterministic evidence remains fully usable.")).toBeInTheDocument();
    expect(screen.getAllByText(/23514/).length).toBeGreaterThan(0);
    expect(screen.getByRole("button", { name: "Table" })).toBeInTheDocument();
  });

  it("rejects unsupported evidence schema at the runtime boundary", async () => {
    const transport: AnalysisTransport = {
      ...createFixtureTransport(),
      open: async () => ({
        ok: true,
        payload: { schema: "sqlolla.enumeration_response.v99", status: "ok" },
      }),
    };
    render(<App transport={transport} />);
    expect(await screen.findByText("Unsupported evidence format")).toBeInTheDocument();
    expect(screen.getByText(/unsupported schema/i)).toBeInTheDocument();
    expect(screen.queryByTestId("review-card")).toBeNull();
  });

  it("sets aside validated evidence on a local draft and refuses to pretend a compiler exists", async () => {
    const user = userEvent.setup();
    render(<App transport={createFixtureTransport()} />);
    await screen.findByRole("heading", { name: "Orders without a tenant filter" });
    await user.click(screen.getByRole("button", { name: "Start a local draft" }));
    expect(await screen.findByRole("heading", { name: "Draft awaiting deterministic analysis" })).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "Orders without a tenant filter" })).toBeNull();
    await user.click(screen.getByRole("button", { name: "Continue in Query studio" }));
    const limit = screen.getByLabelText(/Result limit/i);
    await user.clear(limit);
    await user.type(limit, "25");
    expect(screen.getAllByText(/LIMIT 25/).length).toBeGreaterThan(0);
    await user.click(screen.getByRole("button", { name: "Check draft with deterministic compiler" }));
    expect(await screen.findByText("Analysis bridge unavailable")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "01 / Cases" }));
    await user.click(screen.getByRole("button", { name: /A parameterized tenant read/ }));
    expect(await screen.findByRole("heading", { name: "Discard the local draft?" })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Keep draft" }));
    expect(screen.getByRole("heading", { name: "Draft awaiting deterministic analysis" })).toBeInTheDocument();
  });

  it("keeps provider secrets out of browser storage and journal exports", async () => {
    const user = userEvent.setup();
    render(<App transport={createFixtureTransport()} />);
    await screen.findByRole("heading", { name: "Orders without a tenant filter" });
    await user.click(screen.getByRole("button", { name: "Open AI provider settings" }));
    await user.click(screen.getByRole("radio", { name: /OpenAI-compatible/ }));
    await user.type(screen.getByLabelText(/API key \(memory only\)/i), "sk-live-should-not-persist");
    await user.type(screen.getByLabelText(/Model identifier/i), "future-model");
    await user.click(screen.getByRole("button", { name: /Save in this tab/i }));
    expect(window.localStorage.getItem("sqlolla.feedback.journal.v1") ?? "").not.toContain("sk-live-should-not-persist");
    for (let index = 0; index < window.localStorage.length; index += 1) {
      const key = window.localStorage.key(index)!;
      expect(window.localStorage.getItem(key) ?? "").not.toContain("sk-live-should-not-persist");
    }
  });

  it("renders a failure state for a transport gap without inventing evidence", async () => {
    render(<App transport={createFixtureTransport()} initialScenarioId="state-transport-gap" />);
    expect(await screen.findByText("Analysis host unavailable")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "No host analysis bridge" })).toBeInTheDocument();
    expect(screen.queryByTestId("review-card")).toBeNull();
  });
});
