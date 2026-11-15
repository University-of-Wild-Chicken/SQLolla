/**
 * Append-only browser-local business-intent journal. Evidence remains immutable;
 * undo is a superseding withdrawal event, never an edit or deletion.
 */
import { canonicalJson } from "../analysis/format";

/** Feedback labels; withdrawal exists only to supersede a previous label. */
export type FeedbackDisposition = "accepted_use_case" | "unintended_behavior" | "withdrawn";

/** The exact persisted feedback event shape. */
export interface FeedbackEvent {
  readonly event_id: string;
  readonly idempotency_key: string;
  readonly instance_id: string;
  readonly model_revision_sha256: string;
  readonly semantic_sha256: string;
  readonly disposition: FeedbackDisposition;
  readonly rationale?: string;
  readonly created_at: string;
  readonly supersedes_event_id?: string;
}

/** The export/import envelope, versioned separately from evidence. */
export interface FeedbackJournalEnvelope {
  readonly schema: "sqlolla.feedback_journal.v1";
  readonly exported_at: string;
  readonly events: readonly FeedbackEvent[];
}

/** Identity of the observation receiving a decision. */
export interface FeedbackTarget {
  readonly instanceId: string;
  readonly modelRevisionSha256: string;
  readonly semanticSha256: string;
}

/** Deterministic seams for tests; production defaults use browser time/UUIDs. */
export interface EventDependencies {
  readonly now: () => string;
  readonly newId: () => string;
}

/** Minimal storage interface, satisfied by window.localStorage. */
export interface JournalStorage {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
}

/** Conflict group for one contextual semantic observation. */
export interface FeedbackConflict {
  readonly revision: string;
  readonly semantic: string;
  readonly accepted: readonly FeedbackEvent[];
  readonly rejected: readonly FeedbackEvent[];
}

/** Result of a proposed decision or withdrawal. */
export type FeedbackChange =
  | { readonly kind: "appended"; readonly events: readonly FeedbackEvent[] }
  | { readonly kind: "noop"; readonly reason: string }
  | { readonly kind: "conflict"; readonly reason: string };

/** Result of importing a journal. */
export type ImportResult =
  | {
      readonly ok: true;
      readonly events: readonly FeedbackEvent[];
      readonly added: number;
      readonly duplicates: number;
    }
  | { readonly ok: false; readonly message: string };

/** Stable local-storage key. Contains only journal events, never credentials. */
export const JOURNAL_STORAGE_KEY = "sqlolla.feedback.journal.v1";
const JOURNAL_SCHEMA = "sqlolla.feedback_journal.v1";
const HASH = /^[0-9a-f]{64}$/;

/** Production event identity/time seams; no secret material is involved. */
export const defaultEventDependencies: EventDependencies = {
  now: () => new Date().toISOString(),
  newId: () =>
    typeof crypto !== "undefined" && "randomUUID" in crypto ? crypto.randomUUID() : `feedback-${Date.now()}-${Math.random().toString(16).slice(2)}`,
};

/** Check the exact event keys so unsupported or secret-bearing fields are rejected. */
function hasOnlyEventKeys(object: Record<string, unknown>): boolean {
  const allowed = new Set([
    "event_id",
    "idempotency_key",
    "instance_id",
    "model_revision_sha256",
    "semantic_sha256",
    "disposition",
    "rationale",
    "created_at",
    "supersedes_event_id",
  ]);
  return Object.keys(object).every((key) => allowed.has(key));
}

/** Read one required non-empty event string. */
function eventString(object: Record<string, unknown>, key: string): string {
  const field = object[key];
  if (typeof field !== "string" || field.length === 0) {
    throw new Error(`Feedback ${key} must be a non-empty string`);
  }
  return field;
}

/** Read and validate the bounded optional rationale. */
function eventRationale(object: Record<string, unknown>): string | undefined {
  const rationale = object["rationale"];
  if (rationale !== undefined && (typeof rationale !== "string" || rationale.length > 500)) {
    throw new Error("Feedback rationale must be a string of at most 500 characters");
  }
  return rationale as string | undefined;
}

/** Read and validate the optional superseded event identity. */
function eventSupersedes(object: Record<string, unknown>): string | undefined {
  const supersedes = object["supersedes_event_id"];
  if (supersedes !== undefined && (typeof supersedes !== "string" || supersedes.length === 0)) {
    throw new Error("Feedback supersedes_event_id must be a non-empty string");
  }
  return supersedes as string | undefined;
}

/** Validate one unknown value as an object with only supported event keys. */
function eventObject(value: unknown): Record<string, unknown> {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    throw new Error("Each feedback event must be an object");
  }
  const object = value as Record<string, unknown>;
  if (!hasOnlyEventKeys(object)) throw new Error("Feedback event has unsupported fields");
  return object;
}

