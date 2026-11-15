import SQLollaM2.Outcome

/-!
Selected aggregate contracts use mathematical counts/sums, not machine overflow.
ORDER BY requires an ordered result relation; bags alone cannot express LIMIT ties.
Expression error ordering is deliberately supplied by a reference relation rather
than a claim that SQL AND, CASE or projection is always evaluated left to right.
-/

namespace SQLollaM2

/-- COUNT(*) counts occurrences, including rows whose cells are NULL. -/
def countAll (rows : List α) : Nat := rows.length

/-- COUNT(expression) excludes NULL cells but retains duplicate non-NULL occurrences. -/
def countNonNull (cells : List (Option α)) : Nat := (cells.filter Option.isSome).length

/-- Duplicate occurrences contribute separately to COUNT(*). -/
theorem count_duplicates : countAll [7, 7] = 2 := rfl

/-- COUNT(expression) ignores NULL and does not silently apply DISTINCT. -/
theorem count_nullable_duplicates : countNonNull [none, some 7, some 7] = 2 := rfl

/-- Empty input gives zero rather than NULL for COUNT. -/
theorem count_empty : countNonNull (α := Nat) [] = 0 ∧ countAll ([] : List Nat) = 0 :=
  ⟨rfl, rfl⟩

/-- Nullable mathematical SUM remembers whether any non-NULL cell was present. -/
def sumNullable : List (Option Int) → Option Int
  | [] => none
  | none :: rest => sumNullable rest
  | some value :: rest => some (value + (sumNullable rest).getD 0)

/-- SUM differs from COUNT on empty and entirely NULL input. -/
theorem sum_empty_or_null : sumNullable [] = none ∧ sumNullable [none, none] = none :=
  ⟨rfl, rfl⟩

/-- Duplicate non-NULL values are included in a nondistinct sum. -/
theorem sum_duplicates : sumNullable [some 2, none, some 2] = some 4 := by decide

/-- A global aggregate without GROUP BY produces one output row even on empty input. -/
def globalAggregate (aggregate : List α → β) (rows : List α) : List β := [aggregate rows]

/-- Global COUNT over empty input is a one-row result containing zero. -/
theorem global_empty_count : globalAggregate (countAll (α := Nat)) [] = [0] := rfl

/-- SQL ordering permits any permutation satisfying the admitted ordering relation. -/
def AllowedOrder (le : α → α → Prop) (input output : List α) : Prop :=
  input.Perm output ∧ output.Pairwise le

/-- LIMIT may select different tied rows unless the ordering keys distinguish them. -/
theorem equal_keys_have_two_pages :
    AllowedOrder (fun _ _ : Nat => True) [1, 2] [1, 2] ∧
    AllowedOrder (fun _ _ : Nat => True) [1, 2] [2, 1] ∧
    ([1, 2] : List Nat).take 1 ≠ [2, 1].take 1 := by
  constructor
  · exact ⟨List.Perm.refl _, by simp⟩
  · constructor
    · exact ⟨List.Perm.swap 2 1 [], by simp⟩
    · decide

/-- Error-producing expressions require an outcome relation, including planning failures. -/
structure ExpressionContract (Input Output : Type) where
  evaluates : Input → Evaluation Output → Prop

/-- The draft records a domain requirement without inventing deterministic evaluation order. -/
def ExpressionTotal (contract : ExpressionContract Input Output) (within : Input → Prop) : Prop :=
  ∀ input, within input → ∃ output, contract.evaluates input output

end SQLollaM2
