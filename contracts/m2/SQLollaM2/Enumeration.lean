import SQLollaM2.FiniteSearch
import SQLolla.Instance

/-! Durable evidence and enumeration boundaries. Hash collision resistance, filesystem
atomicity and solver scheduling are engineering assumptions, not theorems here. -/
namespace SQLollaM2

/-- An example run and a property check retain distinct evidence obligations. -/
inductive EvidenceCommand where
  | runExample
  | checkCounterexample
  deriving DecidableEq, Repr

/-- Resource refusal and engine failure cannot be confused with solver exhaustion. -/
inductive EnumerationStep (Instance : Type) where
  | yielded (witness : Instance)
  | exhausted
  | limited
  | failed (diagnostic : String)
  deriving DecidableEq, Repr

/-- Only a confirmed terminal solver step establishes enumeration exhaustion. -/
def exhausts (step : EnumerationStep Instance) : Bool :=
  match step with
  | .exhausted => true
  | _ => false

/-- A requested capacity boundary is not evidence that no further instance exists. -/
theorem limit_not_exhaustion : exhausts (EnumerationStep.limited : EnumerationStep Unit) = false := rfl

/-- Solver and persistence errors never establish absence of counterexamples. -/
theorem error_not_exhaustion (diagnostic : String) :
    exhausts (EnumerationStep.failed diagnostic : EnumerationStep Unit) = false := rfl

/-- One yielded example alone cannot establish exhaustion. -/
theorem yield_not_exhaustion (witness : Instance) : exhausts (.yielded witness) = false := rfl

/-- Typed persisted evidence retains the complete model, context, command and observation. -/
structure EvidenceRecord (Model Context Observation : Type) where
  model : Model
  context : Context
  command : EvidenceCommand
  observation : Observation

/-- Replay validates admissibility and adds violation evidence only for check commands. -/
def ReplayValid (admissible violates : Observation → Prop)
    (record : EvidenceRecord Model Context Observation) : Prop :=
  admissible record.observation ∧
    (record.command = .checkCounterexample → violates record.observation)

/-- An archived counterexample retains a violation of the same archived observation. -/
theorem replayed_counterexample (admissible violates : Observation → Prop)
    (record : EvidenceRecord Model Context Observation)
    (valid : ReplayValid admissible violates record)
    (checked : record.command = .checkCounterexample) : violates record.observation :=
  valid.2 checked

/-- Every yielded kind, including a satisfying example, requires semantic replay. -/
theorem replayed_example_admissible (admissible violates : Observation → Prop)
    (record : EvidenceRecord Model Context Observation)
    (valid : ReplayValid admissible violates record) : admissible record.observation := valid.1

/-- Resume accepts a regenerated prefix only when every retained observation agrees. -/
def PrefixVerified (saved regenerated : List Observation) : Prop := saved = regenerated

/-- A verified prefix cannot silently replace a previously retained observation. -/
theorem verified_prefix_membership (saved regenerated : List Observation)
    (verified : PrefixVerified saved regenerated) (value : Observation) :
    value ∈ saved ↔ value ∈ regenerated := by rw [verified]

/-- Ordinals bind to the checked prefix length rather than an untrusted cursor request. -/
def CursorValid (saved : List Observation) (ordinal : Nat) : Prop := ordinal = saved.length

/-- A resumed cursor cannot skip an unverified prefix element. -/
theorem valid_cursor_exact_prefix (saved : List Observation) (ordinal : Nat)
    (valid : CursorValid saved ordinal) : ordinal = saved.length := valid

/-- Atomic publication exposes either the old committed journal or its complete next record. -/
def PublishedAppend (before after : List Record) (record : Record) : Prop :=
  after = before ∨ after = before ++ [record]

/-- A committed append preserves every previously committed record. -/
theorem published_append_preserves (before after : List Record) (record old : Record)
    (published : PublishedAppend before after record) (retained : old ∈ before) :
    old ∈ after := by
  rcases published with same | appended
  · simpa [same] using retained
  · simp [appended, retained]

end SQLollaM2
