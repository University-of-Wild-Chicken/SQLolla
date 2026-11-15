import SQLollaM2.Subquery

/-! Aggregate filtering, duplicate classes and empty nullable results are separate operations. -/

namespace SQLollaM2

/-- FILTER removes non-TRUE rows before evaluating the aggregate argument. -/
def filteredArgument (predicate : α → SQLolla.Truth) (argument : α → Evaluation β)
    (rows : List α) : Evaluation (List β) :=
  (rows.filter fun row => (predicate row).keepsRow).mapM argument

/-- A filtered-out row cannot raise an error while evaluating the aggregate argument. -/
theorem filtered_error_not_evaluated (failure : Failure) :
    filteredArgument (fun _ : Unit => .no) (fun _ => .error failure : Unit → Evaluation Int)
      [()] = .ok [] := rfl

/-- UNKNOWN FILTER predicates discard their row just like WHERE. -/
theorem unknown_filter_discarded :
    filteredArgument (fun _ : Unit => .unknown) (fun _ => .ok (7 : Int)) [()] = .ok [] := rfl

/-- DISTINCT removes semantic duplicate classes after the caller supplies canonical values. -/
def aggregateDistinct (values : List Int) : List Int := values.eraseDups

/-- Duplicate occurrences contribute once to a DISTINCT aggregate. -/
theorem distinct_aggregate_duplicates : aggregateDistinct [1, 1, 2] = [1, 2] := rfl

/-- MIN ignores NULL and retains NULL only when no non-NULL input exists. -/
def minNullable : List (Option Int) → Option Int
  | [] => none
  | none :: rest => minNullable rest
  | some value :: rest => some (min value ((minNullable rest).getD value))

/-- MAX uses the same presence rule with the opposite ordering. -/
def maxNullable : List (Option Int) → Option Int
  | [] => none
  | none :: rest => maxNullable rest
  | some value :: rest => some (max value ((maxNullable rest).getD value))

/-- Empty MIN and MAX do not invent zero or another identity scalar. -/
theorem extrema_empty : minNullable [] = none ∧ maxNullable [] = none := ⟨rfl, rfl⟩

/-- Entirely NULL groups remain NULL for MIN and MAX. -/
theorem extrema_all_null : minNullable [none, none] = none ∧ maxNullable [none, none] = none := ⟨rfl, rfl⟩

/-- MIN and MAX ignore NULL while retaining the extremal non-NULL input. -/
theorem extrema_mixed :
    minNullable [none, some 3, some 1] = some 1 ∧
    maxNullable [none, some 3, some 1] = some 3 := ⟨rfl, rfl⟩

/-- Exact AVG stores sum and non-NULL count before result-type division/rounding. -/
def averageParts (values : List (Option Int)) : Option (Int × Nat) :=
  (sumNullable values).map fun total => (total, countNonNull values)

/-- AVG ignores NULL and counts duplicate non-NULL occurrences. -/
theorem average_nullable_duplicates : averageParts [some 2, none, some 2] = some (4, 2) := rfl

/-- Empty AVG returns NULL, so it does not attempt to divide by zero. -/
theorem average_empty : averageParts [] = none := rfl

/-- DISTINCT ON retains the earliest representative of each semantic key in an ordered stream. -/
def firstPerKey [DecidableEq Key] (key : Row → Key) (rows : List Row) : List Row :=
  rows.foldl (fun kept row =>
    if kept.any (fun prior => decide (key prior = key row)) then kept else kept ++ [row]) []

/-- Reversing a tied group's input can change its selected non-key value. -/
theorem distinct_on_tied_representatives :
    firstPerKey Prod.fst [(1, 2), (1, 3)] = [(1, 2)] ∧
    firstPerKey Prod.fst [(1, 3), (1, 2)] = [(1, 3)] := ⟨rfl, rfl⟩

/-- SQL NULL grouping keys form one duplicate class rather than UNKNOWN comparisons. -/
theorem distinct_on_null_key :
    firstPerKey Prod.fst [(none, 2), (none, 3), (some 1, 4)] = [(none, 2), (some 1, 4)] := rfl

/-- Unordered DISTINCT ON enumerates every representative product without preferring a row. -/
def representativeProducts (groups : List (List Row)) : List (List Row) :=
  groups.foldr (fun group tails => group.flatMap fun row => tails.map (row :: ·)) [[]]

/-- No groups produce exactly the successful empty output. -/
theorem representatives_empty : representativeProducts ([] : List (List Row)) = [[]] := rfl

/-- Every declared representative choice remains in the complete product. -/
theorem representative_choice (row : Row) (group : List Row) (groups : List (List Row))
    (tail : List Row) (headMem : row ∈ group) (tailMem : tail ∈ representativeProducts groups) :
    row :: tail ∈ representativeProducts (group :: groups) := by
  exact List.mem_flatMap.mpr ⟨row, headMem, List.mem_map.mpr ⟨tail, tailMem, rfl⟩⟩

/-- Pagination consumes the key-ordered representative stream before presentation order is hidden. -/
def representativePage [DecidableEq Key] (key : Row → Key) (ordered : List Row)
    (offset limit : Nat) : List Row :=
  ((firstPerKey key ordered).drop offset).take limit

/-- Paginating raw duplicate input would skip a distinct group that remains in the selected stream. -/
theorem representatives_before_page :
    representativePage Prod.fst [(1, 2), (1, 3), (2, 4)] 1 1 = [(2, 4)] := rfl

/-- A zero-size page exposes no selected row even when representatives exist. -/
theorem representative_page_zero [DecidableEq Key] (key : Row → Key)
    (ordered : List Row) (offset : Nat) : representativePage key ordered offset 0 = [] := by
  simp [representativePage]

end SQLollaM2
