import Std

/-! SQL truth values and the distinct WHERE and CHECK acceptance rules. -/

namespace SQLolla

/-- SQL predicate results include UNKNOWN, represented independently of FALSE. -/
inductive Truth where
  | yes
  | no
  | unknown
  deriving DecidableEq, Repr

/-- SQL NOT preserves UNKNOWN. This operation has no effects. -/
def Truth.negate : Truth → Truth
  | .yes => .no
  | .no => .yes
  | .unknown => .unknown

/-- SQL AND uses three-valued logic; FALSE dominates UNKNOWN. -/
def Truth.and : Truth → Truth → Truth
  | .no, _ => .no
  | _, .no => .no
  | .yes, .yes => .yes
  | _, _ => .unknown

/-- SQL OR uses three-valued logic; TRUE dominates UNKNOWN. -/
def Truth.or : Truth → Truth → Truth
  | .yes, _ => .yes
  | _, .yes => .yes
  | .no, .no => .no
  | _, _ => .unknown

/-- WHERE, HAVING and ON retain only rows whose predicate is TRUE. -/
def Truth.keepsRow : Truth → Bool
  | .yes => true
  | _ => false

/-- A CHECK rejects FALSE; UNKNOWN alone does not violate a CHECK. -/
def Truth.passesCheck : Truth → Bool
  | .no => false
  | _ => true

/-- Equality on nullable values is UNKNOWN if either operand is NULL. -/
def sqlEq [DecidableEq α] : Option α → Option α → Truth
  | some left, some right => if left = right then .yes else .no
  | _, _ => .unknown

/-- Duplicate equivalence differs from SQL predicate equality at NULL. -/
def notDistinct [DecidableEq α] (left right : Option α) : Bool :=
  decide (left = right)

/-- Negation is involutive for all three SQL truth values. -/
theorem Truth.negate_involutive (value : Truth) :
    value.negate.negate = value := by
  cases value <;> rfl

/-- UNKNOWN is excluded by a filter but accepted by a CHECK. -/
theorem unknown_filter_check_distinction :
    Truth.unknown.keepsRow = false ∧ Truth.unknown.passesCheck = true := by
  exact ⟨rfl, rfl⟩

/-- SQL NULL equality must not be reused as duplicate equivalence. -/
theorem null_equality_distinction :
    sqlEq (none : Option Int) none = .unknown ∧
    notDistinct (none : Option Int) none = true := by
  exact ⟨rfl, rfl⟩

end SQLolla
