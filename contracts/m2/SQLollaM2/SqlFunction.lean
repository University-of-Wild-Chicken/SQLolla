import SQLollaM2.UniqueIndex

/-! Restricted immutable SQL function expansion is contextual, deterministic and effect-free.
These contracts do not establish PostgreSQL's optimizer or function-resolution equivalence. -/
namespace SQLollaM2

/-- A declared scalar body consumes already resolved parameter values, independent of name spelling. -/
def scalarBody (body : List Value → Evaluation Value) (arguments : List Value) : Evaluation Value :=
  body arguments

/-- Applying the same immutable body to the same typed values gives the same observation. -/
theorem scalar_body_deterministic (body : List Value → Evaluation Value) (arguments : List Value) :
    scalarBody body arguments = scalarBody body arguments := rfl

/-- Substitution maps a formal parameter occurrence to the corresponding actual argument. -/
def substituteParameter (arguments : List Value) (position : Nat) : Option Value := arguments[position]?

/-- Repeated occurrences of a parameter use the same bound value rather than fresh external effects. -/
theorem repeated_parameter_value (arguments : List Value) (position : Nat) :
    substituteParameter arguments position = substituteParameter arguments position := rfl

/-- A body error remains an explicit error at the invocation boundary. -/
theorem scalar_body_failure (body : List Value → Evaluation Value) (arguments : List Value)
    (failure : Failure) (failed : body arguments = .error failure) :
    scalarBody body arguments = .error failure := failed

/-- The initial expansion profile accepts only arguments whose evaluation cannot produce effects. -/
def InertArguments (evaluate : Argument → Value) (arguments : List Argument) : List Value :=
  arguments.map evaluate

/-- Changing only an unused formal argument does not invent an evaluation effect. -/
theorem constant_body_ignores_inert (a b : List Value) (value : Value) :
    scalarBody (fun _ => .ok value) a = scalarBody (fun _ => .ok value) b := rfl

/-- A strict cast changes NULL's type without applying the non-NULL conversion operation. -/
def strictValueCast (cast : α → Evaluation β) : Option α → Evaluation (Option β)
  | none => .ok none
  | some value => match cast value with
    | .ok result => .ok (some result)
    | .error failure => .error failure

/-- Typed NULL casts are inert even when the non-NULL conversion could fail. -/
theorem null_cast_is_inert (cast : α → Evaluation β) : strictValueCast cast none = .ok none := rfl

end SQLollaM2
