import SQLollaM2.SessionEffects

/-! A fixed, explicitly authorized backend snapshot models signal requests, not process exit.
Operating-system failure, concurrent exit/PID reuse, self-signaling, positive timeout waiting,
and repeated requests need separate schedules and remain refused. -/
namespace SQLollaM2

/-- The caller's independently supplied permissions are evaluated before a signal is attempted. -/
structure BackendSignalPermissions where
  actorSuperuser : Bool
  actorSignalRole : Bool
  targetSuperuser : Bool
  actorHasTargetRole : Bool
  deriving DecidableEq

/-- Superuser-owned targets require a superuser; ordinary targets also permit role privileges. -/
def backendSignalAllowed (p : BackendSignalPermissions) : Bool :=
  (p.actorSuperuser || !p.targetSuperuser) &&
  (p.actorSuperuser || p.actorSignalRole || p.actorHasTargetRole)

/-- Both observations are emitted externally and cannot be undone by SQL rollback. -/
inductive BackendSignalEvent where
  | request (pid : Int)
  | absentWarning (pid : Int)
  deriving DecidableEq

/-- A successful return does not contain a claim that the target has exited. -/
inductive BackendSignalAnswer where
  | value (result : Option Bool) (events : List BackendSignalEvent)
  | sqlError (state : String)
  | refused
  deriving DecidableEq

/-- The deterministic profile requires a stable target and guaranteed signal capability. -/
def backendSignal (pid timeout : Option Int) (present allowed stableCapability : Bool) : BackendSignalAnswer :=
  match pid, timeout with
  | some pid, some timeout =>
    if timeout < 0 then .sqlError "22003"
    else if !present then .value (some false) [.absentWarning pid]
    else if !allowed then .sqlError "42501"
    else if timeout != 0 || !stableCapability then .refused
    else .value (some true) [.request pid]
  | _, _ => .value none []

/-- Strict NULL avoids signaling, including otherwise invalid timeout arguments. -/
theorem backend_null_pid (timeout : Option Int) (present allowed stable : Bool) :
    backendSignal none timeout present allowed stable = .value none [] := by
  cases timeout <;> rfl

/-- Timeout validation precedes backend lookup and its missing-PID warning. -/
theorem backend_negative_timeout (pid timeout : Int) (negative : timeout < 0)
    (present allowed stable : Bool) :
    backendSignal (some pid) (some timeout) present allowed stable = .sqlError "22003" := by
  simp [backendSignal, negative]

/-- An absent backend returns false and an externally visible warning without an attempted request. -/
theorem backend_absent (pid : Int) (allowed stable : Bool) :
    backendSignal (some pid) (some 0) false allowed stable = .value (some false) [.absentWarning pid] := by
  simp [backendSignal]

/-- Lack of role permission is an SQL failure rather than a false or successful return. -/
theorem backend_denied (pid : Int) (stable : Bool) :
    backendSignal (some pid) (some 0) true false stable = .sqlError "42501" := by
  simp [backendSignal]

/-- The admitted successful outcome records only a signal request. -/
theorem backend_requested (pid : Int) :
    backendSignal (some pid) (some 0) true true true = .value (some true) [.request pid] := by
  simp [backendSignal]

/-- Removing the stable capability premise must not fabricate successful signal delivery. -/
theorem backend_unknown_capability (pid : Int) :
    backendSignal (some pid) (some 0) true true false = .refused := by
  simp [backendSignal]

/-- A signal request or warning already emitted survives transactional rollback. -/
def backendSignalRollback (events : List BackendSignalEvent) : List BackendSignalEvent := events

/-- SQL rollback cannot retract an operating-system signal or an already observed warning. -/
theorem backend_signal_survives (events : List BackendSignalEvent) :
    backendSignalRollback events = events := rfl

end SQLollaM2
