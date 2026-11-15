import SQLolla.Transaction
import SQLolla.Relation

/-! Explicit SQL outcomes remain distinct from coverage or engine refusals. -/

namespace SQLollaM2

/-- Initial SQLSTATE families; this inventory is not a claim of full execution coverage. -/
inductive ErrorCode where
  | notNullViolation | uniqueViolation | checkViolation | foreignKeyViolation
  | numericOutOfRange | divisionByZero | invalidText | cardinalityViolation
  | failedTransaction
  | arrayDimensions
  deriving DecidableEq, Repr

/-- Stable PostgreSQL codes support locale-independent oracle comparisons. -/
def ErrorCode.sqlstate : ErrorCode → String
  | .notNullViolation => "23502"
  | .uniqueViolation => "23505"
  | .checkViolation => "23514"
  | .foreignKeyViolation => "23503"
  | .numericOutOfRange => "22003"
  | .divisionByZero => "22012"
  | .invalidText => "22P02"
  | .cardinalityViolation => "21000"
  | .failedTransaction => "25P02"
  | .arrayDimensions => "2202E"

/-- Errors may arise before row execution or at commit; neither phase is discarded. -/
inductive ErrorPhase where
  | planning | statement | commit | unspecified
  deriving DecidableEq, Repr

/-- Preserve distinct PostgreSQL diagnostic fields; NOT NULL names a column, not a constraint. -/
structure Failure where
  code : ErrorCode
  phase : ErrorPhase := .statement
  constraintName : Option String := none
  columnName : Option String := none
  deriving DecidableEq, Repr

/-- A SQL evaluator returns a value or an SQL failure, never a truncated success. -/
abbrev Evaluation (α : Type) := Except Failure α

/-- Coverage and engine failures are separate from the SQL program's own outcomes. -/
inductive Attempt (α : Type) where
  | completed (outcome : Evaluation α)
  | unsupported (capability : String)
  | inconclusive (reason : String)

/-- Statement success retains affected occurrences and returned data separately. -/
structure Applied (State Output : Type) where
  after : State
  affected : Nat
  returning : List Output

/-- A rejected statement exposes no candidate state through this atomic boundary. -/
inductive StatementResult (State Output : Type) where
  | applied (result : Applied State Output)
  | rejected (failure : Failure) (exposed : List Output)

/-- Validate a fully computed candidate before making any table state visible. -/
def enforceCandidate (validate : State → Evaluation Unit)
    (candidate : Applied State Output) : StatementResult State Output :=
  match validate candidate.after with
  | .ok () => .applied candidate
  | .error failure => .rejected failure []

/-- Rejected constraints remain reachable SQL errors instead of unsatisfiable post-facts. -/
theorem constraint_rejection_retained (validate : State → Evaluation Unit)
    (candidate : Applied State Output) (failure : Failure)
    (rejected : validate candidate.after = .error failure) :
    enforceCandidate validate candidate = .rejected failure [] := by
  simp [enforceCandidate, rejected]

/-- Buffered candidate calculation cannot publish rows before its success; streaming needs traces. -/
def atomic (compute : State → Evaluation (Applied State Output))
    (validate : State → Evaluation Unit) (before : State) : StatementResult State Output :=
  match compute before with
  | .error failure => .rejected failure []
  | .ok candidate => enforceCandidate validate candidate

/-- Failed arithmetic or expression evaluation exposes no partially computed candidate. -/
theorem calculation_error_retained (compute : State → Evaluation (Applied State Output))
    (validate : State → Evaluation Unit) (before : State) (failure : Failure)
    (failed : compute before = .error failure) :
    atomic compute validate before = .rejected failure [] := by
  simp [atomic, failed]

/-- A single CHECK accepts UNKNOWN and rejects FALSE, matching the inherited truth contract. -/
def checkRow (name : String) (truth : SQLolla.Truth) : Evaluation Unit :=
  if truth.passesCheck then .ok () else .error ⟨.checkViolation, .statement, some name, none⟩

/-- A nullable CHECK is not a substitute for NOT NULL. -/
theorem check_unknown_accepted (name : String) : checkRow name .unknown = .ok () := rfl

/-- NOT NULL has its own error family and does not reuse CHECK's UNKNOWN rule. -/
def requireNonNull (name : String) : Option α → Evaluation Unit
  | some _ => .ok ()
  | none => .error ⟨.notNullViolation, .statement, none, some name⟩

/-- Missing required values yield their specific constraint error. -/
theorem null_rejected (name : String) :
    requireNonNull (α := α) name none =
      .error ⟨.notNullViolation, .statement, none, some name⟩ := rfl

/-- Single-column unique conflicts distinguish default and NULLS NOT DISTINCT behavior. -/
def uniqueConflict [DecidableEq α] (nullsNotDistinct : Bool)
    (left right : Option α) : Bool :=
  if nullsNotDistinct then decide (left = right) else
    match left, right with
    | some a, some b => decide (a = b)
    | _, _ => false

/-- Default UNIQUE permits repeated NULL; NULLS NOT DISTINCT does not. -/
theorem unique_null_modes :
    uniqueConflict (α := Nat) false none none = false ∧
    uniqueConflict (α := Nat) true none none = true := by
  decide

end SQLollaM2
