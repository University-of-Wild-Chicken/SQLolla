import SQLollaM2.Crud
import SQLollaM2.Transaction

/-! Small kernel-checked semantic witnesses are not SAT or PostgreSQL execution evidence. -/

namespace SQLollaM2

/-- Minimal business row for tenant isolation and inventory examples. -/
structure StockRow where
  tenant : Nat
  stock : Int
  deriving DecidableEq, Repr

/-- Two occurrence identities belong to different tenants. -/
def tenantStore : Store Bool StockRow
  | false => some ⟨1, 2⟩
  | true => some ⟨2, 2⟩

/-- This mathematical assignment needs explicit Int32 checking before implementation admission. -/
def decrement (row : StockRow) : StockRow := { row with stock := row.stock - 1 }

/-- A missing tenant predicate changes a different tenant's row. -/
theorem cross_tenant_update_witness :
    updateRows (fun _ => .yes) decrement tenantStore true = some ⟨2, 1⟩ ∧
    tenantStore true = some ⟨2, 2⟩ := by
  decide

/-- Tenant-constrained mutation preserves the other tenant's complete row. -/
theorem tenant_frame_witness :
    updateRows (fun row => if row.tenant = 1 then .yes else .no)
      decrement tenantStore true = tenantStore true := by
  decide

/-- Immediate stock CHECK uses SQL's CHECK truth contract. -/
def stockCheck (row : StockRow) : Evaluation Unit :=
  checkRow "stock_nonnegative" (if 0 ≤ row.stock then .yes else .no)

/-- Decrementing zero creates a bad candidate that must be rejected, not omitted. -/
theorem negative_stock_candidate_rejected :
    enforceCandidate stockCheck (⟨⟨1, -1⟩, 1, []⟩ : Applied StockRow Nat) =
      .rejected ⟨.checkViolation, .statement, some "stock_nonnegative", none⟩ [] := rfl

/-- A rejected stock write leaves the initial stock visible at transaction termination. -/
theorem rejected_stock_rolls_back :
    (commit stockCheck (⟨1, 0⟩ : StockRow)
      (.failed ⟨.checkViolation, .statement, some "stock_nonnegative", none⟩)
      ([] : List (StatementObservation Nat))).visible = ⟨1, 0⟩ := rfl

/-- A prior success and its returned value survive as evidence despite table rollback. -/
theorem earlier_returning_not_erased :
    (commit stockCheck (⟨1, 0⟩ : StockRow) (.failed ⟨.divisionByZero, .statement, none, none⟩)
      [.succeeded 1 [42]]).observations = [.succeeded 1 [42]] := rfl

end SQLollaM2
