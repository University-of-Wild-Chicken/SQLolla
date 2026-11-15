import SQLolla.IR
import SQLollaM2.Scalar

/-! Constant propagation requires syntactic singleton-row evidence, never a database sample.
The fragment proves substitution and error provenance; optimizer coverage remains separately tested. -/

namespace SQLollaM2

/-- Replace the inherited literal/column fragment by its known singleton-row value. -/
def specializeKnown (row : SQLolla.Row schema) (expr : SQLolla.ScalarExpr schema ty) :
    SQLolla.ScalarExpr schema ty := .literal (expr.eval row)

/-- Substitution preserves a typed value when evaluated at the evidenced row. -/
theorem singleton_substitution_preserves (row : SQLolla.Row schema)
    (expr : SQLolla.ScalarExpr schema ty) :
    (specializeKnown row expr).eval row = expr.eval row := rfl

/-- A literal singleton source supplies evidence independently of external database contents. -/
def singletonLiteral : List Row → Option Row
  | [row] => some row
  | _ => none

/-- Neither an empty bag nor multiple occurrences establishes the singleton lineage rule. -/
theorem singleton_literal_boundaries (a b : Row) (rest : List Row) :
    singletonLiteral ([] : List Row) = none ∧ singletonLiteral (a :: b :: rest) = none :=
  ⟨rfl, rfl⟩

/-- Equal-valued duplicate occurrences are still not syntactically a singleton source. -/
theorem duplicate_values_not_singleton (row : Row) :
    singletonLiteral [row, row] = none := rfl

/-- Planning provenance is attached by constant evaluation rather than guessed from SQLSTATE. -/
def planningResult : Evaluation Value → Evaluation Value
  | .ok value => .ok value
  | .error failure => .error { failure with phase := .planning }

/-- A constant SQL failure remains a failure after value propagation. -/
theorem propagated_error_retained (failure : Failure) :
    planningResult (Value := Value) (.error failure) =
      .error { failure with phase := .planning } := rfl

/-- Knowing a source row does not turn successful scalar computation into a different value. -/
theorem propagated_success_preserved (value : Value) :
    planningResult (.ok value) = .ok value := rfl

/-- A planned cell distinguishes a completed literal from an unevaluated computation. -/
inductive PlannedCell (α κ : Type) where
  | literal : α → PlannedCell α κ
  | deferred : κ → PlannedCell α κ

/-- Literal extraction never invokes a deferred computation or consults its context. -/
def extractLiteral : PlannedCell α κ → Option α
  | .literal value => some value
  | .deferred _ => none

/-- Extraction traverses every occurrence in order, preserving duplicates and cell identity. -/
def extractLiteralRows (rows : List (List (PlannedCell α κ))) : Option (List (List α)) :=
  rows.mapM (fun row => row.mapM extractLiteral)

/-- Every completed row extracts exactly, without changing its width or values. -/
theorem extract_literal_row (row : List α) :
    (row.map (PlannedCell.literal (κ := κ))).mapM extractLiteral = some row := by
  induction row with
  | nil => rfl
  | cons value rest ih => simp [List.mapM_cons, extractLiteral, ih]

/-- A literal VALUES bag is identical to its expression form, including duplicate occurrences. -/
theorem extract_literal_bag (rows : List (List α)) :
    extractLiteralRows (rows.map (List.map (PlannedCell.literal (κ := κ)))) = some rows := by
  induction rows with
  | nil => rfl
  | cons row rest ih =>
      unfold extractLiteralRows at ih ⊢
      rw [List.map_cons, List.mapM_cons, extract_literal_row, ih]
      rfl

/-- Encountering an unevaluated cell refuses extraction rather than executing it. -/
theorem deferred_cell_not_extracted (computation : κ) :
    extractLiteral (α := α) (.deferred computation) = none := rfl

end SQLollaM2
