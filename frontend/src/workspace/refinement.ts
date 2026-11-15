import { useEffect, useMemo, useRef, useState } from "react";
import { AI_MODEL } from "../ai/config";
import { checkRevision, generateRevision, type CheckedRevision, type RefinementBasis, type RefinementLabel } from "../ai/refinement";
import type { AiProviderSettings } from "../ai/summary";
import { DEMO_SCENARIOS } from "../analysis/fixtures";
import type { SessionState } from "../analysis/session";
import type { AnalysisTransport } from "../analysis/types";
import { activeDecisions, detectConflicts, type FeedbackEvent } from "../feedback/journal";
import type { FeedbackJournal } from "../feedback/useFeedbackJournal";
import { archiveKey } from "./archivedReview";

interface RefinementInputs {
  readonly transport: AnalysisTransport;
  readonly session: SessionState;
  readonly draftActive: boolean;
  readonly journal: FeedbackJournal;
  readonly archiveErrors: Readonly<Record<string, string>>;
  readonly settings: AiProviderSettings;
  readonly onAdopt: (request: Record<string, unknown>) => void;
}

type Operation = "generate" | "check";
interface RunState {
  readonly identity: object;
  readonly busy?: Operation;
  readonly error?: string;
  readonly result?: CheckedRevision;
}

/** Distinguish unresolved, duplicate and contradictory active labels before contacting the host. */
function issues(inputs: RefinementInputs, events: ReturnType<typeof activeDecisions>): string | null {
  if (inputs.journal.storageError) return `Review journal unavailable. ${inputs.journal.storageError}`;
  if (!events.some(({ disposition }) => disposition !== "unsure")) return "Mark at least one case Yes or No to continue.";
  if (detectConflicts(inputs.journal.events).length) return "Resolve contradictory Yes and No labels in the review bins or ledger.";
  if (events.some((event) => !event.bundle_id)) return "Some labels have no bundle identity. Review their original cases or withdraw those labels.";
  if (events.some((event) => inputs.archiveErrors[archiveKey(event)])) return "Some saved cases could not be reopened. Retry or withdraw their labels.";
  const fixtureIds = new Set(DEMO_SCENARIOS.map((scenario) => scenario.bundleId));
  if (events.some((event) => event.bundle_id !== inputs.session.bundleId && fixtureIds.has(event.bundle_id ?? null))) {
    return "Some labels belong to demo cases. Local refinement needs archived solver cases.";
  }
  if (new Set(events.map((event) => `${event.bundle_id}|${event.instance_id}`)).size !== events.length) {
    return "At least one case has multiple active labels. Keep one label for each saved case.";
  }
  return null;
}

/** Send exact active journal identities without history, evidence content or credentials. */
function refinementLabel(event: FeedbackEvent & { disposition: RefinementLabel["disposition"] }): RefinementLabel {
  if (!event.bundle_id) throw new Error("Missing bundle identity.");
  return {
    bundle_id: event.bundle_id,
    instance_id: event.instance_id,
    model_revision_sha256: event.model_revision_sha256,
    semantic_sha256: event.semantic_sha256,
    event_id: event.event_id,
    disposition: event.disposition,
    ...(event.rationale ? { rationale: event.rationale } : {}),
  };
}

/** Derive a basis only for settled bounded native evidence and an unambiguous journal. */
function refinementSnapshot(inputs: RefinementInputs): { basis: RefinementBasis | null; blocked: string | null; hasYes: boolean } {
  const { session, transport } = inputs;
  const events = activeDecisions(inputs.journal.events);
  const hasYes = events.some((event) => event.disposition === "accepted_use_case");
  const blocked = issues(inputs, events);
  if (blocked) return { basis: null, blocked, hasYes };
  if (transport.kind !== "local-host" || !session.native || session.phase !== "ready" || !session.bundleId || !session.bundle) {
    return { basis: null, blocked: "Run a local analysis to refine saved solver cases.", hasYes };
  }
  if (inputs.draftActive) return { basis: null, blocked: "Analyze the current draft before refining it.", hasYes };
  if (session.bundle.request.mode !== "analyze" || !session.bundle.request.property) {
    return { basis: null, blocked: "Refinement requires a bounded analysis with a property.", hasYes };
  }
  const basis = { bundle_id: session.bundleId, model_revision_sha256: session.bundle.modelRevisionSha256, feedback: events.map(refinementLabel) };
  return { basis, blocked: null, hasYes };
}

