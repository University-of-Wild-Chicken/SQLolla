import { useEffect, useRef, useState } from "react";
import { parseNextPayload, parseOpenPayload } from "../analysis/adapter";
import { buildEvidenceView } from "../analysis/evidence";
import { DEMO_SCENARIOS, nextPayloadFor } from "../analysis/fixtures";
import type { AnalysisTransport, EvidenceView, FixtureScenario, InstanceModel, OpenedAnalysis } from "../analysis/types";
import { matchesTarget, type FeedbackEvent } from "../feedback/journal";

interface ArchivedCase {
  readonly bundleId: string;
  readonly evidence: EvidenceView;
  readonly fixture: FixtureScenario | undefined;
  readonly openRaw: unknown;
  readonly nextRaw?: unknown;
}

interface ArchiveLoad {
  readonly scope: object;
  readonly caseKey: string;
  readonly pending: boolean;
  readonly result?: ArchivedCase;
}

/** Identify restored local evidence independently of the active scenario or editor. */
export function archivedScenario(archived: ArchivedCase, fallback: FixtureScenario): FixtureScenario {
  return (
    archived.fixture ?? {
      ...fallback,
      title: "Reopened local case",
      description: "Replayed from its saved evidence bundle.",
      statusLabel: "SAVED LOCAL CASE",
      provenance: { label: "Local solver evidence", source: "Local SQLolla host", note: "The archive was replayed before reopening." },
      summarySeed: null,
      bundleId: archived.bundleId,
      openPayload: archived.openRaw,
      nextByCursor: {},
    }
  );
}

/** Stable local key for lookup state across new events that relabel the same case. */
export function archiveKey(event: FeedbackEvent): string {
  return [event.bundle_id, event.instance_id, event.model_revision_sha256, event.semantic_sha256].join("|");
}

/** Require the bundle and all instance identities before showing a saved case. */
function archivedInstance(event: FeedbackEvent, instance: InstanceModel, bundleId: string): boolean {
  return matchesTarget(event, {
    bundleId,
    instanceId: instance.instanceId,
    modelRevisionSha256: instance.modelRevisionSha256,
    semanticSha256: instance.semanticSha256,
  });
}

/** Reopen later fixture cards from their immutable in-memory payloads, with no next command. */
function findFixtureCase(opened: OpenedAnalysis, event: FeedbackEvent, fixture: FixtureScenario): ArchivedCase | undefined {
  let cursor = opened.cursor;
  for (const _key of Object.keys(fixture.nextByCursor)) {
    if (!cursor) break;
    const raw = nextPayloadFor(fixture, cursor);
    const parsed = parseNextPayload(raw, opened.bundle, opened.bundleId);
    if (!parsed.ok) break;
    const step = parsed.value;
    if (step.evidence && archivedInstance(event, step.evidence.instance, step.bundleId)) {
      return { bundleId: step.bundleId, evidence: step.evidence, fixture, openRaw: opened.raw, nextRaw: raw };
    }
    cursor = step.cursor;
  }
  return undefined;
}

/** Ask the local archive to replay the saved bundle, then match the exact record. */
async function reopenCase(transport: AnalysisTransport, event: FeedbackEvent): Promise<ArchivedCase> {
  if (!event.bundle_id) throw new Error("This legacy label needs an exact bundle identity.");
  const response = await transport.open({ action: "instances.open", bundle_id: event.bundle_id });
  if (!response.ok) throw new Error(response.error.message);
  const parsed = parseOpenPayload(response.payload);
  if (!parsed.ok) throw new Error(parsed.error.message);
  const opened = parsed.value;
  if (opened.bundleId !== event.bundle_id) throw new Error("The archived bundle identity changed.");
  const fixture = DEMO_SCENARIOS.find((scenario) => scenario.bundleId === opened.bundleId);
  const record = opened.records.find((record) => record.instance && archivedInstance(event, record.instance, opened.bundleId));
  if (record?.instance) {
    return { bundleId: opened.bundleId, evidence: buildEvidenceView(opened.bundle, record.instance), fixture, openRaw: opened.raw };
  }
  const saved = fixture && findFixtureCase(opened, event, fixture);
  if (saved) return saved;
  throw new Error("The saved instance, revision and semantic identity could not be matched in this bundle.");
}

/** Bring the reopened card and its return control into view when the lookup succeeds. */
function useReopenedFocus(result: ArchivedCase | undefined): void {
  useEffect(() => {
    if (!result) return;
    document.getElementById("review-center")?.scrollIntoView?.({ block: "start" });
    document.getElementById("return-to-live-review")?.focus({ preventScroll: true });
  }, [result]);
}

/** Keep archive lookup and navigation independent of the live analysis cursor. */
export function useArchivedReview(transport: AnalysisTransport, scope: object) {
  const [load, setLoad] = useState<ArchiveLoad | null>(null);
  const [errors, setErrors] = useState<Readonly<Record<string, string>>>({});
  const token = useRef(0);
  const live = useRef(scope);
  live.current = scope;
  const visible = load?.scope === scope ? load : null;
  useReopenedFocus(visible?.result);
  const close = (): void => {
    ++token.current;
    setLoad(null);
  };
  const open = async (event: FeedbackEvent): Promise<void> => {
    const request = ++token.current;
    const caseKey = archiveKey(event);
    setLoad({ scope, caseKey, pending: true });
    try {
      const result = await reopenCase(transport, event);
      if (token.current !== request || live.current !== scope) return;
      setErrors((current) => ({ ...current, [caseKey]: "" }));
      setLoad({ scope, caseKey, pending: false, result });
    } catch (error) {
      if (token.current !== request || live.current !== scope) return;
      setErrors((current) => ({ ...current, [caseKey]: error instanceof Error ? error.message : "Unable to reopen the saved case." }));
      setLoad(null);
    }
  };
  return { visible, errors, open, close };
}
