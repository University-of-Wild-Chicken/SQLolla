/**
 * Injectable transports distinguish labelled local fixtures from explicit
 * same-origin commands to the Rust host. Host failures never become fixture evidence.
 */
import { DEMO_SCENARIOS, nextPayloadFor } from "./fixtures";
import type { AnalysisTransport, FixtureScenario, OpenCommand, StartCommand, NextCommand, TransportResult } from "./types";

/** Find the fixture scenario that owns an opaque demo cursor. */
function scenarioForCursor(cursor: string): FixtureScenario | undefined {
  return DEMO_SCENARIOS.find((scenario) => Object.hasOwn(scenario.nextByCursor, cursor));
}

/** Convert one scenario's optional demo transport failure into a transport result. */
function transportGap(scenario: FixtureScenario | undefined): TransportResult | null {
  if (!scenario?.transportFailure) return null;
  return { ok: false, error: scenario.transportFailure };
}

/** Supply deterministic stored cases for tests without connecting a solver. */
export function createFixtureTransport(): AnalysisTransport {
  return {
    id: "sqlolla-fixture-evidence-v1",
    label: "Local fixture archive",
    kind: "fixture",
    fixtures: DEMO_SCENARIOS,
    async open(command: OpenCommand): Promise<TransportResult> {
      const scenario = DEMO_SCENARIOS.find((candidate) => candidate.bundleId === command.bundle_id);
      const gap = transportGap(scenario);
      if (gap) return gap;
      if (!scenario) return missingScenario(command.bundle_id);
      return { ok: true, payload: scenario.openPayload };
    },
    async next(command: NextCommand): Promise<TransportResult> {
      const scenario = scenarioForCursor(command.cursor);
      const gap = transportGap(scenario);
      if (gap) return gap;
      if (!scenario) return missingCursor(command.cursor);
      return { ok: true, payload: nextPayloadFor(scenario, command.cursor) };
    },
    async start(_command: StartCommand): Promise<TransportResult> {
      return startUnavailable();
    },
  };
}

/** Fixture mode opens stored revisions; it does not pretend to compile a request. */
function startUnavailable(): TransportResult {
  return {
    ok: false,
    error: {
      kind: "transport",
      code: "transport.start_unavailable",
      message: "Fixture mode opens stored evidence only. Starting new analysis requires the separate host bridge.",
      retryable: false,
    },
  };
}

/** Report an unknown bundle id as a fixture lookup failure, not an enum result. */
function missingScenario(bundleId: string): TransportResult {
  return {
    ok: false,
    error: {
      kind: "transport",
      code: "transport.fixture_unknown_bundle",
      message: `No demo scenario owns bundle ${bundleId.slice(0, 12)}…`,
      retryable: false,
    },
  };
}

/** Report an unknown cursor without pretending the solver exhausted its search. */
function missingCursor(cursor: string): TransportResult {
  return {
    ok: false,
    error: {
      kind: "transport",
      code: "transport.fixture_unknown_cursor",
      message: `The fixture cursor ${cursor.slice(0, 12)}… is not known locally.`,
      retryable: false,
    },
  };
}
