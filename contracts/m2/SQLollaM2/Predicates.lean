import SQLollaM2.Subquery

/-! Row comparisons consume already type/arity-checked pairs; LIKE uses explicit pattern tokens. -/

namespace SQLollaM2

/-- Row equality combines every component, so FALSE can dominate an earlier UNKNOWN. -/
def rowEqualPairs : List (Option Int × Option Int) → SQLolla.Truth
  | [] => .yes
  | (left, right) :: rest => (nullableEqual left right).and (rowEqualPairs rest)

/-- A definite later mismatch makes row equality FALSE despite a NULL component. -/
theorem row_mismatch_dominates_null :
    rowEqualPairs [(none, some 1), (some 1, some 2)] = .no := rfl

/-- Without a decisive mismatch, a nullable row comparison remains UNKNOWN. -/
theorem row_null_without_mismatch :
    rowEqualPairs [(none, some 1), (some 2, some 2)] = .unknown := rfl

/-- Lexicographic less-than stops at the first unequal or NULL component. -/
def rowLessPairs : List (Option Int × Option Int) → SQLolla.Truth
  | [] => .no
  | (some left, some right) :: rest =>
      if left < right then .yes else if left > right then .no else rowLessPairs rest
  | _ :: _ => .unknown

/-- Later NULL components cannot erase a comparison decided by an earlier field. -/
theorem row_lexicographic_stops :
    rowLessPairs [(some 1, some 2), (none, none)] = .yes := rfl

/-- A NULL encountered before any unequal field prevents a lexicographic result. -/
theorem row_lexicographic_null :
    rowLessPairs [(none, none), (some 1, some 2)] = .unknown := rfl

/-- LIKE tokenization distinguishes escaped literals from wildcard instructions. -/
inductive LikeToken where
  | literal (value : Char)
  | one
  | many
  deriving DecidableEq, Repr

/-- Whole-string LIKE semantics over Unicode characters, independent of any regex engine. -/
inductive LikeMatches : List LikeToken → List Char → Prop where
  | empty : LikeMatches [] []
  | literal : LikeMatches pattern input →
      LikeMatches (.literal value :: pattern) (value :: input)
  | one : LikeMatches pattern input → LikeMatches (.one :: pattern) (value :: input)
  | manyEmpty : LikeMatches pattern input → LikeMatches (.many :: pattern) input
  | manyMore : LikeMatches (.many :: pattern) input →
      LikeMatches (.many :: pattern) (value :: input)

/-- An ordinary literal pattern matches its exact whole string. -/
theorem like_exact : LikeMatches [.literal 'a'] ['a'] := .literal .empty

/-- The one-character wildcard consumes one Unicode character. -/
theorem like_one_character : LikeMatches [.one] ['é'] := .one .empty

/-- A percent wildcard can consume zero or several characters. -/
theorem like_many_characters :
    LikeMatches [.many] [] ∧ LikeMatches [.many] ['a', 'b'] :=
  ⟨.manyEmpty .empty, .manyMore (.manyMore (.manyEmpty .empty))⟩

/-- An escaped percent is an ordinary literal rather than a wildcard. -/
theorem like_escaped_percent : LikeMatches [.literal '%'] ['%'] := .literal .empty

/-- Literal matching does not permit an arbitrary suffix after the pattern ends. -/
theorem like_full_string : ¬ LikeMatches [.literal 'a'] ['a', 'b'] := by
  intro matching
  cases matching with
  | literal remaining => cases remaining

end SQLollaM2
