import SQLollaM2.RelationalEffects
import SQLollaM2.SequenceEffects

/-! Explicit branch prefixes are semantic inputs, not a shared execution log.
These contracts specify composition obligations, not PostgreSQL optimizer scheduling. -/
namespace SQLollaM2.BranchEffects

/-- Each operation returns only newly produced events and its success or failure. -/
structure Observation (Event Failure Value : Type) where
  delta : List Event
  outcome : Except Failure Value

/-- An admitted continuation receives the complete inherited prefix before it executes. -/
def bind (inherited : List Event) (before : Observation Event Failure Value)
    (next : Value → List Event → Observation Event Failure Output) :
    Observation Event Failure Output :=
  match before.outcome with
  | .error failure => ⟨before.delta, .error failure⟩
  | .ok value =>
    let after := next value (inherited ++ before.delta)
    ⟨before.delta ++ after.delta, after.outcome⟩

/-- Successful composition passes inherited state before computing the child's result. -/
theorem continuation_receives_prefix (inherited delta : List Event) (value : Value)
    (next : Value → List Event → Observation Event Failure Output) :
    bind inherited ⟨delta, .ok value⟩ next =
      ⟨delta ++ (next value (inherited ++ delta)).delta,
       (next value (inherited ++ delta)).outcome⟩ := rfl

/-- An inherited prefix is not copied into the operation's delta a second time. -/
theorem delta_excludes_inherited (inherited delta : List Event) (value : Value) :
    (bind inherited ⟨delta, .ok value⟩ (fun result _ =>
      (⟨[], .ok result⟩ : Observation Event Failure Value))).delta = delta := by
  simp [bind]

/-- Failure prevents the continuation from running while preserving prior local events. -/
theorem failure_is_terminal (inherited delta : List Event) (failure : Failure)
    (next : Value → List Event → Observation Event Failure Output) :
    bind inherited ⟨delta, .error failure⟩ next = ⟨delta, .error failure⟩ := rfl

/-- Every alternative receives the same inherited prefix independently. -/
def fork (inherited : List Event)
    (alternatives : List (List Event → Observation Event Failure Value)) :=
  alternatives.map (fun alternative => alternative inherited)

/-- Enumerating a later alternative cannot expose events emitted by an earlier alternative. -/
theorem independent_forks (inherited : List Event)
    (left right : List Event → Observation Event Failure Value) :
    fork inherited [left, right] = [left inherited, right inherited] := rfl

/-- Lazy selection invokes exactly one branch with the caller's inherited prefix. -/
def choose (inherited : List Event) (condition : Bool)
    (yes no : List Event → Observation Event Failure Value) :=
  if condition then yes inherited else no inherited

/-- An unselected branch contributes no allocation, notification or advisory observation. -/
theorem lazy_false (inherited : List Event)
    (yes no : List Event → Observation Event Failure Value) :
    choose inherited false yes no = no inherited := rfl

/-- Notification delivery and locks roll back; retained events are selected by an explicit policy. -/
def rollback (retained : Event → Bool) (delta : List Event) : List Event :=
  delta.filter retained

/-- Retained duplicate events remain two occurrences after rollback filtering. -/
theorem rollback_duplicate (retained : Event → Bool) (event : Event)
    (keep : retained event = true) : rollback retained [event,event] = [event,event] := by
  simp [rollback, keep]

/-- A proposed CTE schedule must place each dependency before the dependent statement. -/
def scheduleValid (dependencies : Nat → List Nat) : List Nat → List Nat → Bool
  | _, [] => true
  | seen, item :: rest =>
    !(seen.contains item) && (dependencies item).all seen.contains &&
      scheduleValid dependencies (item :: seen) rest

/-- Independent bindings permit both orders; lexical order is not silently imposed. -/
theorem independent_orders :
    scheduleValid (fun _ => []) [] [0,1] = true ∧
    scheduleValid (fun _ => []) [] [1,0] = true := by
  decide

/-- A dependent binding cannot be scheduled before the relation that supplies its input. -/
theorem dependency_order :
    scheduleValid (fun id => if id = 1 then [0] else []) [] [1,0] = false := by
  decide

/-- Duplicate CTE execution cannot masquerade as a valid schedule. -/
theorem duplicate_rejected : scheduleValid (fun _ => []) [] [0,0] = false := by
  decide

/-- After child planning succeeds, a known NULL argument eliminates a declared strict invocation. -/
def strictNullPlan (strict knownNull : Bool) (inherited : List Event)
    (invoke : List Event → Observation Event Failure (Option Value)) :
    Observation Event Failure (Option Value) :=
  if strict && knownNull then ⟨[], .ok none⟩ else invoke inherited

/-- A strict NULL fold does not execute a volatile sibling or produce any local event. -/
theorem strict_null_suppresses_invocation (inherited : List Event)
    (invoke : List Event → Observation Event Failure (Option Value)) :
    strictNullPlan true true inherited invoke = ⟨[], .ok none⟩ := rfl

/-- A non-strict function such as CONCAT cannot use a NULL argument to erase sibling effects. -/
theorem nonstrict_null_preserves_invocation (inherited : List Event)
    (invoke : List Event → Observation Event Failure (Option Value)) :
    strictNullPlan false true inherited invoke = invoke inherited := rfl

/-- Strict metadata alone does not prove a dynamic argument is NULL. -/
theorem dynamic_argument_not_folded (inherited : List Event)
    (invoke : List Event → Observation Event Failure (Option Value)) :
    strictNullPlan true false inherited invoke = invoke inherited := rfl

/-- Whole-job orders do not represent tuple interleavings of multirow effectful producers. -/
def atomicOrders (left right : List Nat) : List (List Nat) :=
  [left ++ right, right ++ left]

/-- A concrete independently observed PostgreSQL row schedule is absent from whole-job permutations. -/
theorem interleaving_missing_from_atomic_orders :
    [0,2,3,1] ∉ atomicOrders [0,1] [2,3] := by decide

/-- The current atomic scheduler requires every competing effectful job to have at most one row. -/
def atomicScheduleCovered (bounds : List Nat) : Bool :=
  bounds.length ≤ 1 || bounds.all (· ≤ 1)

/-- Two multirow producers cannot receive a complete verdict from the atomic scheduler. -/
theorem multiple_multirow_jobs_uncovered : atomicScheduleCovered [2,2] = false := by decide

/-- A single producer and independent singleton jobs remain distinct coverage cases. -/
theorem singleton_jobs_covered : atomicScheduleCovered [1,1] = true := by decide

/-- A demanded producer may still receive no request or only a prefix from its consumer. -/
inductive Demand where
  | none | prefix | full | unknown
  deriving DecidableEq

/-- Eager complete materialization requires an explicit exhausting-consumer certificate. -/
def eagerCovered : Demand → Bool
  | .full => true
  | _ => false

/-- An empty outer input never requests the right producer of a nested-loop left join. -/
def leftJoinDemand (outerEmpty : Bool) : Demand :=
  if outerEmpty then .none else .unknown

/-- Empty-consumer evaluation cannot justify eager evaluation of a referenced read CTE. -/
theorem empty_left_join_uncovered : eagerCovered (leftJoinDemand true) = false := rfl

/-- A correlated membership test may stop at its first matching producer occurrence. -/
theorem membership_prefix_uncovered : eagerCovered .prefix = false := rfl

/-- Reference existence alone leaves demand unknown and cannot authorize complete materialization. -/
theorem unknown_demand_uncovered : eagerCovered .unknown = false := rfl

end SQLollaM2.BranchEffects


