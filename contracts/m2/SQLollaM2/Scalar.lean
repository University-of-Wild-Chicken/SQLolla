import SQLollaM2.Outcome

/-!
Checked Int32 arithmetic uses mathematical integers plus explicit SQL error branches.
Input type admission, constant folding and evaluation order remain separate duties.
These functions describe individual operations after their operands are available.
-/

namespace SQLollaM2

/-- PostgreSQL integer values occupy the signed 32-bit range. -/
def inInt32 (value : Int) : Prop := -2147483648 ≤ value ∧ value ≤ 2147483647

/-- Decide representability before returning a mathematical result to the SQL value domain. -/
def checkedInt32 (value : Int) : Evaluation Int :=
  if -2147483648 ≤ value ∧ value ≤ 2147483647 then .ok value
  else .error ⟨.numericOutOfRange, .statement, none, none⟩

/-- Every successful checked result is representable; wrapping cannot count as success. -/
theorem checked_value_in_range (value result : Int)
    (success : checkedInt32 value = .ok result) : inInt32 result := by
  unfold checkedInt32 at success
  split at success
  · cases success
    assumption
  · cases success

/-- SQL subtraction exposes out-of-range results rather than saturating or wrapping. -/
def subtractInt32 (left right : Int) : Evaluation Int := checkedInt32 (left - right)

/-- SQL addition uses the same checked result boundary. -/
def addInt32 (left right : Int) : Evaluation Int := checkedInt32 (left + right)

/-- Multiplication computes the mathematical product before the SQL range check. -/
def multiplyInt32 (left right : Int) : Evaluation Int := checkedInt32 (left * right)

/-- Multiplication overflow is a SQL error, never a wrapped value. -/
theorem multiplication_overflow :
    multiplyInt32 2147483647 2 = .error ⟨.numericOutOfRange, .statement, none, none⟩ := rfl

/-- PostgreSQL bigint occupies the signed 64-bit range. -/
def inInt64 (value : Int) : Prop :=
  -9223372036854775808 ≤ value ∧ value ≤ 9223372036854775807

/-- Bigint conversion and arithmetic retain an explicit overflow outcome. -/
def checkedInt64 (value : Int) : Evaluation Int :=
  if -9223372036854775808 ≤ value ∧ value ≤ 9223372036854775807 then .ok value
  else .error ⟨.numericOutOfRange, .statement, none, none⟩

/-- Successful bigint results remain representable without host integer wrapping. -/
theorem checked_bigint_in_range (value result : Int)
    (success : checkedInt64 value = .ok result) : inInt64 result := by
  unfold checkedInt64 at success
  split at success
  · cases success
    assumption
  · cases success

/-- The positive bigint overflow boundary remains an observable error. -/
theorem bigint_overflow :
    checkedInt64 9223372036854775808 = .error ⟨.numericOutOfRange, .statement, none, none⟩ := rfl

/-- COUNT's mathematical count needs the bigint conversion gate. -/
def checkedCount (count : Nat) : Evaluation Int := checkedInt64 (Int.ofNat count)

/-- COUNT over empty input returns a representable bigint zero. -/
theorem count_zero_in_range : checkedCount 0 = .ok 0 := rfl

/-- Integer division rejects zero and truncates a successful quotient toward zero. -/
def divideInt32 (left right : Int) : Evaluation Int :=
  if right = 0 then .error ⟨.divisionByZero, .statement, none, none⟩
  else checkedInt32 (left.tdiv right)

/-- The minimum Int32 divided by negative one is an overflow error. -/
theorem division_overflow :
    divideInt32 (-2147483648) (-1) = .error ⟨.numericOutOfRange, .statement, none, none⟩ := rfl

/-- Division by zero remains an explicit SQL outcome. -/
theorem division_zero (left : Int) :
    divideInt32 left 0 = .error ⟨.divisionByZero, .statement, none, none⟩ := by
  simp [divideInt32]

/-- PostgreSQL-style signed integer division differs from mathematical floor division. -/
theorem division_truncates : divideInt32 (-3) 2 = .ok (-1) := rfl

/-- Both signed range boundaries have tested error outcomes. -/
theorem arithmetic_boundaries :
    addInt32 2147483647 1 = .error ⟨.numericOutOfRange, .statement, none, none⟩ ∧
    subtractInt32 (-2147483648) 1 = .error ⟨.numericOutOfRange, .statement, none, none⟩ :=
  ⟨rfl, rfl⟩

end SQLollaM2
