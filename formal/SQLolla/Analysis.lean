import SQLolla.Transaction

/-!
# Analysis claims

`Case` includes a prestate, parameters and context in a later instantiation.
The specification quantifies over every execution outcome. A no-counterexample
claim needs domain soundness, execution reachability and a completed proof.
It says nothing about cases outside its domain.
-/

namespace SQLolla

/-- An execution contract separates admissibility, possible outcomes and failure. -/
structure Contract (Case Outcome : Type) where
  admissible : Case → Prop
  executes : Case → Outcome → Prop
  bad : Case → Outcome → Prop

/-- A named finite analysis domain; exact capacity evidence is still required. -/
structure Domain (Case : Type) where
  contains : Case → Prop
  finiteSupport : List Case
  exhaustive : ∀ input, contains input → input ∈ finiteSupport

/-- A counterexample includes all assumptions and an actual modeled outcome. -/
def Counterexample (contract : Contract Case Outcome) (domain : Domain Case) : Prop :=
  ∃ input output, domain.contains input ∧ contract.admissible input ∧
    contract.executes input output ∧ contract.bad input output

/-- Assumption consistency is independent of whether a failure is found. -/
def AssumptionsExist (contract : Contract Case Outcome) (domain : Domain Case) : Prop :=
  ∃ input, domain.contains input ∧ contract.admissible input

/-- A usable model has at least one reachable outcome from an admissible input. -/
def ExecutionExists (contract : Contract Case Outcome) (domain : Domain Case) : Prop :=
  ∃ input output, domain.contains input ∧ contract.admissible input ∧
    contract.executes input output

/-- Total modeled coverage rules out silently dropping an admissible input. -/
def AllInputsCovered (contract : Contract Case Outcome) (domain : Domain Case) : Prop :=
  ∀ input, domain.contains input → contract.admissible input →
    ∃ output, contract.executes input output

/-- Preservation is only over the specified input domain and modeled outcomes. -/
def BoundedSafe (contract : Contract Case Outcome) (domain : Domain Case) : Prop :=
  ∀ input output, domain.contains input → contract.admissible input →
    contract.executes input output → ¬ contract.bad input output

/-- No counterexample and bounded preservation are logically equivalent. -/
theorem no_counterexample_iff (contract : Contract Case Outcome) (domain : Domain Case) :
    (¬ Counterexample contract domain) ↔ BoundedSafe contract domain := by
  constructor
  · intro absent input output within admissible executes bad
    exact absent ⟨input, output, within, admissible, executes, bad⟩
  · intro safe witness
    obtain ⟨input, output, within, admissible, executes, bad⟩ := witness
    exact safe input output within admissible executes bad

/-- A genuine reachable outcome establishes assumption consistency. -/
theorem execution_implies_assumptions (contract : Contract Case Outcome)
    (domain : Domain Case) (reachable : ExecutionExists contract domain) :
    AssumptionsExist contract domain := by
  obtain ⟨input, _, within, admissible, _⟩ := reachable
  exact ⟨input, within, admissible⟩

/-- Contradictory assumptions yield vacuous absence of counterexamples. -/
theorem inconsistent_is_vacuous (contract : Contract Case Outcome)
    (domain : Domain Case) (empty : ¬ AssumptionsExist contract domain) :
    ¬ Counterexample contract domain := by
  intro witness
  obtain ⟨input, _, within, admissible, _, _⟩ := witness
  exact empty ⟨input, within, admissible⟩

/-- A proof-bearing bounded claim requires nonvacuity and input coverage. -/
structure BoundedClaim (contract : Contract Case Outcome) (domain : Domain Case) : Prop where
  assumptions : AssumptionsExist contract domain
  reachable : ExecutionExists contract domain
  allInputs : AllInputsCovered contract domain
  safe : BoundedSafe contract domain

/-- Transport of safety to a smaller domain does not prove its nonvacuity. -/
theorem safe_on_subdomain (contract : Contract Case Outcome) (large small : Domain Case)
    (subset : ∀ input, small.contains input → large.contains input)
    (safe : BoundedSafe contract large) : BoundedSafe contract small := by
  intro input output within admissible executes
  exact safe input output (subset input within) admissible executes

/-- Operational results cannot convert an unknown result into a bounded claim. -/
inductive SearchResult where
  | sat
  | unsat
  | unknown
  | failure
  deriving DecidableEq, Repr

/-- User-facing summaries preserve every inconclusive stage. -/
inductive Summary where
  | counterexampleCandidate
  | noCounterexampleWithinBounds
  | inconsistentAssumptions
  | missingExecution
  | inconclusive
  deriving DecidableEq, Repr

/-- Classify solver stages only. This creates no BoundedClaim or encoding proof. -/
def summarize : SearchResult → SearchResult → SearchResult → Bool → Summary
  | .unsat, _, _, _ => .inconsistentAssumptions
  | .sat, .unsat, _, _ => .missingExecution
  | .sat, .sat, .sat, _ => .counterexampleCandidate
  | .sat, .sat, .unsat, true => .noCounterexampleWithinBounds
  | _, _, _, _ => .inconclusive

/-- Timeout/unknown in the failure search never produces a successful summary. -/
theorem unknown_is_inconclusive (coverage : Bool) :
    summarize .sat .sat .unknown coverage = .inconclusive := rfl

/-- An UNSAT failure search is insufficient when scope coverage is missing. -/
theorem uncovered_is_inconclusive :
    summarize .sat .sat .unsat false = .inconclusive := rfl

end SQLolla
