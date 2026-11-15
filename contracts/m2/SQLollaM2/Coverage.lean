import SQLolla.Analysis
import SQLolla.Encoding

/-!
Reachability of one outcome per input is weaker than coverage of every outcome.
Reference execution below is a supplied relation; no PostgreSQL relation is built.
Transport is conditional on admission, outcome and bad-behavior correspondence.
-/

namespace SQLollaM2

/-- Include every reference outcome, including errors and permitted ordered-result ties. -/
def AllOutcomesCovered (reference modeled : SQLolla.Contract Case Outcome)
    (domain : SQLolla.Domain Case) : Prop :=
  ∀ input output, domain.contains input → reference.admissible input →
    reference.executes input output → modeled.executes input output

/-- Witness soundness requires each modeled outcome to be possible in the reference. -/
def OutcomesSound (reference modeled : SQLolla.Contract Case Outcome)
    (domain : SQLolla.Domain Case) : Prop :=
  ∀ input output, domain.contains input → modeled.admissible input →
    modeled.executes input output → reference.executes input output

/-- SQL-to-model admission and property meaning must be retained independently of execution. -/
structure MeaningAgrees (reference modeled : SQLolla.Contract Case Outcome) : Prop where
  admission : ∀ input, reference.admissible input ↔ modeled.admissible input
  bad : ∀ input output, reference.bad input output ↔ modeled.bad input output

/-- A modeled bounded safety theorem transfers only when every reference outcome is covered. -/
theorem safe_transfer (reference modeled : SQLolla.Contract Case Outcome)
    (domain : SQLolla.Domain Case) (meaning : MeaningAgrees reference modeled)
    (coverage : AllOutcomesCovered reference modeled domain)
    (safe : SQLolla.BoundedSafe modeled domain) : SQLolla.BoundedSafe reference domain := by
  intro input output within admitted executes bad
  exact safe input output within ((meaning.admission input).mp admitted)
    (coverage input output within admitted executes) ((meaning.bad input output).mp bad)

/-- Replayed modeled witnesses require reference soundness, not just finite input coverage. -/
theorem counterexample_transfer (reference modeled : SQLolla.Contract Case Outcome)
    (domain : SQLolla.Domain Case) (meaning : MeaningAgrees reference modeled)
    (sound : OutcomesSound reference modeled domain)
    (witness : SQLolla.Counterexample modeled domain) :
    SQLolla.Counterexample reference domain := by
  obtain ⟨input, output, within, admitted, executes, bad⟩ := witness
  exact ⟨input, output, within, (meaning.admission input).mpr admitted,
    sound input output within admitted executes, (meaning.bad input output).mpr bad⟩

/-- Strengthen M0 claims with explicit reference correspondence; none is manufactured by SAT. -/
structure CompleteBoundedClaim (reference modeled : SQLolla.Contract Case Outcome)
    (domain : SQLolla.Domain Case) : Prop where
  model : SQLolla.BoundedClaim modeled domain
  meaning : MeaningAgrees reference modeled
  sound : OutcomesSound reference modeled domain
  complete : AllOutcomesCovered reference modeled domain

/-- The strengthened contract implies reference safety within exactly the same finite domain. -/
theorem complete_claim_safe (reference modeled : SQLolla.Contract Case Outcome)
    (domain : SQLolla.Domain Case) (claim : CompleteBoundedClaim reference modeled domain) :
    SQLolla.BoundedSafe reference domain :=
  safe_transfer reference modeled domain claim.meaning claim.complete claim.model.safe

/-- One finite input suffices to expose the gap between input and outcome coverage. -/
def oneInput : SQLolla.Domain Unit :=
  ⟨fun _ => True, [()], fun input _ => by cases input; simp⟩

/-- Reference success and failure can both be possible for one input. -/
def twoOutcomes : SQLolla.Contract Unit Bool :=
  ⟨fun _ => True, fun _ _ => True, fun _ output => output = true⟩

/-- An unsoundly simplified evaluator keeps success and loses the failure branch. -/
def successOnly : SQLolla.Contract Unit Bool :=
  ⟨fun _ => True, fun _ output => output = false, fun _ output => output = true⟩

/-- M0 input reachability alone accepts the incomplete evaluator. -/
theorem input_coverage_is_insufficient : SQLolla.AllInputsCovered successOnly oneInput := by
  intro _ _ _
  exact ⟨false, rfl⟩

/-- Dropping failures gives a safe modeled relation despite a reference property violation. -/
theorem dropped_failure_appears_safe : SQLolla.BoundedSafe successOnly oneInput := by
  intro _ output _ _ success bad
  cases success.symm.trans bad

/-- The missing reference behavior is a concrete counterexample in the same finite domain. -/
theorem reference_failure_exists : SQLolla.Counterexample twoOutcomes oneInput := by
  exact ⟨(), true, True.intro, True.intro, True.intro, rfl⟩

/-- Complete-outcome coverage detects the omitted error/tie branch. -/
theorem dropped_failure_not_covered : ¬ AllOutcomesCovered twoOutcomes successOnly oneInput := by
  intro coverage
  have impossible := coverage () true True.intro True.intro True.intro
  cases impossible

/-- Admission correspondence transfers the model's independently established nonvacuity. -/
theorem assumptions_transfer (reference modeled : SQLolla.Contract Case Outcome)
    (domain : SQLolla.Domain Case) (meaning : MeaningAgrees reference modeled)
    (existsInput : SQLolla.AssumptionsExist modeled domain) :
    SQLolla.AssumptionsExist reference domain := by
  obtain ⟨input, within, admitted⟩ := existsInput
  exact ⟨input, within, (meaning.admission input).mpr admitted⟩

/-- Reference reachability follows only with soundness of the modeled execution relation. -/
theorem reachability_transfer (reference modeled : SQLolla.Contract Case Outcome)
    (domain : SQLolla.Domain Case) (meaning : MeaningAgrees reference modeled)
    (sound : OutcomesSound reference modeled domain)
    (reachable : SQLolla.ExecutionExists modeled domain) :
    SQLolla.ExecutionExists reference domain := by
  obtain ⟨input, output, within, admitted, executes⟩ := reachable
  exact ⟨input, output, within, (meaning.admission input).mpr admitted,
    sound input output within admitted executes⟩

/-- Model input reachability transfers separately from coverage of every reference outcome. -/
theorem input_coverage_transfer (reference modeled : SQLolla.Contract Case Outcome)
    (domain : SQLolla.Domain Case) (meaning : MeaningAgrees reference modeled)
    (sound : OutcomesSound reference modeled domain)
    (covered : SQLolla.AllInputsCovered modeled domain) :
    SQLolla.AllInputsCovered reference domain := by
  intro input within admitted
  have modelAdmission := (meaning.admission input).mp admitted
  obtain ⟨output, executes⟩ := covered input within modelAdmission
  exact ⟨output, sound input output within modelAdmission executes⟩

/-- Every inherited M0 claim obligation is retained in the strengthened M2 contract. -/
theorem complete_claim_preserves_m0 (reference modeled : SQLolla.Contract Case Outcome)
    (domain : SQLolla.Domain Case) (claim : CompleteBoundedClaim reference modeled domain) :
    SQLolla.BoundedClaim reference domain := by
  exact ⟨assumptions_transfer reference modeled domain claim.meaning claim.model.assumptions,
    reachability_transfer reference modeled domain claim.meaning claim.sound claim.model.reachable,
    input_coverage_transfer reference modeled domain claim.meaning claim.sound claim.model.allInputs,
    complete_claim_safe reference modeled domain claim⟩

end SQLollaM2
