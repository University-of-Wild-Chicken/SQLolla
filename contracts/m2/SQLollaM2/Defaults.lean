import SQLollaM2.Outcome

/-! Per-position INSERT defaults distinguish explicit NULL from omitted/defaulted input.
These contracts do not claim implementation refinement or sequence semantics. -/
namespace SQLollaM2

/-- A supplied value bypasses its default; absence requests exactly that position's default. -/
def resolveDefault {Value Failure : Type} (supplied : Option Value)
    (default : Unit → Except Failure Value) : Except Failure Value :=
  match supplied with
  | some value => .ok value
  | none => default ()

/-- Explicit input is retained even if evaluating its unused default would fail. -/
theorem explicit_value_bypasses_default {Value Failure : Type} (value : Value)
    (default : Unit → Except Failure Value) :
    resolveDefault (some value) default = .ok value := rfl

/-- An explicit DEFAULT request retains default evaluation failures. -/
theorem requested_default_preserves_failure {Value Failure : Type} (failure : Failure) :
    resolveDefault (none : Option Value) (fun _ => .error failure) = .error failure := rfl

/-- A supplied SQL NULL is still supplied input, not a request for the default. -/
theorem explicit_null_is_not_default {Value Failure : Type}
    (default : Unit → Except Failure (Option Value)) :
    resolveDefault (some none) default = .ok none := rfl

/-- Changing defaults at other physical positions cannot alter this position's result. -/
theorem default_position_frame {Value Failure : Type} (supplied : Option Value)
    (left right : Unit → Except Failure Value) (same : left () = right ()) :
    resolveDefault supplied left = resolveDefault supplied right := by
  cases supplied with
  | none => exact same
  | some value => rfl

end SQLollaM2
