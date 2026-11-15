/-! Syntax-derived output names retain original names separately from weak cast names. -/
namespace SQLollaM2.OutputLabels

/-- Original columns/functions carry stronger names than an anonymous explicit cast. -/
inductive Label where
  | anonymous
  | cast (name : String)
  | named (name : String)
  deriving DecidableEq

/-- CASE inherits only an original ELSE name; anonymous and weak cast labels use case. -/
def caseLabel : Label → String
  | .named name => name
  | _ => "case"

/-- An explicit cast preserves an original name and replaces any weaker generated name. -/
def castLabel (typeName : String) : Label → Label
  | .named name => .named name
  | _ => .cast typeName

/-- An anonymous cast in ELSE cannot rename its enclosing CASE expression. -/
theorem case_cast_is_weak (typeName : String) :
    caseLabel (.cast typeName) = "case" := rfl

/-- A named ELSE column or function survives an arbitrary explicit cast. -/
theorem case_named_cast_preserved (typeName name : String) :
    caseLabel (castLabel typeName (.named name)) = name := rfl

/-- Nested casts use the outer type when no original column or function supplied a name. -/
theorem outer_cast_replaces_weak (outer inner : String) :
    castLabel outer (.cast inner) = .cast outer := rfl

end SQLollaM2.OutputLabels
