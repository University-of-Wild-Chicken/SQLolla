import SQLollaM2.Scalar

/-!+Rich values retain nominal type identity and exact finite decimal coefficients.
These operation contracts are not a parser or a Rust preservation theorem.
-/

namespace SQLollaM2

/-- A finite decimal is an exact coefficient multiplied by ten to a negative scale. -/
structure Decimal where
  coefficient : Int
  scale : Nat
  deriving DecidableEq, Repr

/-- Compare without conversion through a binary floating point approximation. -/
def decimalLe (left right : Decimal) : Prop :=
  left.coefficient * (10 : Int) ^ right.scale ≤
    right.coefficient * (10 : Int) ^ left.scale

/-- Decimal equality ignores scale only when the exact cross products agree. -/
def decimalEq (left right : Decimal) : Prop :=
  left.coefficient * (10 : Int) ^ right.scale =
    right.coefficient * (10 : Int) ^ left.scale

/-- Comparing a decimal to itself is reflexive at every scale. -/
theorem decimal_reflexive (value : Decimal) : decimalLe value value := Int.le_refl _

/-- Trailing decimal zeroes do not change numeric equality. -/
theorem decimal_scale_example : decimalEq ⟨12, 1⟩ ⟨120, 2⟩ := rfl

/-- Exact addition aligns the scales before adding coefficients. -/
def decimalAdd (left right : Decimal) : Decimal :=
  ⟨left.coefficient * (10 : Int) ^ right.scale +
    right.coefficient * (10 : Int) ^ left.scale, left.scale + right.scale⟩

/-- Exact multiplication preserves all coefficient digits before typmod rounding. -/
def decimalMultiply (left right : Decimal) : Decimal :=
  ⟨left.coefficient * right.coefficient, left.scale + right.scale⟩

/-- Round a nonnegative magnitude to nearest, with ties away from zero. -/
def roundMagnitude (magnitude divisor : Nat) : Nat :=
  magnitude / divisor + if 2 * (magnitude % divisor) ≥ divisor then 1 else 0

/-- Preserve sign after magnitude rounding; callers require a positive divisor. -/
def roundAway (coefficient : Int) (divisor : Nat) : Int :=
  let rounded := Int.ofNat (roundMagnitude coefficient.natAbs divisor)
  if coefficient < 0 then -rounded else rounded

/-- Positive and negative decimal ties round away from zero. -/
theorem decimal_round_ties : roundAway 15 10 = 2 ∧ roundAway (-15) 10 = -2 :=
  ⟨rfl, rfl⟩

/-- A fixed precision stores only coefficients strictly smaller than its power of ten. -/
def decimalFits (precision : Nat) (coefficient : Int) : Prop :=
  coefficient.natAbs < 10 ^ precision

/-- Rounding can create overflow and must precede the precision check. -/
theorem rounding_can_overflow : ¬ decimalFits 2 (roundAway 995 10) := by
  unfold decimalFits roundAway roundMagnitude
  decide

/-- SQL floating comparison has a distinguished NaN above all ordinary values. -/
inductive OrderedFloat (α : Type) where
  | ordinary : α → OrderedFloat α
  | nan : OrderedFloat α
  deriving DecidableEq, Repr

/-- SQL NaN is equal to NaN; ordinary values use semantic rather than bit equality. -/
def floatEq {α : Type} (equivalent : α → α → Bool) :
    OrderedFloat α → OrderedFloat α → Bool
  | .nan, .nan => true
  | .ordinary left, .ordinary right => equivalent left right
  | _, _ => false

/-- SQL's float equality is intentionally different from IEEE NaN equality. -/
theorem nan_equal {α : Type} (equivalent : α → α → Bool) :
    floatEq equivalent .nan .nan = true := rfl

/-- Ordinary floating values sort below NaN, using the supplied total finite ordering. -/
def floatLe {α : Type} (ordered : α → α → Bool) :
    OrderedFloat α → OrderedFloat α → Bool
  | _, .nan => true
  | .nan, .ordinary _ => false
  | .ordinary left, .ordinary right => ordered left right

/-- NaN is maximal in PostgreSQL's SQL ordering. -/
theorem nan_greatest {α : Type} (ordered : α → α → Bool) (value : OrderedFloat α) :
    floatLe ordered value .nan = true := by cases value <;> rfl

/-- Enum labels retain their declared nominal type and ordinal. -/
structure EnumValue where
  typeName : String
  ordinal : Nat
  deriving DecidableEq, Repr

/-- Comparing enum ordinals is admitted only within the same declared enum type. -/
def enumLe (left right : EnumValue) : Option Bool :=
  if left.typeName = right.typeName then some (left.ordinal ≤ right.ordinal) else none

/-- Equal ordinal numbers in different enum types do not authorize a comparison. -/
theorem enum_nominal (left right : EnumValue) (different : left.typeName ≠ right.typeName) :
    enumLe left right = none := by simp [enumLe, different]

/-- UTC timestamp values use integral microseconds from a declared epoch. -/
structure TimestampUTC where
  microseconds : Int
  deriving DecidableEq, Repr

/-- Adding a fixed duration is exact before the representability gate. -/
def timestampAdd (value : TimestampUTC) (duration : Int) : TimestampUTC :=
  ⟨value.microseconds + duration⟩

/-- Zero duration preserves an instant exactly. -/
theorem timestamp_zero (value : TimestampUTC) : timestampAdd value 0 = value := by
  cases value
  simp [timestampAdd]

/-- Array equality retains dimensions and lower bounds as well as nullable elements. -/
structure ArrayValue (α : Type) where
  dimensions : List Nat
  lowerBounds : List Int
  elements : List (Option α)
  deriving DecidableEq, Repr

/-- SQL array duplicate equality is reflexive even for NULL elements. -/
theorem array_null_reflexive {α : Type} (value : ArrayValue α) : value = value := rfl

/-- Canonical JSON objects have unique keys; duplicate resolution happens first. -/
structure CanonicalObject (α : Type) where
  fields : List (String × α)
  unique : fields.map Prod.fst |>.Nodup

/-- A canonical object's identity includes all retained keys and values. -/
theorem object_fields_identity {α : Type} (left right : CanonicalObject α)
    (same : left.fields = right.fields) : left = right := by
  cases left
  cases right
  cases same
  rfl

/-- Expression result annotations discard numeric storage modifiers while retaining the value's scale. -/
def signResultTypmod (isNumeric : Bool) (stored : Option (Nat × Int)) : Option (Nat × Int) :=
  if isNumeric then none else stored

/-- Both numeric signs return the unconstrained numeric family, including typed NULL. -/
theorem numeric_sign_drops_typmod (stored : Option (Nat × Int)) :
    signResultTypmod true stored = none := rfl

/-- Resolving a numeric expression twice cannot reintroduce its storage modifier. -/
theorem numeric_sign_typmod_idempotent (stored : Option (Nat × Int)) :
    signResultTypmod true (signResultTypmod true stored) = signResultTypmod true stored := rfl

/-- Other admitted scalar families retain their existing result annotation. -/
theorem other_sign_retains_typmod (stored : Option (Nat × Int)) :
    signResultTypmod false stored = stored := rfl

end SQLollaM2
