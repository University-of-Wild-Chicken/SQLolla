import SQLollaM2.Collections

/-! Nullable string splitting and JSON type inspection precede scalar admission.
The splitter uses exact strings; collation and textual JSON parsing remain separate.
-/
namespace SQLollaM2

/-- Delimiter absence splits Unicode characters; an empty delimiter retains one whole field. -/
def splitFields (text : String) (delimiter : Option String) : List String :=
  if text.isEmpty then [] else
    match delimiter with
    | none => text.toList.map String.singleton
    | some "" => [text]
    | some delimiter => text.splitOn delimiter

/-- Optional NULL-marker replacement happens after splitting, preserving each field position. -/
def markStringField (nullMarker : Option String) (field : String) : Option String :=
  if nullMarker = some field then none else some field

/-- SQL NULL input remains distinct from a present array of nullable string fields. -/
def stringToArray (text delimiter nullMarker : Option String) : Option (List (Option String)) :=
  text.map (fun text => (splitFields text delimiter).map (markStringField nullMarker))

/-- NULL delimiter is custom behavior, not generic strict-NULL propagation. -/
theorem string_null_delimiter : stringToArray (some "ab") none none = some [some "a", some "b"] := rfl

/-- SQL NULL input remains SQL NULL array, distinct from the empty array. -/
theorem string_null_input (delimiter marker : Option String) :
    stringToArray none delimiter marker = none := rfl

/-- Empty input produces zero fields even when the delimiter itself is empty. -/
theorem string_empty_input (delimiter marker : Option String) :
    stringToArray (some "") delimiter marker = some [] := by simp [stringToArray, splitFields]

/-- An empty delimiter leaves a nonempty string as one field. -/
theorem string_empty_delimiter : stringToArray (some "a,b") (some "") none = some [some "a,b"] := rfl

/-- A matching NULL marker changes the field value without dropping its occurrence. -/
theorem string_null_marker (field : String) : markStringField (some field) field = none := by
  simp [markStringField]

/-- JSON scalar payloads distinguish Boolean and numeric values without their wire spelling. -/
inductive JsonScalarKind where
  | number
  | boolean

/-- JSONB type inspection concerns the top-level value only. -/
def jsonTypeName : JsonTree JsonScalarKind → String
  | .null => "null"
  | .string _ => "string"
  | .scalar .number => "number"
  | .scalar .boolean => "boolean"
  | .array _ => "array"
  | .object _ => "object"

/-- JSON null has a non-NULL textual type name. -/
theorem json_null_type : jsonTypeName .null = "null" := rfl

/-- SQL NULL type inspection remains absent rather than becoming the string null. -/
theorem json_sql_null_type : (none : Option (JsonTree JsonScalarKind)).map jsonTypeName = none := rfl

/-- Strict and custom NULL policies are explicit builtin metadata. -/
inductive BuiltinNullPolicy where
  | strict
  | custom
  deriving DecidableEq

/-- The generic fast path is allowed only for an implemented, declared-strict builtin. -/
def genericNullShortcut (implemented : Bool) (policy : BuiltinNullPolicy) (anyNull : Bool) :=
  implemented && decide (policy = .strict) && anyNull

/-- A custom NULL policy can never silently use the generic strict shortcut. -/
theorem custom_null_no_shortcut (implemented anyNull : Bool) :
    genericNullShortcut implemented .custom anyNull = false := by simp [genericNullShortcut]

end SQLollaM2
