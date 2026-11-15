import SQLollaM2.Conflict

/-! Partial unique indexes apply their predicate before uniqueness and arbiter matching. -/
namespace SQLollaM2

/-- Only TRUE predicate values contribute rows to a partial index. -/
def indexedRows (predicate : Row → SQLolla.Truth) (rows : List Row) : List Row :=
  rows.filter fun row => (predicate row).keepsRow

/-- UNKNOWN excludes a row from the index just like FALSE. -/
theorem unknown_index_excludes (rows : List Row) :
    indexedRows (fun _ => .unknown) rows = [] := by simp [indexedRows, SQLolla.Truth.keepsRow]

/-- A total index retains the entire occurrence list. -/
theorem total_index_keeps_all (rows : List Row) :
    indexedRows (fun _ => .yes) rows = rows := by simp [indexedRows, SQLolla.Truth.keepsRow]

/-- Two rows can conflict only when both are in the index and their key values conflict. -/
def partialConflict (predicate : Row → SQLolla.Truth) (keyConflict : Row → Row → Bool)
    (left right : Row) : Bool :=
  (predicate left).keepsRow && (predicate right).keepsRow && keyConflict left right

/-- A proposal outside the index cannot conflict with a row inside it. -/
theorem excluded_proposal_no_conflict (predicate : Row → SQLolla.Truth)
    (keyConflict : Row → Row → Bool) (left right : Row)
    (excluded : (predicate left).keepsRow = false) :
    partialConflict predicate keyConflict left right = false := by simp [partialConflict, excluded]

/-- Index inference requires implication of the index predicate, not merely a matching key. -/
def IndexPredicateImplied (target index : Row → SQLolla.Truth) : Prop :=
  ∀ row, (target row).keepsRow = true → (index row).keepsRow = true

/-- An identical inference predicate is sufficient without assuming arbitrary predicate implication. -/
theorem identical_index_predicate (predicate : Row → SQLolla.Truth) :
    IndexPredicateImplied predicate predicate := fun _ present => present

/-- Adding another TRUE condition retains implication of an existing index predicate. -/
theorem conjunction_index_predicate (target index extra : Row → SQLolla.Truth)
    (implied : IndexPredicateImplied target index) :
    ∀ row, (target row).keepsRow && (extra row).keepsRow = true → (index row).keepsRow = true := by
  intro row both
  have keeps : (target row).keepsRow = true := by
    cases value : (target row).keepsRow <;> simp_all
  exact implied row keeps

end SQLollaM2