/** Validate and reconstruct one untrusted event without copying extra fields. */
export function parseFeedbackEvent(value: unknown): FeedbackEvent {
  const object = eventObject(value);
  const disposition = eventString(object, "disposition");
  if (!["accepted_use_case", "unintended_behavior", "withdrawn"].includes(disposition)) {
    throw new Error(`Unsupported feedback disposition "${disposition}"`);
  }
  const revision = eventString(object, "model_revision_sha256");
  const semantic = eventString(object, "semantic_sha256");
  if (!HASH.test(revision) || !HASH.test(semantic)) {
    throw new Error("Feedback revision/semantic identity is malformed");
  }
  const createdAt = eventString(object, "created_at");
  if (!Number.isFinite(Date.parse(createdAt))) {
    throw new Error("Feedback created_at must be a valid timestamp");
  }
  const rationale = eventRationale(object);
  const supersedes = eventSupersedes(object);
  return {
    event_id: eventString(object, "event_id"),
    idempotency_key: eventString(object, "idempotency_key"),
    instance_id: eventString(object, "instance_id"),
    model_revision_sha256: revision,
    semantic_sha256: semantic,
    disposition: disposition as FeedbackDisposition,
    ...(rationale === undefined ? {} : { rationale }),
    created_at: createdAt,
    ...(supersedes === undefined ? {} : { supersedes_event_id: supersedes }),
  };
}

/** Return events not superseded by any later event. */
export function activeEvents(events: readonly FeedbackEvent[]): readonly FeedbackEvent[] {
  const superseded = new Set(events.flatMap((event) => (event.supersedes_event_id ? [event.supersedes_event_id] : [])));
  return events.filter((event) => !superseded.has(event.event_id));
}

/** Return active decision events, excluding withdrawal records. */
export function activeDecisions(events: readonly FeedbackEvent[]): readonly FeedbackEvent[] {
  return activeEvents(events).filter((event) => event.disposition !== "withdrawn");
}

/** Contextual key groups different atom assignments with the same SQL behavior. */
export function contextKey(event: Pick<FeedbackEvent, "model_revision_sha256" | "semantic_sha256">): string {
  return `${event.model_revision_sha256}|${event.semantic_sha256}`;
}

/** Find the latest active decision for an exact contextual observation. */
export function activeDecisionFor(events: readonly FeedbackEvent[], target: FeedbackTarget): FeedbackEvent | undefined {
  const key = `${target.modelRevisionSha256}|${target.semanticSha256}`;
  return activeDecisions(events)
    .filter((event) => contextKey(event) === key)
    .slice(-1)[0];
}

/** Detect contradictory active labels for the same contextual semantic observation. */
export function detectConflicts(events: readonly FeedbackEvent[]): readonly FeedbackConflict[] {
  const groups = new Map<string, FeedbackEvent[]>();
  for (const event of activeDecisions(events)) {
    const key = contextKey(event);
    groups.set(key, [...(groups.get(key) ?? []), event]);
  }
  const conflicts: FeedbackConflict[] = [];
  for (const group of groups.values()) {
    const accepted = group.filter((event) => event.disposition === "accepted_use_case");
    const rejected = group.filter((event) => event.disposition === "unintended_behavior");
    if (accepted.length > 0 && rejected.length > 0) {
      conflicts.push({
        revision: group[0]!.model_revision_sha256,
        semantic: group[0]!.semantic_sha256,
        accepted,
        rejected,
      });
    }
  }
  return conflicts;
}

/** Return active labels written against a different model revision. */
export function staleDecisions(events: readonly FeedbackEvent[], currentRevision: string | null): readonly FeedbackEvent[] {
  if (!currentRevision) return activeDecisions(events);
  return activeDecisions(events).filter((event) => event.model_revision_sha256 !== currentRevision);
}

/** Build a decision event with stable target identity and optional supersession. */
function decisionEvent(
  target: FeedbackTarget,
  disposition: "accepted_use_case" | "unintended_behavior",
  rationale: string,
  active: FeedbackEvent | undefined,
  dependencies: EventDependencies,
): FeedbackEvent {
  const newId = dependencies.newId();
  return {
    event_id: newId,
    idempotency_key: `decision:${newId}`,
    instance_id: target.instanceId,
    model_revision_sha256: target.modelRevisionSha256,
    semantic_sha256: target.semanticSha256,
    disposition,
    ...(rationale.trim() ? { rationale: rationale.trim().slice(0, 500) } : {}),
    created_at: dependencies.now(),
    ...(active ? { supersedes_event_id: active.event_id } : {}),
  };
}

/** Check active context for a multi-label conflict or a repeated identical label. */
function decisionBlock(
  events: readonly FeedbackEvent[],
  target: FeedbackTarget,
  disposition: "accepted_use_case" | "unintended_behavior",
): FeedbackChange | null {
  const active = activeDecisionFor(events, target);
  if (active?.disposition === disposition) {
    return { kind: "noop", reason: "That use-case label is already active." };
  }
  const key = `${target.modelRevisionSha256}|${target.semanticSha256}`;
  const context = activeDecisions(events).filter((event) => contextKey(event) === key);
  const hasOpposite = context.some((event) => event.disposition !== disposition);
  if (hasOpposite && context.length > 1) {
    return {
      kind: "conflict",
      reason: "Resolve the conflicting active labels before adding another decision.",
    };
  }
  return null;
}

