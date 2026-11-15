import SQLollaM2.Values

/-!
Finite interval operation contracts. PostgreSQL 16 keeps calendar months, days and
microseconds separate. This module specifies qualifier/rounding and constructor
operations; it does not prove textual parsing or IEEE arithmetic preservation.
-/

namespace SQLollaM2

/-- The least retained field controls truncation and unmarked input units. -/
inductive IntervalUnit where
  | year | month | day | hour | minute | second
  deriving DecidableEq, Repr

/-- Integral storage components are not replaced by an elapsed-seconds estimate. -/
structure IntervalComponents where
  months : Int
  days : Int
  microseconds : Int
  deriving DecidableEq, Repr

/-- C-style signed division truncates toward zero, including negative intervals. -/
def intervalTruncate (value : Int) (granule : Nat) : Int :=
  let magnitude := Int.ofNat (value.natAbs / granule) * Int.ofNat granule
  if value < 0 then -magnitude else magnitude

/-- Qualifiers remove only lower fields; leading calendar components remain intact. -/
def intervalQualify (unit : IntervalUnit) (value : IntervalComponents) : IntervalComponents :=
  match unit with
  | .year => ⟨intervalTruncate value.months 12, 0, 0⟩
  | .month => ⟨value.months, 0, 0⟩
  | .day => ⟨value.months, value.days, 0⟩
  | .hour => ⟨value.months, value.days, intervalTruncate value.microseconds 3600000000⟩
  | .minute => ⟨value.months, value.days, intervalTruncate value.microseconds 60000000⟩
  | .second => value

/-- Integral unmarked quantities use the least declared field, defaulting to seconds. -/
def intervalUnmarked (unit : IntervalUnit) (amount : Int) : IntervalComponents :=
  match unit with
  | .year => ⟨amount * 12, 0, 0⟩
  | .month => ⟨amount, 0, 0⟩
  | .day => ⟨0, amount, 0⟩
  | .hour => ⟨0, 0, amount * 3600000000⟩
  | .minute => ⟨0, 0, amount * 60000000⟩
  | .second => ⟨0, 0, amount * 1000000⟩

/-- Explicit precision rounds stored microseconds half away from zero, never toward even. -/
def intervalPrecision (precision : Nat) (value : IntervalComponents) : Option IntervalComponents :=
  if precision ≤ 6 then
    let granule := 10 ^ (6 - precision)
    some { value with microseconds := roundAway value.microseconds granule * Int.ofNat granule }
  else none

/-- Final constructor fields use wide exact arithmetic before storage admission. -/
structure IntervalArguments where
  years : Int := 0
  months : Int := 0
  weeks : Int := 0
  days : Int := 0
  hours : Int := 0
  minutes : Int := 0
  secondsMicroseconds : Int := 0
  deriving DecidableEq, Repr

/-- Seconds here are already converted to integral microseconds by the finite rounding contract. -/
def makeIntervalExact (args : IntervalArguments) : IntervalComponents :=
  ⟨args.years * 12 + args.months, args.weeks * 7 + args.days,
   args.hours * 3600000000 + args.minutes * 60000000 + args.secondsMicroseconds⟩

/-- Storage admission rejects unchecked integer overflow instead of pretending it is SQL rejection. -/
def intervalFits (value : IntervalComponents) : Prop :=
  -(2 ^ 31 : Int) ≤ value.months ∧ value.months < 2 ^ 31 ∧
  -(2 ^ 31 : Int) ≤ value.days ∧ value.days < 2 ^ 31 ∧
  -(2 ^ 63 : Int) ≤ value.microseconds ∧ value.microseconds < 2 ^ 63

/-- Finite integer storage bounds are decidable without any additional assumptions. -/
instance (value : IntervalComponents) : Decidable (intervalFits value) := by
  unfold intervalFits
  infer_instance

/-- A finite constructor exposes no partial wrapped value outside its declared storage profile. -/
def makeIntervalChecked (args : IntervalArguments) : Option IntervalComponents :=
  let value := makeIntervalExact args
  if intervalFits value then some value else none

/-- Round an exact nonnegative ratio to nearest, choosing the even integer at a tie. -/
def intervalRoundEvenMagnitude (numerator denominator : Nat) : Nat :=
  let quotient := numerator / denominator
  let remainder := numerator % denominator
  quotient + if 2 * remainder > denominator ∨
    (2 * remainder = denominator ∧ quotient % 2 = 1) then 1 else 0

/-- Signed rounding for a finite already computed floating product, represented as a ratio. -/
def intervalRoundEven (numerator : Int) (denominator : Nat) : Int :=
  let magnitude := Int.ofNat (intervalRoundEvenMagnitude numerator.natAbs denominator)
  if numerator < 0 then -magnitude else magnitude

/-- Verbose fractional-unit parsing rounds nearest but keeps exact halfway products toward zero. -/
def intervalRoundLiteral (numerator : Int) (denominator : Nat) : Int :=
  let magnitude := numerator.natAbs / denominator +
    if 2 * (numerator.natAbs % denominator) > denominator then 1 else 0
  if numerator < 0 then -Int.ofNat magnitude else Int.ofNat magnitude

