import SQLollaM2.Scalar

/-! Sequence allocation is a separate nontransactional state transition.
These contracts assume an admitted fixed sequence snapshot and explicit evaluation
order. They do not prove that arbitrary SQL plans have that order or exclude callers
in other sessions. Cache ownership and privileges are runtime context obligations. -/
namespace SQLollaM2.SequenceEffects

/-- Sequence bounds and direction are immutable within one admitted statement. -/
structure Specification where
  minimum : Int
  maximum : Int
  increment : Int
  cycle : Bool
  deriving DecidableEq, Repr

/-- The first call may return the existing value without incrementing it. -/
structure State where
  last : Int
  called : Bool
  deriving DecidableEq, Repr

/-- Invalid metadata is an admission failure, not a modeled PostgreSQL sequence. -/
def Valid (spec : Specification) (state : State) : Prop :=
  spec.minimum ≤ state.last ∧ state.last ≤ spec.maximum ∧
  inInt64 spec.minimum ∧ inInt64 spec.maximum ∧
  inInt64 spec.increment ∧ spec.increment ≠ 0

/-- Sequence-specific SQL errors remain explicit and do not advance the state. -/
inductive Failure where
  | exhausted | permissionDenied
  deriving DecidableEq, Repr

/-- Stable codes distinguish sequence exhaustion from numeric expression overflow. -/
def Failure.sqlstate : Failure → String
  | .exhausted => "2200H"
  | .permissionDenied => "42501"

/-- Compute in mathematical integers before checking finite declared sequence bounds. -/
def candidate (spec : Specification) (state : State) : Int :=
  if state.called then state.last + spec.increment else state.last

/-- Noncycling exhaustion returns no value; cycling wraps to the opposite declared endpoint. -/
def selectValue (spec : Specification) (value : Int) : Option Int :=
  if value < spec.minimum then if spec.cycle then some spec.maximum else none
  else if spec.maximum < value then if spec.cycle then some spec.minimum else none
  else some value

/-- A successful selection remains in the declared range, including either cycling endpoint. -/
theorem selected_value_in_bounds (spec : Specification) (value result : Int)
    (bounds : spec.minimum ≤ spec.maximum)
    (success : selectValue spec value = some result) :
    spec.minimum ≤ result ∧ result ≤ spec.maximum := by
  unfold selectValue at success
  split at success
  · split at success
    · cases success
      exact ⟨bounds, Int.le_refl _⟩
    · cases success
  · split at success
    · split at success
      · cases success
        exact ⟨Int.le_refl _, bounds⟩
      · cases success
    · cases success
      omega

/-- Every successful allocation sets is_called and returns exactly its new last value. -/
def allocate (spec : Specification) (state : State) (permitted : Bool) :
    Except Failure (State × Int) :=
  if !permitted then .error .permissionDenied
  else match selectValue spec (candidate spec state) with
    | none => .error .exhausted
    | some value => .ok (⟨value, true⟩, value)

/-- Permission denial cannot publish a sequence value or poststate. -/
theorem denied_allocation (spec : Specification) (state : State) :
    allocate spec state false = .error .permissionDenied := rfl

/-- Returned values and persisted sequence state cannot diverge on success. -/
theorem allocated_value_matches_state (spec : Specification) (before after : State)
    (permitted : Bool) (value : Int)
    (success : allocate spec before permitted = .ok (after, value)) :
    after.last = value ∧ after.called = true := by
  unfold allocate at success
  split at success
  · cases success
  · split at success
    · cases success
    · cases success
      exact ⟨rfl, rfl⟩

/-- An uncalled sequence returns its initialized value instead of incrementing prematurely. -/
theorem first_call :
    allocate ⟨1, 100, 3, false⟩ ⟨41, false⟩ true = .ok (⟨41, true⟩, 41) := rfl

/-- A later ascending call uses the declared increment. -/
theorem later_call :
    allocate ⟨1, 100, 3, false⟩ ⟨41, true⟩ true = .ok (⟨44, true⟩, 44) := rfl

/-- Ascending exhaustion is an observable SQL failure rather than wrapping. -/
theorem ascending_exhausted :
    allocate ⟨1, 3, 1, false⟩ ⟨3, true⟩ true = .error .exhausted := rfl

/-- Explicit ascending cycling wraps to the minimum. -/
theorem ascending_cycle :
    allocate ⟨1, 3, 1, true⟩ ⟨3, true⟩ true = .ok (⟨1, true⟩, 1) := rfl

/-- Descending cycling wraps to the maximum instead of treating negative increments as overflow. -/
theorem descending_cycle :
    allocate ⟨-3, -1, -1, true⟩ ⟨-3, true⟩ true = .ok (⟨-1, true⟩, -1) := rfl

/-- A branch carries its own sequence state and ordered allocation events across SQL failure. -/
structure Branch (α : Type) where
  state : State
  allocations : List Int
  outcome : Except Failure α

/-- Continue one branch using only its own state and append only its own new observations. -/
def continueBranch (before : Branch α) (next : α → State → Branch β) : Branch β :=
  match before.outcome with
  | .error failure => ⟨before.state, before.allocations, .error failure⟩
  | .ok value =>
    let after := next value before.state
    ⟨after.state, before.allocations ++ after.allocations, after.outcome⟩

/-- A later failure cannot erase allocations that already occurred in the same branch. -/
theorem prior_allocations_retained (state : State) (events : List Int) (failure : Failure)
    (next : α → State → Branch β) :
    (continueBranch ⟨state, events, .error failure⟩ next).allocations = events := rfl

/-- Every alternative starts from the supplied state, never another alternative's poststate. -/
def forkBranches (state : State) (alternatives : List (State → Branch α)) : List (Branch α) :=
  alternatives.map (fun alternative => alternative state)

/-- Two alternatives receive exactly the same initial snapshot independently. -/
theorem independent_alternatives (state : State) (left right : State → Branch α) :
    forkBranches state [left, right] = [left state, right state] := rfl

/-- Table rollback does not roll back a sequence allocation that preceded the SQL failure. -/
def publishTables (before candidate : Tables) (sequence : State)
    (success : Bool) : Tables × State :=
  (if success then candidate else before, sequence)

/-- Failed table writes restore the table frame while retaining the new sequence state. -/
theorem failed_write_retains_sequence (before candidate : Tables) (sequence : State) :
    publishTables before candidate sequence false = (before, sequence) := rfl

/-- A lazy fallback is not invoked when its first argument already has a value. -/
def lazyFallback (state : State) (value : Option α) (fallback : State → Branch α) : Branch α :=
  match value with
  | some present => ⟨state, [], .ok present⟩
  | none => fallback state

/-- COALESCE-like short circuiting cannot allocate from an unused fallback. -/
theorem unused_fallback_no_allocation (state : State) (value : α)
    (fallback : State → Branch α) :
    lazyFallback state (some value) fallback = ⟨state, [], .ok value⟩ := rfl

end SQLollaM2.SequenceEffects
