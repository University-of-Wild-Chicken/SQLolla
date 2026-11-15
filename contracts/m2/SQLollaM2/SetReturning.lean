import SQLollaM2.Aggregates

/-! PostgreSQL target-list expansion aligns sibling set-returning calls and pads short sets. -/

namespace SQLollaM2

/-- The longest sibling set determines the number of expanded rows for one source occurrence. -/
def expansionWidth (sets : List (List α)) : Nat :=
  sets.foldl (fun length values => max length values.length) 0

/-- Each ordinal emits one value from each set, with missing values represented explicitly. -/
def alignedSets (sets : List (List α)) : List (List (Option α)) :=
  (List.range (expansionWidth sets)).map fun index => sets.map (·[index]?)

/-- No source set values produce no target-list row. -/
theorem empty_sets_discard_source : alignedSets ([[], []] : List (List Nat)) = [] := rfl

/-- Unequal sibling lengths pad with NULL rather than taking a Cartesian product. -/
theorem unequal_sets_pad :
    alignedSets [[1, 2], [3]] = [[some 1, some 3], [some 2, none]] := rfl

/-- Duplicate array elements remain duplicate row occurrences in their original storage order. -/
theorem duplicate_elements_preserved :
    alignedSets [[1, 1, 2]] = [[some 1], [some 1], [some 2]] := rfl

/-- Array lower bounds do not enter the storage-order expansion contract. -/
def unnestElements (_lowerBound : Int) (elements : List α) : List α := elements

/-- Two arrays with the same stored elements expand identically despite different lower bounds. -/
theorem unnest_ignores_lower_bound (a b : Int) (elements : List α) :
    unnestElements a elements = unnestElements b elements := rfl

/-- Expansion cardinality is the maximum sibling length, including the all-empty case. -/
theorem aligned_cardinality (sets : List (List α)) :
    (alignedSets sets).length = expansionWidth sets := by
  simp [alignedSets]

end SQLollaM2
