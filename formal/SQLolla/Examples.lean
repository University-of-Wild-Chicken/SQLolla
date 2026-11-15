import SQLolla.Relation
import SQLolla.Transaction
import SQLolla.Analysis
import SQLolla.Encoding

/-! Synthetic witnesses prove small contracts only, not SQL execution behavior. -/

namespace SQLolla.Examples

/-- An illustrative order for the tenant-isolation property. -/
structure Order where
  id : Nat
  tenant : Nat
  paid : Bool
  deriving DecidableEq, Repr

/-- A missing tenant predicate selects an order only by its identifier. -/
def unsafePay (target : Nat) (row : Order) : Order :=
  if row.id = target then { row with paid := true } else row

/-- The repaired predicate additionally requires the actor's tenant identity. -/
def guardedPay (actor target : Nat) (row : Order) : Order :=
  if row.id = target ∧ row.tenant = actor then { row with paid := true } else row

/-- A foreign tenant's row is changed by the unguarded operation. -/
theorem tenant_counterexample :
    let before : Order := ⟨101, 2, false⟩
    before.tenant ≠ 1 ∧ (unsafePay 101 before).paid = true := by
  decide

/-- The guarded operation preserves every row belonging to another tenant. -/
theorem guarded_preserves_foreign (actor target : Nat) (row : Order)
    (foreign : row.tenant ≠ actor) : guardedPay actor target row = row := by
  simp [guardedPay, foreign]

/-- A tenant-safe repair may affect no rows and need a separate success rule. -/
theorem guarded_can_be_noop : guardedPay 1 101 ⟨101, 2, false⟩ = ⟨101, 2, false⟩ := by
  decide

/-- A small two-column row demonstrates simultaneous assignment semantics. -/
structure PairRow where
  first : Int
  second : Int
  deriving DecidableEq, Repr

/-- Both right-hand sides read the old row, rather than a partially updated row. -/
def swap (row : PairRow) : PairRow := ⟨row.second, row.first⟩

/-- Simultaneous swapping preserves the pair after two applications. -/
theorem swap_twice (row : PairRow) : swap (swap row) = row := by
  cases row
  rfl

/-- The first statement changes working state, but a later rejection rolls back. -/
theorem second_failure_rolls_back :
    publish 10 (advance (fun _ : Nat => .rejected .constraintViolation)
      (advance (fun value => .applied (value - 1)) (.active 10))) = 10 := rfl

end SQLolla.Examples