/** Build one event after checking active context for conflicts and idempotence. */
export function proposeDecision(
  events: readonly FeedbackEvent[],
  target: FeedbackTarget,
  disposition: "accepted_use_case" | "unintended_behavior",
  rationale: string,
  dependencies: EventDependencies = defaultEventDependencies,
): FeedbackChange {
  const blocked = decisionBlock(events, target, disposition);
  if (blocked) return blocked;
  const active = activeDecisionFor(events, target);
  const event = decisionEvent(target, disposition, rationale, active, dependencies);
  return { kind: "appended", events: [...events, event] };
}

/** Build one withdrawal event that preserves the superseded event's binding. */
function withdrawalFor(target: FeedbackEvent, dependencies: EventDependencies): FeedbackEvent {
  const id = dependencies.newId();
  return {
    event_id: id,
    idempotency_key: `withdraw:${target.event_id}:${id}`,
    instance_id: target.instance_id,
    model_revision_sha256: target.model_revision_sha256,
    semantic_sha256: target.semantic_sha256,
    disposition: "withdrawn",
    created_at: dependencies.now(),
    supersedes_event_id: target.event_id,
  };
}

/** Build withdrawal events for explicit conflict resolution or target undo. */
export function proposeWithdrawals(
  events: readonly FeedbackEvent[],
  targetEventIds: readonly string[],
  dependencies: EventDependencies = defaultEventDependencies,
): FeedbackChange {
  const active = activeEvents(events);
  const targets = active.filter((event) => targetEventIds.includes(event.event_id));
  if (targets.length === 0) return { kind: "noop", reason: "There is no active label to withdraw." };
  return {
    kind: "appended",
    events: [...events, ...targets.map((target) => withdrawalFor(target, dependencies))],
  };
}

/** Serialize only the journal envelope; no provider configuration is reachable here. */
export function exportJournal(events: readonly FeedbackEvent[], now: string = new Date().toISOString()): string {
  const envelope: FeedbackJournalEnvelope = {
    schema: JOURNAL_SCHEMA,
    exported_at: now,
    events: [...events],
  };
  return JSON.stringify(envelope, null, 2);
}

/** Parse and validate a feedback journal JSON string. */
export function parseJournal(text: string): readonly FeedbackEvent[] {
  let raw: unknown;
  try {
    raw = JSON.parse(text);
  } catch {
    throw new Error("Journal is not valid JSON");
  }
  if (typeof raw !== "object" || raw === null || Array.isArray(raw)) throw new Error("Journal must be an object");
  const object = raw as Record<string, unknown>;
  if (object["schema"] !== JOURNAL_SCHEMA) throw new Error("Unsupported feedback journal schema version");
  if (!Array.isArray(object["events"])) throw new Error("Journal events must be an array");
  if (typeof object["exported_at"] !== "string") throw new Error("Journal exported_at is required");
  const events = object["events"].map(parseFeedbackEvent);
  return events;
}

/** Merge imported events by event and idempotency identity, rejecting conflicting reuse. */
export function mergeJournal(existing: readonly FeedbackEvent[], incoming: readonly FeedbackEvent[]): ImportResult {
  const byEventId = new Map(existing.map((event) => [event.event_id, event]));
  const byIdempotency = new Map(existing.map((event) => [event.idempotency_key, event]));
  let added = 0;
  let duplicates = 0;
  for (const event of incoming) {
    const sameId = byEventId.get(event.event_id);
    const sameKey = byIdempotency.get(event.idempotency_key);
    if (sameId || sameKey) {
      if (canonicalJson(sameId ?? sameKey) !== canonicalJson(event)) {
        return {
          ok: false,
          message: "An imported event reuses an identity with different content.",
        };
      }
      duplicates += 1;
      continue;
    }
    byEventId.set(event.event_id, event);
    byIdempotency.set(event.idempotency_key, event);
    added += 1;
  }
  return { ok: true, events: [...byEventId.values()], added, duplicates };
}

/** Read a stored journal; a parse failure is surfaced and is never silently overwritten. */
export function readStoredJournal(storage: JournalStorage): {
  events: readonly FeedbackEvent[];
  error: string | null;
} {
  try {
    const stored = storage.getItem(JOURNAL_STORAGE_KEY);
    if (!stored) return { events: [], error: null };
    return { events: parseJournal(stored), error: null };
  } catch (error) {
    return {
      events: [],
      error: error instanceof Error ? error.message : "Stored feedback journal could not be read.",
    };
  }
}

/** Persist the journal, returning a storage error instead of pretending it was saved. */
export function writeStoredJournal(storage: JournalStorage, events: readonly FeedbackEvent[]): string | null {
  try {
    storage.setItem(JOURNAL_STORAGE_KEY, exportJournal(events));
    return null;
  } catch (error) {
    return error instanceof Error ? error.message : "Feedback journal could not be saved.";
  }
}
