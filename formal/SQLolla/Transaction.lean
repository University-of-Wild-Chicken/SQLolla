import SQLolla.Truth

/-!
# Transaction boundary

The specification abstracts the admitted SQL execution relation. It models
transactional table state and already exposed observations separately. Sequences,
external functions and other nontransactional effects are not covered by rollback.
-/

namespace SQLolla

/-- Named errors distinguish modeled SQL rejection from compiler failures. -/
inductive SqlError where
  | constraintViolation
  | arithmeticError
  | scalarCardinality
  | transactionAborted
  deriving DecidableEq, Repr

/-- A statement either yields a new working state or a modeled SQL error. -/
inductive StepResult (State : Type) where
  | applied (state : State)
  | rejected (error : SqlError)
  deriving Repr

/-- Transaction progress keeps the working state distinct from committed state. -/
inductive Progress (State : Type) where
  | active (working : State)
  | failed (error : SqlError)
  deriving Repr

/-- Advance an active transaction; failure prevents subsequent writes. -/
def advance (step : State → StepResult State) : Progress State → Progress State
  | .failed error => .failed error
  | .active working => match step working with
    | .applied next => .active next
    | .rejected error => .failed error

/-- A failed transaction cannot resume applying statements without recovery. -/
theorem failed_stays_failed (step : State → StepResult State) (error : SqlError) :
    advance step (.failed error) = .failed error := rfl

/-- Commit publishes working state; failure leaves transactional tables unchanged. -/
def publish (initial : State) : Progress State → State
  | .active working => working
  | .failed _ => initial

/-- Rejected transactions restore the initial transactional table state. -/
theorem failed_publishes_initial (initial : State) (error : SqlError) :
    publish initial (.failed error) = initial := rfl

/-- Commit and abort remain observable even if both leave identical table values. -/
inductive Termination where
  | committed
  | aborted (error : SqlError)
  deriving DecidableEq, Repr

/-- Derive a termination result without conflating no-op commit and abort. -/
def termination : Progress State → Termination
  | .active _ => .committed
  | .failed error => .aborted error

/-- Observations retain termination and previously exposed events across rollback. -/
structure Observation (State Event : Type) where
  visibleState : State
  events : List Event
  result : Termination

/-- Finish a transaction while retaining any observations already exposed. -/
def finish (initial : State) (progress : Progress State) (events : List Event) :
    Observation State Event := ⟨publish initial progress, events, termination progress⟩

/-- Failure remains distinguishable from a no-op commit in the evidence boundary. -/
theorem abort_result_retained (initial : State) (error : SqlError) (events : List Event) :
    (finish initial (.failed error) events).result = .aborted error := rfl

/-- Rollback is not an erasure guarantee for previously exposed data. -/
theorem abort_retains_observations (initial : State) (error : SqlError)
    (events : List Event) :
    (finish initial (.failed error) events).events = events := rfl

/-- Enforced constraints choose apply versus rejection; they are not post-facts. -/
def enforce (valid : State → Bool) (candidate : State) : StepResult State :=
  if valid candidate then .applied candidate else .rejected .constraintViolation

/-- A violating candidate has an explicit error outcome, not no transition. -/
theorem invalid_candidate_rejected (valid : State → Bool) (candidate : State)
    (invalid : valid candidate = false) :
    enforce valid candidate = .rejected .constraintViolation := by
  simp [enforce, invalid]

/-- Pointwise update defines an exact frame over an abstract finite-key store. -/
def updateStore (selected : Key → Bool) (change : Key → Value → Value)
    (before : Key → Value) (key : Key) : Value :=
  if selected key then change key (before key) else before key

/-- Unselected keys retain their exact old value. -/
theorem update_frame (selected : Key → Bool) (change : Key → Value → Value)
    (before : Key → Value) (key : Key) (outside : selected key = false) :
    updateStore selected change before key = before key := by
  simp [updateStore, outside]

end SQLolla
