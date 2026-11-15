import SQLollaM2.Locks

/-! Held ownership is a set-like state; ordered nontransactional events remain an occurrence trace. -/
namespace SQLollaM2

/-- Ordering the four modes represents strength, not acquisition time. -/
def lockStrength : RowLockMode → Nat
  | .keyShare => 0 | .share => 1 | .noKeyUpdate => 2 | .update => 3

/-- Repeated ownership combines by the strongest observed mode for the same row identity. -/
def mergeLockMode (left right : RowLockMode) : RowLockMode :=
  if lockStrength left ≥ lockStrength right then left else right

/-- Reacquiring an already held mode does not create another ownership occurrence. -/
theorem merge_lock_idempotent (mode : RowLockMode) : mergeLockMode mode mode = mode := by
  cases mode <;> rfl

/-- Held ownership does not depend on the order in which compatible acquisitions were composed. -/
theorem merge_lock_commutative (left right : RowLockMode) :
    mergeLockMode left right = mergeLockMode right left := by
  cases left <;> cases right <;> rfl

/-- Stronger ownership cannot be lost when merging another source observation. -/
theorem merge_lock_update (mode : RowLockMode) : mergeLockMode mode .update = .update := by
  cases mode <;> rfl

/-- Independent observable events preserve their order and multiplicity. -/
def appendEventTrace (left right : List Event) : List Event := left ++ right

/-- Repeated equal sequence/notification events are not deduplicated as held locks are. -/
theorem event_duplicates_retained (event : Event) : appendEventTrace [event] [event] = [event,event] := rfl

/-- A successful relational operator may change rows while preserving its input ownership and trace. -/
structure RelationalObservation (Row Lock Event : Type) where
  rows : List Row
  held : List Lock
  events : List Event

/-- Pure projection/filter output retains observations produced while consuming the input. -/
def replaceObservedRows (value : RelationalObservation Row Lock Event) (rows : List Output) :
    RelationalObservation Output Lock Event := ⟨rows,value.held,value.events⟩

/-- Filtering away an exposed row does not erase an ownership observation already acquired upstream. -/
theorem filtered_observation_retained (value : RelationalObservation Row Lock Event) :
    (replaceObservedRows value ([] : List Row)).held = value.held := rfl

/-- Statement-savepoint rollback releases newly acquired transactional ownership but retains external traces. -/
def failedObservation (events : List Event) : List Lock × List Event := ([],events)

/-- Failed statement publication cannot present newly acquired row locks as still held after rollback. -/
theorem failed_locks_released (events : List Event) :
    (failedObservation (Lock := Lock) events).1 = [] := rfl

/-- Retained nontransactional traces survive failure without reordering or duplicate loss. -/
theorem failed_events_retained (events : List Event) :
    (failedObservation (Lock := Lock) events).2 = events := rfl

end SQLollaM2
