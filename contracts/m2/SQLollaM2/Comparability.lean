import SQLollaM2.Outcome

/-! Operator availability is a type/planning condition for relational comparison.
Generic array scalar functions remain strict runtime operations: NULL can bypass
their invocation, and aggregate extrema may never invoke a comparator. -/
namespace SQLollaM2.Comparability

/-- The abstraction distinguishes ordinary sortable families from JSON and arrays thereof. -/
inductive Family where
  | orderedScalar
  | json
  | array (element : Family)
  deriving DecidableEq, Repr

/-- Array comparison availability depends recursively on its element family. -/
def available : Family → Bool
  | .orderedScalar => true
  | .json => false
  | .array element => available element

/-- Relational comparison cannot be skipped merely because its input happens to be empty. -/
def admitRelation (family : Family) (_rows : List Row) : Except String Unit :=
  if available family then .ok () else .error "42883"

/-- Empty inputs do not make JSON's missing equality/ordering operator available. -/
theorem empty_json_refused : admitRelation .json ([] : List Unit) = .error "42883" := rfl

/-- Arrays recursively inherit missing element comparison support. -/
theorem json_array_refused (rows : List Row) :
    admitRelation (.array .json) rows = .error "42883" := rfl

/-- Planning admission is independent of the actual current table contents. -/
theorem row_independent_admission (family : Family) (left right : List Row) :
    admitRelation family left = admitRelation family right := rfl

/-- UNION ALL concatenation does not itself need a duplicate/equality operator. -/
def unionAll (left right : List Row) : List Row := left ++ right

/-- An empty UNION ALL is valid without requiring any comparison on its element type. -/
theorem union_all_empty : unionAll ([] : List Family) [] = [] := rfl

/-- Strict scalar comparison invokes the generic operator only for two non-NULL arguments. -/
def strictComparison (compare : Value → Value → Except String Bool)
    (left right : Option Value) : Except String (Option Bool) :=
  match left, right with
  | some a, some b => (compare a b).map some
  | _, _ => .ok none

/-- A NULL array operand bypasses the generic comparator, even for a JSON element family. -/
theorem null_skips_operator (compare : Value → Value → Except String Bool)
    (right : Option Value) : strictComparison compare none right = .ok none := by
  cases right <;> rfl

/-- A strict NULL result is independent of an unavailable comparator's possible failure. -/
theorem unavailable_operator_still_null (left : Option Value) :
    strictComparison (fun _ _ => .error "42883") left none = .ok none := by
  cases left <;> rfl

end SQLollaM2.Comparability
