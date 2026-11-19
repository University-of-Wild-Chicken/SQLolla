/** The production workbench explains its purpose and AI's role without sending any request. */
import { describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen, within } from "@testing-library/react";
import { App } from "../App";
import { createFixtureTransport } from "../analysis/transports";
import type { AnalysisTransport } from "../analysis/types";

vi.mock("../analysis/host", async (original) => ({
  ...(await original<typeof import("../analysis/host")>()),
  hostStatus: vi.fn(async () => ({ schema: "sqlolla.host.v1", csrf_token: "a".repeat(64) })),
  hostJson: vi.fn(),
}));

/** A host-shaped transport with no fixture library, as in production. */
function productionTransport(): AnalysisTransport {
  return { ...createFixtureTransport(), fixtures: undefined, kind: "local-host" };
}

describe("production workbench", () => {
  it("states the review loop, the three AI jobs and who decides", async () => {
    window.localStorage.clear();
    render(<App transport={productionTransport()} />);
    expect(screen.getByRole("heading", { level: 1, name: "Query workbench" })).toBeInTheDocument();
    for (const step of ["Build", "Check", "Review", "Refine"]) expect(screen.getAllByText(step).length).toBeGreaterThan(0);
    const intro = screen.getByRole("region", { name: "AI assistant (optional)" });
    for (const job of ["Draft", "Explain", "Refine"]) expect(within(intro).getByText(job)).toBeInTheDocument();
    expect(within(intro).getByText("AI proposes. SQLolla decides.")).toBeInTheDocument();
    expect(await screen.findByText("Analysis server reachable")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "AI: not connected" })).toBeInTheDocument();
    fireEvent.click(within(intro).getByRole("button", { name: "Continue without AI" }));
    expect(screen.queryByRole("region", { name: "AI assistant (optional)" })).toBeNull();
    expect(screen.getByRole("region", { name: "Query editor" })).toBeInTheDocument();
    expect(screen.getByRole("region", { name: "Results" })).toBeInTheDocument();
  });
});
