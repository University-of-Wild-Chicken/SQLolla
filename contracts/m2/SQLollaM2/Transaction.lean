import SQLollaM2.Outcome

/-!
Sequential structured transactions have no savepoints or concurrent actors here.
Statement observations already delivered to a client survive a later rollback.
The commit validator is explicit so deferred failures cannot be assumed away.
-/

namespace SQLollaM2

/-- Preserve the first SQL failure while preventing further state transitions. -/
inductive Progress (State : Type) where
  | active (working : State)
  | failed (cause : Failure)

/-- Client-visible outcomes retain successful counts/rows and subsequent SQL errors. -/
inductive StatementObservation (Output : Type) where
  | succeeded (affected : Nat) (returning : List Output)
  | rejected (failure : Failure) (exposed : List Output)

/-- Execute one statement or return 25P02 after an earlier failure. -/
def advance (step : State → StatementResult State Output) :
    Progress State → Progress State × StatementObservation Output
  | .failed cause => (.failed cause, .rejected ⟨.failedTransaction, .statement, none, none⟩ [])
  | .active working => match step working with
    | .rejected failure exposed => (.failed failure, .rejected failure exposed)
    | .applied result => (.active result.after, .succeeded result.affected result.returning)

/-- A failed transaction does not evaluate a subsequent mutation. -/
theorem failed_stays_failed (step : State → StatementResult State Output) (cause : Failure) :
    (advance step (.failed cause)).1 = .failed cause := rfl

/-- The client sees the aborted-transaction error while original cause remains available. -/
theorem later_statement_reports_25P02 (step : State → StatementResult State Output)
    (cause : Failure) :
    (advance step (.failed cause)).2 = .rejected ⟨.failedTransaction, .statement, none, none⟩ [] := rfl

/-- Already exposed rows from the same failing statement are retained as evidence. -/
theorem failing_statement_retains_emission (working : State) (failure : Failure)
    (exposed : List Output) :
    (advance (fun _ => .rejected failure exposed) (.active working)).2 =
      .rejected failure exposed := rfl

/-- Evaluate a declared sequence while retaining observations in statement order. -/
def runStatements (steps : List (State → StatementResult State Output))
    (progress : Progress State) : Progress State × List (StatementObservation Output) :=
  match steps with
  | [] => (progress, [])
  | step :: rest =>
    let current := advance step progress
    let later := runStatements rest current.1
    (later.1, current.2 :: later.2)

/-- Any suffix leaves an already failed transaction failed with its original cause. -/
theorem failed_suffix_sticky (steps : List (State → StatementResult State Output))
    (cause : Failure) : (runStatements steps (.failed cause)).1 = .failed cause := by
  induction steps with
  | nil => rfl
  | cons step rest ih => simpa [runStatements, advance] using ih

/-- Explicit user rollback is distinct from committing or aborting because of an error. -/
inductive EndReason where
  | committed | rolledBack | aborted (cause : Failure)
  deriving DecidableEq, Repr

/-- PostgreSQL's transaction-ending command tags are distinct from the logical outcome. -/
inductive EndTag where
  | commit | rollback
  deriving DecidableEq, Repr

/-- A successful protocol command may still report that an already failed transaction rolled back. -/
inductive EndProtocol where
  | commandComplete (tag : EndTag)
  | sqlError (failure : Failure)
  deriving DecidableEq, Repr

/-- Final evidence distinguishes visible table state from retained client observations. -/
structure Finished (State Output : Type) where
  visible : State
  observations : List (StatementObservation Output)
  reason : EndReason
  protocol : EndProtocol

/-- COMMIT checks remaining constraints and publishes only an accepted working state. -/
def commit (validate : State → Evaluation Unit) (initial : State)
    (progress : Progress State) (events : List (StatementObservation Output)) :
    Finished State Output :=
  match progress with
  | .failed cause => ⟨initial, events, .aborted cause, .commandComplete .rollback⟩
  | .active working => match validate working with
    | .ok () => ⟨working, events, .committed, .commandComplete .commit⟩
    | .error failure => ⟨initial, events, .aborted failure, .sqlError failure⟩

/-- COMMIT after a failed transaction returns a ROLLBACK tag rather than inventing a new SQL error. -/
theorem failed_commit_protocol (validate : State → Evaluation Unit) (initial : State)
    (cause : Failure) (events : List (StatementObservation Output)) :
    (commit validate initial (.failed cause) events).protocol = .commandComplete .rollback := rfl

/-- Failed statements restore the transaction's original table state at termination. -/
theorem abort_restores_initial (validate : State → Evaluation Unit) (initial : State)
    (cause : Failure) (events : List (StatementObservation Output)) :
    (commit validate initial (.failed cause) events).visible = initial := rfl

/-- A deferred commit failure rolls back every prior successful statement. -/
theorem commit_failure_restores_initial (validate : State → Evaluation Unit)
    (initial working : State) (failure : Failure) (events : List (StatementObservation Output))
    (rejected : validate working = .error failure) :
    (commit validate initial (.active working) events).visible = initial := by
  simp [commit, rejected]

/-- Commit failure remains observable, even if the initial and working states coincide. -/
theorem commit_failure_retained (validate : State → Evaluation Unit)
    (initial working : State) (failure : Failure) (events : List (StatementObservation Output))
    (rejected : validate working = .error failure) :
    (commit validate initial (.active working) events).reason = .aborted failure := by
  simp [commit, rejected]

/-- Earlier returned rows are not erased when a later statement fails. -/
theorem abort_retains_events (validate : State → Evaluation Unit) (initial : State)
    (cause : Failure) (events : List (StatementObservation Output)) :
    (commit validate initial (.failed cause) events).observations = events := rfl

/-- Explicit rollback restores tables without invoking the commit validator. -/
def rollback (initial : State) (events : List (StatementObservation Output)) :
    Finished State Output := ⟨initial, events, .rolledBack, .commandComplete .rollback⟩

/-- User rollback and SQL-error abort remain different evidence outcomes. -/
theorem rollback_reason (initial : State) (events : List (StatementObservation Output)) :
    (rollback initial events).reason = .rolledBack := rfl

/-- Forget only M2 error detail when connecting to the inherited M0 boundary. -/
def coarseProgress : Progress State → SQLolla.Progress State
  | .active state => .active state
  | .failed _ => .failed .transactionAborted

/-- Rich statement failures preserve M0's transactional table rollback invariant. -/
theorem preserves_m0_rollback (initial : State) (cause : Failure) :
    SQLolla.publish initial (coarseProgress (.failed cause)) = initial := rfl

end SQLollaM2