/** Hold the exact original instruction in the tab while the journal and candidate change. */
function useRevisionInputs() {
  const [instruction, setInstruction] = useState("");
  const [sql, setSql] = useState("");
  return { instruction, setInstruction, sql, setSql };
}

/** Report AI-specific admission requirements independently of the offline checker. */
function generationIssue(basis: RefinementBasis | null, instruction: string, settings: AiProviderSettings): string | null {
  if (!basis) return null;
  if (basis.feedback.length > 256) return "Regeneration supports up to 256 active labels.";
  if (settings.mode !== "openai_compatible" || settings.model !== AI_MODEL || !settings.apiKey.trim() || settings.redactValues) {
    return "Connect OpenAI and allow unredacted context in provider settings to regenerate.";
  }
  return instruction.trim() ? null : "Enter the original prompt to regenerate.";
}

/** Invalidate the result on any change to the sent source, journal or editor fields. */
function useRevisionSnapshot(inputs: RefinementInputs, editors: ReturnType<typeof useRevisionInputs>) {
  const { session, journal, archiveErrors, settings, draftActive, transport } = inputs;
  const { instruction, sql } = editors;
  const { basis, blocked, hasYes } = refinementSnapshot(inputs);
  const identity = useMemo(
    () => ({}),
    [session.generation, session.bundleId, draftActive, journal.events, archiveErrors, settings, instruction, sql, transport, blocked],
  );
  const generationBlocked = generationIssue(basis, instruction, settings);
  return { identity, basis, blocked, generationBlocked, hasYes };
}

/** Run one explicit operation and discard any completion whose input snapshot changed. */
function useRevisionRun(identity: object) {
  const live = useRef(identity);
  live.current = identity;
  const active = useRef<{ identity: object; controller: AbortController } | null>(null);
  const [state, setState] = useState<RunState | null>(null);
  useEffect(
    () => () => {
      if (active.current?.identity === identity) active.current.controller.abort();
    },
    [identity],
  );
  const run = async (kind: Operation, request: (signal: AbortSignal) => Promise<CheckedRevision>): Promise<void> => {
    if (live.current !== identity || active.current?.identity === identity) return;
    active.current?.controller.abort();
    const controller = new AbortController();
    active.current = { identity, controller };
    setState({ identity, busy: kind });
    try {
      const result = await request(controller.signal);
      if (live.current === identity && !controller.signal.aborted) setState({ identity, result });
    } catch (error) {
      if (live.current === identity && !controller.signal.aborted) setState({ identity, error: error instanceof Error ? error.message : "The check failed." });
    } finally {
      if (active.current?.controller === controller) active.current = null;
    }
  };
  return { live, run, current: state?.identity === identity ? state : null, stale: !!state && state.identity !== identity };
}

/** Expose same-prompt regeneration, deterministic candidate checking and explicit staging. */
export function useRefinement(inputs: RefinementInputs) {
  const editors = useRevisionInputs();
  const snapshot = useRevisionSnapshot(inputs, editors);
  const { basis, identity, generationBlocked } = snapshot;
  const { current, stale, live, run } = useRevisionRun(identity);
  const generate = (): Promise<void> | undefined => {
    if (basis && !generationBlocked) return run("generate", (signal) => generateRevision(basis, editors.instruction, inputs.settings, signal));
  };
  const check = (): Promise<void> | undefined => {
    if (basis && editors.sql.trim() && basis.feedback.length <= 256)
      return run("check", (signal) => checkRevision(inputs.transport, basis, editors.sql, signal));
  };
  const adopt = (): void => {
    if (live.current !== identity || !basis || !current?.result?.candidateRequest) return;
    live.current = {};
    inputs.onAdopt(current.result.candidateRequest);
  };
  return { ...editors, ...snapshot, current, stale, generate, check, adopt };
}
