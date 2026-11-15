import SQLollaM2.Extensions
import SQLolla.Truth

/-! Query composition preserves SQL cardinality, existential and NULL distinctions. -/

namespace SQLollaM2

/-- Scalar subqueries distinguish empty input, one NULL and too many occurrences. -/
def scalarSubquery : List (Option α) → Evaluation (Option α)
  | [] => .ok none
  | [value] => .ok value
  | _ :: _ :: _ => .error ⟨.cardinalityViolation, .statement, none, none⟩

/-- Empty scalar subqueries return a typed NULL at the binder's declared output type. -/
theorem scalar_empty : scalarSubquery (α := α) [] = .ok none := rfl

/-- Duplicate rows still trigger the scalar-subquery cardinality error. -/
theorem scalar_duplicates (value : Option α) :
    scalarSubquery [value, value] =
      .error ⟨.cardinalityViolation, .statement, none, none⟩ := rfl

/-- EXISTS examines row presence, never the NULLness of the projected value. -/
def existsSubquery (rows : List α) : Bool := !rows.isEmpty

/-- A one-row NULL result satisfies EXISTS. -/
theorem exists_null : existsSubquery [none (α := Int)] = true := rfl

/-- Empty EXISTS is FALSE, without an UNKNOWN branch. -/
theorem exists_empty : existsSubquery ([] : List Int) = false := rfl

/-- SQL equality over nullable mathematical integers preserves UNKNOWN. -/
def nullableEqual : Option Int → Option Int → SQLolla.Truth
  | some left, some right => if left = right then .yes else .no
  | _, _ => .unknown

/-- ANY/IN folds comparison truth values with SQL disjunction. -/
def anyTruth : List SQLolla.Truth → SQLolla.Truth
  | [] => .no
  | .yes :: _ => .yes
  | .no :: rest => anyTruth rest
  | .unknown :: rest => match anyTruth rest with
    | .yes => .yes
    | _ => .unknown

/-- ALL folds comparison truth values with SQL conjunction. -/
def allTruth : List SQLolla.Truth → SQLolla.Truth
  | [] => .yes
  | .no :: _ => .no
  | .yes :: rest => allTruth rest
  | .unknown :: rest => match allTruth rest with
    | .no => .no
    | _ => .unknown

/-- Empty ANY is FALSE while empty ALL is TRUE, including nullable left operands. -/
theorem quantified_empty : anyTruth [] = .no ∧ allTruth [] = .yes := ⟨rfl, rfl⟩

/-- A true comparison dominates an earlier NULL comparison for ANY. -/
theorem any_null_then_true : anyTruth [.unknown, .yes] = .yes := rfl

/-- A false comparison dominates an earlier NULL comparison for ALL. -/
theorem all_null_then_false : allTruth [.unknown, .no] = .no := rfl

/-- IN with no matching row and a NULL candidate remains UNKNOWN. -/
theorem in_null_nonmatch :
    anyTruth ([some 2, none].map (nullableEqual (some 1))) = .unknown := rfl

/-- NULL left operands do not turn an empty IN list into UNKNOWN. -/
theorem null_in_empty : anyTruth (([] : List (Option Int)).map (nullableEqual none)) = .no := rfl

/-- Group duplicate identity treats two NULL keys as the same key. -/
def groupKeyEqual (left right : Option Int) : Bool := decide (left = right)

/-- SQL duplicate/group equality differs from SQL nullable predicate equality. -/
theorem null_grouping_differs :
    groupKeyEqual none none = true ∧ nullableEqual none none = .unknown := ⟨rfl, rfl⟩

/-- A keyed group keeps every occurrence with that key, including duplicate rows. -/
def groupAt (key : α → Option Int) (value : Option Int) (rows : List α) : List α :=
  rows.filter fun row => groupKeyEqual (key row) value

/-- Duplicate input occurrences are not deduplicated by grouping. -/
theorem group_keeps_duplicates : groupAt (fun x : Int => some x) (some 1) [1, 1, 2] = [1, 1] := rfl

/-- Correlation binds each outer row explicitly before evaluating its inner query. -/
def correlated (inner : Outer → Evaluation Inner) (outer : Outer) : Evaluation Inner := inner outer

/-- Correlation cannot substitute a different outer occurrence's environment. -/
theorem correlation_environment (inner : Outer → Evaluation Inner) (outer : Outer) :
    correlated inner outer = inner outer := rfl

end SQLollaM2