/-- Named arguments must name a supported slot and must not overwrite a previous named slot. -/
def intervalNamesValid (names : List String) : Prop :=
  names.Nodup ∧ ∀ name ∈ names,
    name ∈ ["years", "months", "weeks", "days", "hours", "mins", "secs"]

/-- Explicit SECOND does not discard leading fields or fractional microseconds. -/
theorem interval_second_identity (value : IntervalComponents) :
    intervalQualify .second value = value := rfl

/-- MONTH keeps all years because the stored unit is months, not a modulo remainder. -/
theorem interval_month_keeps_years (value : IntervalComponents) :
    (intervalQualify .month value).months = value.months := rfl

/-- DAY retains months even though months are not named by that qualifier. -/
theorem interval_day_keeps_months (value : IntervalComponents) :
    (intervalQualify .day value).months = value.months := rfl

/-- HOUR and MINUTE retain both larger storage fields. -/
theorem interval_minute_keeps_calendar (value : IntervalComponents) :
    (intervalQualify .minute value).months = value.months ∧
    (intervalQualify .minute value).days = value.days := ⟨rfl, rfl⟩

/-- Negative lower fields are truncated toward zero, rather than rounded down. -/
theorem interval_negative_truncation : intervalTruncate (-61000000) 60000000 = -60000000 := rfl

/-- YEAR drops residual months and all day/time components. -/
theorem interval_year_boundary : intervalQualify .year ⟨-14, 3, 9⟩ = ⟨-12, 0, 0⟩ := rfl

/-- Explicit millisecond ties round away from zero for both signs. -/
theorem interval_precision_ties :
    intervalPrecision 3 ⟨14, 3, 1234500⟩ = some ⟨14, 3, 1235000⟩ ∧
    intervalPrecision 3 ⟨14, 3, -1234500⟩ = some ⟨14, 3, -1235000⟩ := ⟨rfl, rfl⟩

/-- The finite precision profile does not silently clamp unsupported declarations. -/
theorem interval_precision_refuses (precision : Nat) (value : IntervalComponents)
    (outside : ¬ precision ≤ 6) : intervalPrecision precision value = none := by
  simp [intervalPrecision, outside]

/-- Integral YEAR and MONTH literals have observably different component values. -/
theorem interval_unmarked_units : intervalUnmarked .year 3 = ⟨36, 0, 0⟩ ∧
    intervalUnmarked .month 3 = ⟨3, 0, 0⟩ := ⟨rfl, rfl⟩

/-- Defaulted constructor fields create the zero interval exactly. -/
theorem interval_constructor_defaults : makeIntervalExact {} = ⟨0, 0, 0⟩ := rfl

/-- Constructor weeks become days without flattening calendar months into time. -/
theorem interval_constructor_components :
    makeIntervalExact ⟨1, 2, 3, 4, 5, 6, 7890000⟩ =
      ⟨14, 25, 18367890000⟩ := rfl

/-- Accepted constructors expose the complete exact value. -/
theorem interval_checked_success (args : IntervalArguments)
    (fits : intervalFits (makeIntervalExact args)) :
    makeIntervalChecked args = some (makeIntervalExact args) := by
  simp [makeIntervalChecked, fits]

/-- An out-of-profile constructor cannot be labeled a successful wrapped interval. -/
theorem interval_checked_refusal (args : IntervalArguments)
    (outside : ¬ intervalFits (makeIntervalExact args)) :
    makeIntervalChecked args = none := by simp [makeIntervalChecked, outside]

/-- Float-product rounding and declared precision have distinct tie rules. -/
theorem interval_constructor_rounding_ties :
    intervalRoundEven 5 2 = 2 ∧ intervalRoundEven 3 2 = 2 ∧
    intervalRoundEven (-5) 2 = -2 := ⟨rfl, rfl, rfl⟩

/-- Literal fractional units and constructor/clock fractions intentionally differ at odd ties. -/
theorem interval_literal_rounding_ties :
    intervalRoundLiteral 3 2 = 1 ∧ intervalRoundLiteral (-3) 2 = -1 ∧
    intervalRoundEven 3 2 = 2 := ⟨rfl, rfl, rfl⟩

/-- Values strictly beyond halfway still round to the nearest integer with the original sign. -/
theorem interval_literal_rounding_beyond :
    intervalRoundLiteral 16 10 = 2 ∧ intervalRoundLiteral (-16) 10 = -2 := ⟨rfl, rfl⟩

/-- Duplicate named arguments are rejected instead of silently taking the last value. -/
theorem interval_duplicate_names : ¬ intervalNamesValid ["days", "days"] := by
  simp [intervalNamesValid]

/-- Unknown named slots cannot be interpreted as a defaulted argument. -/
theorem interval_unknown_name : ¬ intervalNamesValid ["unknown"] := by
  simp [intervalNamesValid]

end SQLollaM2
