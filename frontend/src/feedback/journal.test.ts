import { describe, expect, it } from "vitest";
import {
  activeDecisionFor,
  activeDecisions,
  detectConflicts,
  exportJournal,
  mergeJournal,
  parseJournal,
  proposeDecision,
  proposeWithdrawals,
  readStoredJournal,
  staleDecisions,
  writeStoredJournal,
  type EventDependencies,
  type FeedbackEvent,
  type FeedbackTarget,
  type JournalStorage,
} from "./journal";

const target: FeedbackTarget = {
  instanceId: "a".repeat(64),
  modelRevisionSha256: "b".repeat(64),
  semanticSha256: "c".repeat(64),
};

function deps(): EventDependencies {
  let count = 0;
  return {
    now: () => `2026-01-0${Math.min(9, count + 1)}T12:00:00.000Z`,
    newId: () => `event-${++count}`,
  };
}

function appendDecision(
  events: readonly FeedbackEvent[],
  disposition: "accepted_use_case" | "unintended_behavior",
  dependencies: EventDependencies = deps(),
): FeedbackEvent {
  const change = proposeDecision(events, target, disposition, "", dependencies);
  if (change.kind !== "appended") throw new Error(change.reason);
  return change.events[change.events.length - 1]!;
}

function envelope(events: readonly FeedbackEvent[]): string {
  return JSON.stringify({
    schema: "sqlolla.feedback_journal.v1",
    exported_at: "2026-01-01T00:00:00.000Z",
    events,
  });
}

describe("feedback journal", () => {
  it("labels a decision, suppresses repeated identical intent, and keeps immutable identity", () => {
    const dependencies = deps();
    const change = proposeDecision([], target, "accepted_use_case", "  preserve duplicate handling  ", dependencies);
    expect(change.kind).toBe("appended");
    if (change.kind !== "appended") return;
    const event = change.events[0]!;
    expect(event.disposition).toBe("accepted_use_case");
    expect(event.rationale).toBe("preserve duplicate handling");
    expect(event.instance_id).toBe(target.instanceId);
    expect(event.model_revision_sha256).toBe(target.modelRevisionSha256);
    expect(event.semantic_sha256).toBe(target.semanticSha256);
    expect(event.supersedes_event_id).toBeUndefined();
    expect(proposeDecision(change.events, target, "accepted_use_case", "", dependencies).kind).toBe("noop");
    expect(activeDecisionFor(change.events, target)?.disposition).toBe("accepted_use_case");
  });

  it("changes intent by appending a superseding decision and undo by appending a withdrawal", () => {
    const dependencies = deps();
    const first = appendDecision([], "accepted_use_case", dependencies);
    const changed = proposeDecision([first], target, "unintended_behavior", "wrong tenant", dependencies);
    expect(changed.kind).toBe("appended");
    if (changed.kind !== "appended") return;
    const second = changed.events[changed.events.length - 1]!;
    expect(second.supersedes_event_id).toBe(first.event_id);
    expect(activeDecisionFor(changed.events, target)?.disposition).toBe("unintended_behavior");
    const undo = proposeWithdrawals(changed.events, [second.event_id], dependencies);
    expect(undo.kind).toBe("appended");
    if (undo.kind !== "appended") return;
    const withdrawal = undo.events[undo.events.length - 1]!;
    expect(withdrawal.disposition).toBe("withdrawn");
    expect(withdrawal.supersedes_event_id).toBe(second.event_id);
    expect(activeDecisionFor(undo.events, target)).toBeUndefined();
    expect(undo.events).toHaveLength(3);
    expect(proposeWithdrawals(undo.events, [second.event_id], dependencies).kind).toBe("noop");
  });

  it("detects contradictory active labels and stale revision feedback", () => {
    const firstDeps = deps();
    const accepted = appendDecision([], "accepted_use_case", firstDeps);
    const rejected = appendDecision([], "unintended_behavior", deps());
    const events = [accepted, rejected];
    const conflicts = detectConflicts(events);
    expect(conflicts).toHaveLength(1);
    expect(conflicts[0]?.accepted[0]?.event_id).toBe(accepted.event_id);
    expect(conflicts[0]?.rejected[0]?.event_id).toBe(rejected.event_id);
    expect(activeDecisions(events)).toHaveLength(2);
    expect(staleDecisions(events, "d".repeat(64))).toHaveLength(2);
    expect(staleDecisions(events, target.modelRevisionSha256)).toHaveLength(0);
  });

  it("round-trips only journal events through export and import", () => {
    const accepted = appendDecision([], "accepted_use_case", deps());
    const text = exportJournal([accepted], "2026-02-01T00:00:00.000Z");
    const parsed = parseJournal(text);
    expect(parsed).toEqual([accepted]);
    const merged = mergeJournal([accepted], parsed);
    expect(merged.ok).toBe(true);
    if (merged.ok) {
      expect(merged.added).toBe(0);
      expect(merged.duplicates).toBe(1);
    }
    expect(text).not.toContain("apiKey");
    expect(text).not.toContain("sk-live");
  });

  it("rejects unsupported schema versions, secret-bearing fields and reused identities", () => {
    const event = appendDecision([], "accepted_use_case");
    expect(() => parseJournal("{}")).toThrow();
    expect(() => parseJournal("not-json")).toThrow();
    expect(() => parseJournal(JSON.stringify({ schema: "sqlolla.feedback_journal.v2", exported_at: "x", events: [] }))).toThrow();
    expect(() => parseJournal(envelope([{ ...event, apiKey: "sk-live-secret" } as FeedbackEvent]))).toThrow();
    const different = { ...event, disposition: "unintended_behavior" as const };
    const collision = mergeJournal([event], [different]);
    expect(collision.ok).toBe(false);
    if (!collision.ok) expect(collision.message).toContain("identity");
    expect(() =>
      parseJournal(
        JSON.stringify({
          schema: "sqlolla.feedback_journal.v1",
          exported_at: "x",
          events: [{ ...event, created_at: "not-a-date" }],
        }),
      ),
    ).toThrow();
  });

  it("surfaces corrupted local storage without silently replacing it", () => {
    const storage: JournalStorage = {
      getItem: () => "not json",
      setItem: () => undefined,
    };
    const loaded = readStoredJournal(storage);
    expect(loaded.error).not.toBeNull();
    expect(loaded.events).toEqual([]);
    const event = appendDecision([], "accepted_use_case");
    expect(writeStoredJournal(storage, [event])).toBeNull();
    expect(() => parseJournal("")).toThrow();
  });
});
