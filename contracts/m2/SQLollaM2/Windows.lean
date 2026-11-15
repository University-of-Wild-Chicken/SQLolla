import SQLollaM2.Aggregates

/-! Window ranking is per occurrence and per complete legal partition order; ties never choose a witness. -/
namespace SQLollaM2

/-- One sorted partition occurrence retains its original row and three integral ranking results. -/
def rankOccurrence [DecidableEq Key] (key : Row → Key) (rows : List Row)
    (index : Nat) (row : Row) : Row × Nat × Nat × Nat :=
  let prior := (rows.take index).map key
  let earlier := prior.filter (fun value => decide (value ≠ key row))
  (row, index + 1, earlier.length + 1, earlier.eraseDups.length + 1)

/-- The caller supplies a legal sorted partition order and SQL duplicate-equivalence keys. -/
def rankPartition [DecidableEq Key] (key : Row → Key) (rows : List Row) :
    List (Row × Nat × Nat × Nat) := rows.zipIdx.map fun (row, index) => rankOccurrence key rows index row

/-- Equal ordering keys share RANK and DENSE_RANK while ROW_NUMBER still counts occurrences. -/
theorem ranking_ties : rankPartition id ([7, 7, 9] : List Nat) =
    [(7, 1, 1, 1), (7, 2, 1, 1), (9, 3, 3, 2)] := rfl

/-- Empty partitions contain no invented output occurrence. -/
theorem ranking_empty : rankPartition id ([] : List Nat) = [] := rfl

/-- SQL NULL partition/order keys use one caller-established duplicate class. -/
theorem ranking_null_peers : rankPartition id ([none, none, some 1] : List (Option Nat)) =
    [(none, 1, 1, 1), (none, 2, 1, 1), (some 1, 3, 3, 2)] := rfl

/-- Every legal tie ordering remains a separate candidate until complete observations are deduplicated. -/
def rankingAlternatives [DecidableEq Key] (key : Row → Key) (orders : List (List Row)) :=
  orders.map (rankPartition key)

/-- Swapping distinct rows inside a tie changes their row numbers and preserves peer ranks. -/
theorem ranking_tie_choices :
    rankingAlternatives Prod.snd [[("a", 1), ("b", 1)], [("b", 1), ("a", 1)]] =
      [[(("a", 1), 1, 1, 1), (("b", 1), 2, 1, 1)],
       [(("b", 1), 1, 1, 1), (("a", 1), 2, 1, 1)]] := rfl

/-- Each original row occurrence remains present exactly once in a ranked partition. -/
theorem ranking_length [DecidableEq Key] (key : Row → Key) (rows : List Row) :
    (rankPartition key rows).length = rows.length := by simp [rankPartition]

/-- Repeated row_number references read one shared slot rather than independent tie choices. -/
def repeatedRankSlot (rowNumber : Nat) : Nat × Nat := (rowNumber, rowNumber)

/-- Shared ranking slots cannot disagree in one occurrence, even when ties are nondeterministic. -/
theorem repeated_rank_correlated (number : Nat) :
    (repeatedRankSlot number).1 = (repeatedRankSlot number).2 := rfl

/-- Filtering a syntactic singleton changes presence but cannot change any surviving row value. -/
theorem filtered_singleton_identity (keep : Row → Bool) (row candidate : Row)
    (present : candidate ∈ [row].filter keep) : candidate = row := by
  have original : candidate ∈ [row] := (List.mem_filter.mp present).1
  simpa using original

end SQLollaM2
