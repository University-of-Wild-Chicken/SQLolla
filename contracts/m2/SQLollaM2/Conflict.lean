import SQLollaM2.Outcome

/-! Single-candidate conflict decisions retain proposed-row checks and cardinality failures.
Arbiter inference, partial-index predicates and physical tuple identity remain explicit inputs.
No theorem here establishes refinement of the Rust implementation to PostgreSQL. -/

namespace SQLollaM2

/-- A conflict action cannot be confused with ordinary insertion. -/
inductive ConflictAction where
  | nothing | update
  deriving DecidableEq, Repr

/-- Distinguish a skipped input occurrence from an inserted or updated result row. -/
inductive UpsertDecision (Row : Type) where
  | skipped | inserted (row : Row) | updated (row : Row)
  deriving DecidableEq, Repr

/-- The excluded proposal and existing row are separate inputs to simultaneous assignments. -/
def resolveConflict (action : ConflictAction) (existing : Option Row) (proposed : Row)
    (predicate : Row → Row → SQLolla.Truth)
    (assign : Row → Row → Evaluation Row) : Evaluation (UpsertDecision Row) :=
  match existing, action with
  | none, _ => .ok (.inserted proposed)
  | some _, .nothing => .ok .skipped
  | some old, .update =>
    if (predicate old proposed).keepsRow then
      match assign old proposed with
      | .ok result => .ok (.updated result)
      | .error failure => .error failure
    else .ok .skipped

/-- DO NOTHING still inserts when there is no inferred unique conflict. -/
theorem conflict_free_insert (proposed : Row) (predicate) (assign) :
    resolveConflict .nothing none proposed predicate assign = .ok (.inserted proposed) := rfl

/-- A conflicting DO NOTHING occurrence contributes no inserted/returned row. -/
theorem conflicting_nothing_skips (old proposed : Row) (predicate) (assign) :
    resolveConflict .nothing (some old) proposed predicate assign = .ok .skipped := rfl

/-- UNKNOWN in a conflict UPDATE WHERE does not select the conflicting row. -/
theorem conflict_unknown_skips (old proposed : Row) (assign) :
    resolveConflict .update (some old) proposed (fun _ _ => .unknown) assign =
      .ok .skipped := rfl

/-- New-row NOT NULL/CHECK failures must survive even when uniqueness would skip the row. -/
def checkedConflict (check : Row → Evaluation Unit) (proposed : Row)
    (resolve : Evaluation (UpsertDecision Row)) : Evaluation (UpsertDecision Row) :=
  match check proposed with
  | .ok () => resolve
  | .error failure => .error failure

/-- A proposed-row failure cannot be swallowed by DO NOTHING. -/
theorem conflict_check_error (check : Row → Evaluation Unit) (proposed : Row)
    (failure : Failure) (failed : check proposed = .error failure) :
    checkedConflict check proposed (.ok .skipped) = .error failure := by
  simp [checkedConflict, failed]

/-- A statement must not affect the same target occurrence twice through ON CONFLICT UPDATE. -/
def conflictCardinality (alreadyAffected : Bool) : Evaluation Unit :=
  if alreadyAffected then .error ⟨.cardinalityViolation, .statement, none, none⟩ else .ok ()

/-- Repeated target modification produces 21000, not a second successful update. -/
theorem repeated_conflict_is_error :
    conflictCardinality true = .error ⟨.cardinalityViolation, .statement, none, none⟩ := rfl

/-- Skipping an occurrence must not increment the affected-row count. -/
def UpsertDecision.affected : UpsertDecision Row → Nat
  | .skipped => 0 | .inserted _ => 1 | .updated _ => 1

/-- Counts distinguish skipped conflicts from matched updates, including unchanged values. -/
theorem upsert_count_contract (row : Row) :
    (UpsertDecision.skipped : UpsertDecision Row).affected = 0 ∧
    (UpsertDecision.updated row).affected = 1 := ⟨rfl, rfl⟩

end SQLollaM2
