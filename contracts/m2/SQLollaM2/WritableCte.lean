import SQLollaM2.Outcome

/-! Writable CTEs share the statement's original database snapshot.
RETURNING relations carry inter-CTE data; pending writes are published atomically.
Overlapping writes and volatile evaluation order need separate admission/coverage obligations. -/

namespace SQLollaM2

/-- A statement snapshot, materialized named results and pending effects have different roles. -/
structure CteFrame (Name Snapshot Relation Delta : Type) where
  snapshot : Snapshot
  returning : Name → Option Relation
  pending : List Delta

/-- Recording a CTE exposes its RETURNING relation without replacing the read snapshot. -/
def recordCte [DecidableEq Name] (frame : CteFrame Name Snapshot Relation Delta)
    (name : Name) (rows : Relation) (delta : Delta) : CteFrame Name Snapshot Relation Delta :=
  { snapshot := frame.snapshot
    returning := fun key => if key = name then some rows else frame.returning key
    pending := frame.pending ++ [delta] }

/-- Later base-table reads cannot accidentally see a preceding CTE's unpublished write. -/
theorem writable_cte_snapshot_frame [DecidableEq Name]
    (frame : CteFrame Name Snapshot Relation Delta) (name : Name) (rows : Relation) (delta : Delta) :
    (recordCte frame name rows delta).snapshot = frame.snapshot := rfl

/-- Inter-CTE communication uses the named RETURNING relation. -/
theorem writable_cte_returning_visible [DecidableEq Name]
    (frame : CteFrame Name Snapshot Relation Delta) (name : Name) (rows : Relation) (delta : Delta) :
    (recordCte frame name rows delta).returning name = some rows := by
  simp [recordCte]

/-- Publishing one name cannot overwrite another CTE's materialized result. -/
theorem writable_cte_name_frame [DecidableEq Name]
    (frame : CteFrame Name Snapshot Relation Delta) (name other : Name)
    (rows : Relation) (delta : Delta) (different : other ≠ name) :
    (recordCte frame name rows delta).returning other = frame.returning other := by
  simp [recordCte, different]

/-- An unreferenced writable CTE still executes; read-only CTE demand is a separate rule. -/
def executeWritable (_referenced : Bool) (run : Snapshot → Evaluation Delta)
    (snapshot : Snapshot) : Evaluation Delta := run snapshot

/-- Dropping an unreferenced writable CTE would erase an observable effect. -/
theorem unreferenced_write_executes (run : Snapshot → Evaluation Delta) (snapshot : Snapshot) :
    executeWritable false run snapshot = run snapshot := rfl

/-- Final publication checks the composed candidate; failures expose no partial candidate. -/
def publishCtes (apply : Snapshot → List Delta → Evaluation Snapshot)
    (validate : Snapshot → Evaluation Unit) (frame : CteFrame Name Snapshot Relation Delta)
    (affected : Nat) (output : List Row) : StatementResult Snapshot Row :=
  match apply frame.snapshot frame.pending with
  | .error failure => .rejected failure []
  | .ok after => enforceCandidate validate ⟨after, affected, output⟩

/-- Failed write composition rejects the whole statement even after earlier CTEs produced results. -/
theorem cte_failure_atomic (apply : Snapshot → List Delta → Evaluation Snapshot)
    (validate : Snapshot → Evaluation Unit) (frame : CteFrame Name Snapshot Relation Delta)
    (affected : Nat) (output : List Row) (failure : Failure)
    (failed : apply frame.snapshot frame.pending = .error failure) :
    publishCtes apply validate frame affected output = .rejected failure [] := by
  simp [publishCtes, failed]

/-- Independent patches may be composed in either order only under an explicit commutation proof. -/
def IndependentWrites (first second : Snapshot → Snapshot) : Prop :=
  ∀ snapshot, first (second snapshot) = second (first snapshot)

/-- Independence permits order exchange; it cannot be inferred merely from distinct CTE names. -/
theorem independent_write_exchange (first second : Snapshot → Snapshot)
    (independent : IndependentWrites first second) (snapshot : Snapshot) :
    first (second snapshot) = second (first snapshot) := independent snapshot

end SQLollaM2
