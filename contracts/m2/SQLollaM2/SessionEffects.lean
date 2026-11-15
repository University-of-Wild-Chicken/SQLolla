import SQLollaM2.Outcome

/-!
Pure transaction notification and advisory ownership contracts. Pending notification
invocations are distinct from commit delivery; rollback emits nothing. Ownership is
supplied as a complete snapshot. Blocking/conflicting schedules require a separate
concurrency model and are not converted into successful acquisition.
-/
namespace SQLollaM2

/-- Database notification identity is its exact channel and UTF-8 payload. -/
structure Notification where
  channel : String
  payload : String
  deriving DecidableEq, Repr

/-- Queueing appends an invocation; it does not prematurely claim delivery. -/
def queueNotification (pending : List Notification) (event : Notification) := pending ++ [event]

/-- Statement evaluation preserves the order and multiplicity of pending invocations. -/
theorem notification_queue_cardinality (pending : List Notification) (event : Notification) :
    (queueNotification pending event).length = pending.length + 1 := by simp [queueNotification]

/-- Notification validation counts UTF-8 bytes; a NULL payload means empty, not SQL NULL output. -/
def notificationInput (channel payload : Option String) : Except String Notification :=
  match channel with
  | none => .error "22023"
  | some channel =>
      let payload := payload.getD ""
      if channel.isEmpty || channel.utf8ByteSize ≥ 64 || payload.utf8ByteSize ≥ 8000 then
        .error "22023"
      else .ok ⟨channel, payload⟩

/-- NULL channel is an SQL error and cannot queue a hidden event. -/
theorem notification_null_channel (payload : Option String) :
    notificationInput none payload = .error "22023" := rfl

/-- A valid channel with NULL payload produces an explicit empty payload. -/
theorem notification_null_payload : notificationInput (some "events") none = .ok ⟨"events", ""⟩ := rfl

/-- A void return is a non-NULL scalar even though its wire text is empty. -/
theorem void_not_sql_null : (some () : Option Unit) ≠ none := by simp

/-- Commit delivers each channel/payload pair at its first occurrence in transaction order. -/
def deliverNotificationsFrom (seen : List Notification) : List Notification → List Notification
  | [] => []
  | head :: tail => if head ∈ seen then deliverNotificationsFrom seen tail
      else head :: deliverNotificationsFrom (head :: seen) tail

/-- Delivery starts with no previously emitted event within this transaction. -/
def deliverNotifications (pending : List Notification) := deliverNotificationsFrom [] pending

/-- Duplicate invocations within one transaction produce one delivered notification. -/
theorem notification_duplicate_delivery (event : Notification) :
    deliverNotifications [event, event] = [event] := by simp [deliverNotifications, deliverNotificationsFrom]

/-- Distinct first occurrences retain their original transaction order. -/
theorem notification_distinct_order (first second : Notification) (different : first ≠ second) :
    deliverNotifications [first, second] = [first, second] := by
  simp [deliverNotifications, deliverNotificationsFrom, Ne.symm different]

/-- Rollback and commit have different observable delivery transitions. -/
def finishNotifications (commit : Bool) (pending : List Notification) :=
  if commit then deliverNotifications pending else []

/-- Rolled-back pending work is never reported as delivered output. -/
theorem notification_rollback_empty (pending : List Notification) :
    finishNotifications false pending = [] := rfl

/-- A pending notification that fails a later statement is removed by atomic statement rollback. -/
def rollbackPending (before : List Notification) (_attempted : List Notification) := before

/-- Existing transaction notifications survive a savepoint rollback; new invocations do not. -/
theorem notification_savepoint_frame (before attempted : List Notification) :
    rollbackPending before attempted = before := rfl

/-- Bigint and two-int advisory namespaces remain distinct even when their bits coincide. -/
inductive AdvisoryKey where
  | int64 (key : Int)
  | int32Pair (first second : Int)
  deriving DecidableEq, Repr

/-- Transaction advisory ownership retains owner and shared/exclusive mode. -/
structure AdvisoryHeld where
  key : AdvisoryKey
  owner : String
  shared : Bool
  deriving DecidableEq, Repr

/-- A transaction may reacquire its own locks; shared locks coexist across owners. -/
def advisoryConflicts (request held : AdvisoryHeld) : Bool :=
  decide (request.key = held.key) && decide (request.owner ≠ held.owner) && !(request.shared && held.shared)

/-- A reentrant acquisition never conflicts with the requesting transaction itself. -/
theorem advisory_reentrant (request : AdvisoryHeld) : advisoryConflicts request request = false := by
  simp [advisoryConflicts]

/-- The two advisory namespaces cannot alias through matching numeric payloads. -/
theorem advisory_namespaces_separate (key first second : Int) (left right : String) :
    advisoryConflicts ⟨.int64 key, left, false⟩ ⟨.int32Pair first second, right, false⟩ = false := rfl

/-- Unknown/incomplete ownership and a known blocking conflict remain distinct coverage failures. -/
def acquireAdvisory (complete : Bool) (request : AdvisoryHeld) (held : List AdvisoryHeld) :
    Except String (List AdvisoryHeld) :=
  if !complete then .error "incomplete_snapshot"
  else if held.any (advisoryConflicts request) then .error "blocking_schedule"
  else .ok (if request ∈ held then held else held ++ [request])

/-- Missing ownership evidence cannot establish absence of conflicting holders. -/
theorem advisory_incomplete_refused (request : AdvisoryHeld) (held : List AdvisoryHeld) :
    acquireAdvisory false request held = .error "incomplete_snapshot" := rfl

/-- Duplicate transaction-level acquisition retains a single ownership fact. -/
theorem advisory_ownership_idempotent (request : AdvisoryHeld) :
    acquireAdvisory true request [request] = .ok [request] := by
  simp [acquireAdvisory, advisory_reentrant]

end SQLollaM2
