import SQLolla.Truth

/-!
# Relational specification

Lists store row occurrences, not a semantic ordering guarantee. `Bag.Equivalent`
compares value multiplicities. Production ORDER BY requires a separate ordered
result contract. This module deliberately uses mathematical integers and opaque
text; it does not claim PostgreSQL integer overflow or collation semantics.
-/

namespace SQLolla

/-- Initial scalar families. SQL range and collation profiles remain separate. -/
inductive SqlType where
  | boolean
  | integer
  | text
  deriving DecidableEq, Repr

/-- Mathematical values for a scalar family; NULL is represented with Option. -/
def Value : SqlType → Type
  | .boolean => Bool
  | .integer => Int
  | .text => String

/-- Equality for each mathematical scalar family is decidable without an axiom. -/
instance (ty : SqlType) : DecidableEq (Value ty) := by
  cases ty <;> unfold Value <;> infer_instance

/-- An ordered output schema records positions independently of display names. -/
abbrev Schema := List SqlType

/-- A row's cells carry types selected by its schema; all cells may hold NULL. -/
inductive Row : Schema → Type where
  | nil : Row []
  | cons (value : Option (Value ty)) (rest : Row schema) : Row (ty :: schema)

/-- A statically valid column position; duplicate names cannot affect selection. -/
inductive Column : Schema → SqlType → Type where
  | first : Column (ty :: rest) ty
  | next (column : Column rest ty) : Column (other :: rest) ty

/-- Read a typed cell without name resolution or external effects. -/
def Column.read : Column schema ty → Row schema → Option (Value ty)
  | .first, .cons value _ => value
  | .next column, .cons _ rest => column.read rest

/-- Projection expressions here are literals or typed column references only. -/
inductive ScalarExpr (schema : Schema) : SqlType → Type where
  | literal (value : Option (Value ty)) : ScalarExpr schema ty
  | column (position : Column schema ty) : ScalarExpr schema ty

/-- Evaluate the total scalar fragment; partial expressions need explicit errors. -/
def ScalarExpr.eval : ScalarExpr schema ty → Row schema → Option (Value ty)
  | .literal value, _ => value
  | .column position, row => position.read row

/-- A row-occurrence container; equal values may appear more than once. -/
abbrev Bag (α : Type) := List α

/-- Count occurrences by lawful value equality, not by occurrence identity. -/
def Bag.multiplicity [DecidableEq α] (value : α) (rows : Bag α) : Nat :=
  rows.countP (fun row => decide (row = value))

/-- SQL bags are equivalent when every value has the same multiplicity. -/
def Bag.Equivalent [DecidableEq α] (left right : Bag α) : Prop :=
  ∀ value, Bag.multiplicity value left = Bag.multiplicity value right

/-- Projection retains occurrences even when their resulting values are equal. -/
def Bag.project (project : α → β) (rows : Bag α) : Bag β := rows.map project

/-- Filtering keeps TRUE and discards both FALSE and UNKNOWN. -/
def Bag.select (predicate : α → Truth) (rows : Bag α) : Bag α :=
  rows.filter (fun row => (predicate row).keepsRow)

/-- UNION ALL concatenates occurrences without deduplication. -/
def Bag.unionAll (left right : Bag α) : Bag α := left ++ right

/-- Cartesian product retains a separate occurrence for every input pair. -/
def Bag.product (left : Bag α) (right : Bag β) : Bag (α × β) :=
  left.flatMap (fun row => right.map (fun other => (row, other)))

/-- Projection preserves cardinality, including duplicate output occurrences. -/
theorem Bag.project_length (project : α → β) (rows : Bag α) :
    (Bag.project project rows).length = rows.length := by
  simp [Bag.project]

/-- A projection collision creates two equal occurrences rather than one. -/
theorem projection_collision :
    Bag.multiplicity 7 (Bag.project (fun _ : Nat => 7) [1, 2]) = 2 := by
  decide

/-- A predicate that is always UNKNOWN selects no rows. -/
theorem Bag.select_unknown (rows : Bag α) :
    Bag.select (fun _ => .unknown) rows = [] := by
  induction rows with
  | nil => rfl
  | cons _ _ ih => simp_all [Bag.select, Truth.keepsRow]

/-- A complete product encoding needs the full product of input cardinalities. -/
theorem Bag.product_length (left : Bag α) (right : Bag β) :
    (Bag.product left right).length = left.length * right.length := by
  induction left with
  | nil => simp [Bag.product]
  | cons _ _ ih =>
    simpa [Bag.product, Nat.succ_mul, Nat.add_comm] using
      congrArg (fun size => right.length + size) ih

/-- Product requires capacity for all four occurrences, even if values repeat. -/
theorem product_capacity_example :
    (Bag.product [1, 1] [2, 2]).length = 4 := by
  decide

end SQLolla
