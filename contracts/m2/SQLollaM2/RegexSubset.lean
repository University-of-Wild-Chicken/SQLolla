import SQLollaM2.StringCollections

/-!
Denotational contract for the admitted regular-language subset of PostgreSQL ARE.
Anchors inspect the whole input, alternation/concatenation compose positions, and
repetition preserves zero-width acceptance without requiring empty cycles. Regex
parsing, collation, case folding and Rust matcher preservation remain separate.
-/
namespace SQLollaM2

/-- Regular-language syntax is independent of the corpus query or pattern text. -/
inductive RegexExpr where
  | empty
  | atom (accepts : Char → Bool)
  | start
  | finish
  | sequence (left right : RegexExpr)
  | alternate (left right : RegexExpr)
  | repeat (value : RegexExpr)

/-- A match relates inclusive start to exclusive end positions within the original input. -/
inductive RegexMatches (input : List Char) : RegexExpr → Nat → Nat → Prop where
  | empty (position : Nat) : RegexMatches input .empty position position
  | atom (accepts : Char → Bool) (position : Nat) (value : Char)
      (present : input[position]? = some value) (accepted : accepts value = true) :
      RegexMatches input (.atom accepts) position (position + 1)
  | start : RegexMatches input .start 0 0
  | finish : RegexMatches input .finish input.length input.length
  | sequence (left right : RegexExpr) (first middle last : Nat)
      (a : RegexMatches input left first middle) (b : RegexMatches input right middle last) :
      RegexMatches input (.sequence left right) first last
  | left (left right : RegexExpr) (first last : Nat) (matched : RegexMatches input left first last) :
      RegexMatches input (.alternate left right) first last
  | right (left right : RegexExpr) (first last : Nat) (matched : RegexMatches input right first last) :
      RegexMatches input (.alternate left right) first last
  | repeatZero (value : RegexExpr) (position : Nat) : RegexMatches input (.repeat value) position position
  | repeatMore (value : RegexExpr) (first middle last : Nat) (progress : first < middle)
      (head : RegexMatches input value first middle) (tail : RegexMatches input (.repeat value) middle last) :
      RegexMatches input (.repeat value) first last

/-- PostgreSQL regex predicates may match at any input position unless anchors constrain them. -/
def regexSearch (input : List Char) (pattern : RegexExpr) : Prop :=
  ∃ first last, first ≤ input.length ∧ RegexMatches input pattern first last

/-- Empty repetition can match an empty input without consuming a character. -/
theorem regex_repeat_empty (pattern : RegexExpr) : regexSearch [] (.repeat pattern) := by
  exact ⟨0, 0, Nat.le_refl 0, .repeatZero pattern 0⟩

/-- Concatenation accepts precisely a composable pair of positional witnesses. -/
theorem regex_sequence_witness (input : List Char) (left right : RegexExpr) (first middle last : Nat)
    (a : RegexMatches input left first middle) (b : RegexMatches input right middle last) :
    RegexMatches input (.sequence left right) first last := .sequence left right first middle last a b

/-- An alternation does not require choosing one fixed branch for every input. -/
theorem regex_alternate_right (input : List Char) (left right : RegexExpr) (first last : Nat)
    (matched : RegexMatches input right first last) :
    RegexMatches input (.alternate left right) first last := .right left right first last matched

/-- The whole-input anchors accept the empty input at its unique boundary. -/
theorem regex_anchors_empty : regexSearch [] (.sequence .start .finish) := by
  exact ⟨0, 0, Nat.le_refl 0, .sequence _ _ 0 0 0 .start .finish⟩

/-- Matcher resource exhaustion is a third result, never a fabricated negative match. -/
inductive RegexAnswer where
  | matched (value : Bool)
  | exhausted
  deriving DecidableEq

/-- A bounded checker cannot report an exhausted search as a successful false predicate. -/
theorem regex_exhausted_not_false : RegexAnswer.exhausted ≠ .matched false := by simp

end SQLollaM2
