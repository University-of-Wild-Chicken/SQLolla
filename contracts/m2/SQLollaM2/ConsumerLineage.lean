import SQLollaM2.Subquery

/-! Conditional occurrence-demand certificates. Operator lineage soundness and Rust refinement
remain separate obligations; these lemmas do not assume a PostgreSQL physical execution plan. -/
namespace SQLollaM2.ConsumerLineage

/-- Required producer occurrences are identities, not SQL-value equality classes. -/
def FullCoverage (required witnesses : List Nat) : Prop :=
  ∀ occurrence, occurrence ∈ required → occurrence ∈ witnesses

/-- A certificate may publish only a successful complete branch with nonvacuous observable demand. -/
def Certificate (succeeded : Bool) (required witnesses : List Nat) (events : List Event) : Prop :=
  succeeded = true ∧ (required ≠ [] ∨ events = []) ∧ FullCoverage required witnesses

/-- Operator soundness converts complete occurrence witnesses into actual producer consumption. -/
theorem coverage_forces_consumption (required witnesses : List Nat) (consumed : Nat → Prop)
    (coverage : FullCoverage required witnesses)
    (sound : ∀ occurrence, occurrence ∈ witnesses → consumed occurrence) :
    ∀ occurrence, occurrence ∈ required → consumed occurrence := by
  intro occurrence present
  exact sound occurrence (coverage occurrence present)

/-- Empty output cannot certify a nonempty local event trace through vacuous set coverage. -/
theorem empty_observable_producer_rejected (witnesses : List Nat) (event : Event)
    (events : List Event) : ¬ Certificate true [] witnesses (event :: events) := by
  simp [Certificate]

/-- Error branches need prefix demand semantics and cannot reuse success-only lineage evidence. -/
theorem error_consumer_rejected (required witnesses : List Nat) (events : List Event) :
    ¬ Certificate false required witnesses events := by
  simp [Certificate]

/-- Two equal-valued rows retain different occurrence identities; witnessing one cannot cover both. -/
theorem duplicate_values_do_not_collapse_identity : ¬ FullCoverage [0,1] [0] := by
  intro coverage
  have impossible := coverage 1 (by simp)
  simp at impossible

/-- Each changed target must depend on at most one source occurrence, including duplicate values. -/
def Unambiguous (relates : Nat → Nat → Prop) : Prop :=
  ∀ target first second, relates target first → relates target second → first = second

/-- A target with two distinct source identities cannot certify both as necessarily consumed. -/
theorem duplicate_match_rejected (relates : Nat → Nat → Prop) (target first second : Nat)
    (left : relates target first) (right : relates target second) (different : first ≠ second) :
    ¬ Unambiguous relates := by
  intro unique
  exact different (unique target first second left right)

/-- Backward demand propagation composes only conditional links whose consumer is itself required. -/
theorem demand_links_compose (root middle producer : Prop)
    (first : root → middle) (second : middle → producer) : root → producer := by
  intro required
  exact second (first required)

/-- A nonempty singleton occurrence list has complete coverage when its one identity is witnessed. -/
theorem singleton_coverage (occurrence : Nat) : FullCoverage [occurrence] [occurrence] := by
  intro value present
  exact present

/-- TRUE membership needs a TRUE comparison; NULL/UNKNOWN is never its positive witness. -/
theorem positive_membership_has_true_witness (values : List SQLolla.Truth)
    (accepted : SQLollaM2.anyTruth values = .yes) : .yes ∈ values := by
  induction values with
  | nil => cases accepted
  | cons head tail ih =>
    cases head with
    | yes => simp
    | no => exact List.mem_cons_of_mem _ (ih accepted)
    | unknown =>
      cases outcome : SQLollaM2.anyTruth tail with
      | yes => exact List.mem_cons_of_mem _ (ih outcome)
      | no => simp [SQLollaM2.anyTruth, outcome] at accepted
      | unknown => simp [SQLollaM2.anyTruth, outcome] at accepted

/-- A unique comparison witness fixes identity even when the consumer chooses its own scan order. -/
theorem unique_membership_forces_identity (compares consumed : Nat → Prop) (witness : Nat)
    (unique : ∀ occurrence, compares occurrence → occurrence = witness)
    (observed : ∃ occurrence, compares occurrence ∧ consumed occurrence) : consumed witness := by
  obtain ⟨occurrence, matched, demanded⟩ := observed
  have same := unique occurrence matched
  simpa only [same] using demanded

end SQLollaM2.ConsumerLineage
