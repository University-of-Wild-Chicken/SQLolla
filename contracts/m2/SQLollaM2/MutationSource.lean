import SQLollaM2.Crud

/-! FROM/USING chooses target occurrences independently from source multiplicity.
Ambiguous UPDATE choices require alternatives rather than an arbitrary first match. -/
namespace SQLollaM2

/-- Equal source choices may be collapsed without selecting between distinct observations. -/
def uniqueSource {Value : Type} [DecidableEq Value] (rows : List Value) : Option Value :=
  match rows with
  | [] => none
  | first :: rest => if rest.all (· == first) then some first else none

/-- A single matching source has exactly one admitted representative. -/
theorem unique_source_singleton {Value : Type} [DecidableEq Value] (value : Value) :
    uniqueSource [value] = some value := by simp [uniqueSource]

/-- Distinct source candidates cannot be collapsed to the first by this admission rule. -/
theorem unique_source_rejects_distinct {Value : Type} [DecidableEq Value]
    (first second : Value) (different : second ≠ first) :
    uniqueSource [first, second] = none := by simp [uniqueSource, different]

/-- DELETE selects a target once whenever at least one joined predicate succeeds. -/
def deleteSourceSelected (truths : List Bool) : Bool := truths.any id

/-- Repeating one successful source does not multiply deleted target occurrences. -/
theorem duplicate_source_deletes_once :
    deleteSourceSelected [true, true] = deleteSourceSelected [true] := rfl

/-- An empty USING relation leaves every target occurrence unselected. -/
theorem empty_source_keeps_target : deleteSourceSelected [] = false := rfl

end SQLollaM2
