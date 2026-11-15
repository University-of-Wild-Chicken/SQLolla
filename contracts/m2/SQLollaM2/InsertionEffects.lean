import SQLollaM2.SequenceEffects

/-! Per-proposal insertion preserves nontransactional prefixes and stops after the first SQL failure. -/
namespace SQLollaM2.InsertionEffects
open SequenceEffects

/-- Each proposal receives the state and accumulated candidate of its own preceding branch. -/
def insertRows (step : Row → Candidate → State → Branch Candidate) :
    List Row → Branch Candidate → Branch Candidate
  | [], branch => branch
  | row :: rest, branch => match branch.outcome with
    | .error _ => branch
    | .ok _ => insertRows step rest (continueBranch branch (step row))

/-- No proposal means no new allocation, error or candidate change. -/
theorem empty_insert (step : Row → Candidate → State → Branch Candidate) (branch : Branch Candidate) :
    insertRows step [] branch = branch := rfl

/-- Failure stops subsequent proposals without removing the already observed allocation prefix. -/
theorem failed_insert_stops (step : Row → Candidate → State → Branch Candidate) (rows : List Row)
    (state : State) (events : List Int) (failure : SequenceEffects.Failure) :
    insertRows step rows ⟨state, events, .error failure⟩ = ⟨state, events, .error failure⟩ := by
  cases rows <;> rfl

/-- A successful first proposal's candidate and state feed the next proposal. -/
theorem insert_head (step : Row → Candidate → State → Branch Candidate) (row : Row)
    (rest : List Row) (state : State) (events : List Int) (candidate : Candidate) :
    insertRows step (row :: rest) ⟨state, events, .ok candidate⟩ =
      insertRows step rest (continueBranch ⟨state, events, .ok candidate⟩ (step row)) := rfl

/-- Buffered RETURNING rows are published only after the whole statement succeeds. -/
def publishReturning : Except SequenceEffects.Failure (List Row) → List Row
  | .ok rows => rows
  | .error _ => []

/-- A rejected statement cannot expose buffered successes as completed RETURNING observations. -/
theorem failed_returning_empty (failure : SequenceEffects.Failure) :
    publishReturning (Row := Row) (.error failure) = [] := rfl

/-- DEFAULT VALUES denotes one default-filled proposal, even with no supplied expressions. -/
def defaultValues : List (List α) := [[]]

/-- Its multiplicity is one rather than the zero proposals of an empty SELECT. -/
theorem default_values_one : (defaultValues (α := α)).length = 1 := rfl

/-- Planning applicable defaults precedes all source execution and allocation. -/
def plannedInsert (planning : Except SequenceEffects.Failure Unit)
    (execute : Unit → Branch Candidate) (initial : State) : Branch Candidate :=
  match planning with
  | .error failure => ⟨initial, [], .error failure⟩
  | .ok () => execute ()

/-- A constant default's planning failure preserves the initial state and allocates nothing. -/
theorem planning_failure_no_allocation (failure : SequenceEffects.Failure)
    (execute : Unit → Branch Candidate) (initial : State) :
    plannedInsert (.error failure) execute initial = ⟨initial, [], .error failure⟩ := rfl

/-- Successful planning admits the same runtime branch semantics without extra events. -/
theorem planning_success_executes (execute : Unit → Branch Candidate) (initial : State) :
    plannedInsert (.ok ()) execute initial = execute () := rfl

end SQLollaM2.InsertionEffects
