import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { App } from "./App";
import { scenarioById } from "./analysis/fixtures";
import { createFixtureTransport } from "./analysis/transports";

describe("explicit native workspace actions", () => {
  it("reopens native output then selects a different library case without relabelling the native bundle", async () => {
    const user = userEvent.setup();
    const scenario = scenarioById("tenant-boundary")!;
    const fixtures = createFixtureTransport();
    const open = vi.fn(fixtures.open);
    const start = vi.fn(async () => ({ ok: true as const, payload: scenario.openPayload }));
    render(<App transport={{ ...fixtures, open, start }} />);
    await screen.findByRole("heading", { name: "Orders without a tenant filter" });
    await user.click(screen.getByRole("button", { name: "03 / Analyze" }));
    await user.click(screen.getByRole("button", { name: "Analyze query" }));
    expect(await screen.findByRole("heading", { name: "Your local SQL analysis" })).toBeInTheDocument();
    expect(screen.getByText("LOCAL SOLVER EVIDENCE")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "01 / Cases" }));
    await user.click(screen.getByRole("button", { name: /A parameterized tenant read/ }));
    expect(await screen.findByRole("heading", { name: "A parameterized tenant read" })).toBeInTheDocument();
    expect(open.mock.lastCall?.[0].bundle_id).toBe(scenarioById("scoped-orders")!.bundleId);
    expect(screen.queryByText("LOCAL SOLVER EVIDENCE")).toBeNull();
  });

  it("checks a card draft through the backend and stages a separate explicit analysis", async () => {
    const user = userEvent.setup();
    const command = vi.fn(async (_request: unknown) => ({ ok: true as const, payload: { status: "ok", ir: { test: true }, model: "module test" } }));
    const start = vi.fn(createFixtureTransport().start);
    render(<App transport={{ ...createFixtureTransport(), command, start }} />);
    await screen.findByRole("heading", { name: "Orders without a tenant filter" });
    await user.click(screen.getByRole("button", { name: "Start a local draft" }));
    await user.click(screen.getByRole("button", { name: "Check draft with deterministic compiler" }));
    expect(await screen.findByText("Compiled by the deterministic backend")).toBeInTheDocument();
    expect(command.mock.calls[0]![0]).toMatchObject({ mode: "compile", include_model: true });
    await user.click(screen.getByRole("button", { name: "Review as a new analysis" }));
    expect(await screen.findByRole("heading", { name: "Ready to check a new question" })).toBeInTheDocument();
    expect(screen.getByLabelText("Analysis goal")).toHaveValue("example");
    expect(start).not.toHaveBeenCalled();
  });
});
