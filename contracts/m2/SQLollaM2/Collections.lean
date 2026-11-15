import SQLollaM2.Values

/-!
Top-level JSONB and rectangular array lookup contracts. JSON scalar payloads are
abstract; parsing, numeric encoding, storage ordering and Rust preservation are
separate obligations. SQL NULL is represented outside the JSON null constructor.
-/
namespace SQLollaM2

/-- JSON structure retains nesting without conflating SQL NULL and JSON null. -/
inductive JsonTree (α : Type) where
  | null
  | string (value : String)
  | scalar (value : α)
  | array (values : List (JsonTree α))
  | object (fields : List (String × JsonTree α))

/-- Non-arrays become exactly one element for top-level concatenation. -/
def jsonElements {α : Type} : JsonTree α → List (JsonTree α)
  | .array values => values
  | value => [value]

/-- Right-hand object keys replace whole values; nested objects are not merged. -/
def jsonObjectMerge {α : Type} (left right : List (String × JsonTree α)) :=
  left.filter (fun entry => !(right.any (fun item => item.1 == entry.1))) ++ right

/-- PostgreSQL JSONB concatenation has an object-specific and array-promotion branch. -/
def jsonConcat {α : Type} : JsonTree α → JsonTree α → JsonTree α
  | .object left, .object right => .object (jsonObjectMerge left right)
  | left, right => .array (jsonElements left ++ jsonElements right)

/-- Array concatenation preserves order and repeated occurrences. -/
theorem json_concat_arrays {α : Type} (left right : List (JsonTree α)) :
    jsonConcat (.array left) (.array right) = .array (left ++ right) := rfl

/-- An object collision takes the entire right value, even when both are objects. -/
theorem json_concat_collision {α : Type} (key : String) (left right : JsonTree α) :
    jsonConcat (.object [(key, left)]) (.object [(key, right)]) =
      .object [(key, right)] := by simp [jsonConcat, jsonObjectMerge]

/-- Object/array concatenation preserves the object as one array element. -/
theorem json_concat_object_array {α : Type} (fields : List (String × JsonTree α))
    (right : List (JsonTree α)) :
    jsonConcat (.object fields) (.array right) = .array (.object fields :: right) := rfl

/-- Top-level existence matches object keys, array strings and a scalar string. -/
def jsonExists {α : Type} (key : String) : JsonTree α → Bool
  | .object fields => fields.any (fun entry => entry.1 == key)
  | .array values => values.any (fun value => match value with
      | .string text => text == key
      | _ => false)
  | .string text => text == key
  | _ => false

/-- Nested object keys do not become top-level array string elements. -/
theorem json_exists_not_recursive {α : Type} (key : String) (value : JsonTree α) :
    jsonExists key (.array [.object [(key, value)]]) = false := rfl

/-- Nullable search elements are skipped; they are not SQL UNKNOWN results. -/
def jsonExistsAny {α : Type} (value : JsonTree α) (keys : List (Option String)) :=
  keys.any (fun key => key.any (fun text => jsonExists text value))

/-- Universal existence ignores NULL search elements and is true for the empty set. -/
def jsonExistsAll {α : Type} (value : JsonTree α) (keys : List (Option String)) :=
  keys.all (fun key => key.all (fun text => jsonExists text value))

/-- Empty disjunction is false. -/
theorem json_exists_any_empty {α : Type} (value : JsonTree α) :
    jsonExistsAny value [] = false := rfl

/-- Empty conjunction is true, including a list containing only SQL NULL. -/
theorem json_exists_all_null {α : Type} (value : JsonTree α) :
    jsonExistsAll value [none] = true := rfl

/-- Integral subscripts respect the declared lower bound and dimension width. -/
def arrayCoordinate (lower : Int) (width : Nat) (index : Int) : Option Nat :=
  if lower ≤ index ∧ index < lower + width then some (index - lower).toNat else none

/-- A subscript below the actual lower bound returns no element. -/
theorem array_coordinate_below (lower index : Int) (width : Nat) (below : index < lower) :
    arrayCoordinate lower width index = none := by simp [arrayCoordinate]; omega

/-- The upper boundary is excluded rather than clamped to the last element. -/
theorem array_coordinate_upper (lower : Int) (width : Nat) :
    arrayCoordinate lower width (lower + width) = none := by simp [arrayCoordinate]

/-- Flattening follows row-major dimension order. -/
def arrayOffset (coordinates widths : List Nat) : Nat :=
  (coordinates.zip widths).foldl (fun offset pair => offset * pair.2 + pair.1) 0

/-- Two-dimensional lookup uses the last dimension as its stride. -/
theorem array_offset_two (row column rows columns : Nat) :
    arrayOffset [row, column] [rows, columns] = row * columns + column := by
  simp [arrayOffset]

/-- Plain JSON preserves its validated source spelling instead of JSONB normalization. -/
structure JsonText where
  source : String
  deriving DecidableEq

/-- Transport never silently removes duplicate keys or source whitespace from plain JSON. -/
theorem json_text_identity (source : String) : (JsonText.mk source).source = source := rfl

/-- SQL NULL constructor values become JSON null; they do not null the whole constructor. -/
def jsonConstructorValue {α : Type} : Option (JsonTree α) → JsonTree α
  | none => .null
  | some value => value

/-- Object constructor input retains pair order and repeated key occurrences before JSONB conversion. -/
def jsonBuildObject {α : Type} (fields : List (String × Option (JsonTree α))) :=
  fields.map (fun entry => (entry.1, jsonConstructorValue entry.2))

/-- A NULL value is retained under its key rather than dropping the pair. -/
theorem json_constructor_null {α : Type} (key : String) :
    jsonBuildObject (α := α) [(key, none)] = [(key, .null)] := rfl

/-- JSON array constructors preserve every nullable input position. -/
def jsonBuildArray {α : Type} (values : List (Option (JsonTree α))) :=
  values.map jsonConstructorValue

/-- JSON arrays retain their input arity even when all inputs are SQL NULL. -/
theorem json_constructor_arity {α : Type} (values : List (Option (JsonTree α))) :
    (jsonBuildArray values).length = values.length := by simp [jsonBuildArray]

/-- A bit-string access uses the leftmost bit at zero and refuses an invalid position. -/
def bitAt (values : List Bool) (index : Int) : Option Bool :=
  if index < 0 then none else values[index.toNat]?

/-- Negative bit positions are errors, unlike array element lookups returning SQL NULL. -/
theorem bit_at_negative (values : List Bool) (index : Int) (negative : index < 0) :
    bitAt values index = none := by simp [bitAt, negative]

/-- A slice keeps only existing coordinates inside the explicitly requested closed range. -/
def arraySliceCoordinates (lower : Int) (width : Nat) (start stop : Option Int) : List Nat :=
  (List.range width).filter (fun offset =>
    start.getD lower ≤ lower + offset ∧ lower + offset ≤ stop.getD (lower + width - 1))

/-- Clipping cannot invent positions outside the input dimension. -/
theorem slice_coordinates_valid (lower : Int) (width offset : Nat) (start stop : Option Int)
    (member : offset ∈ arraySliceCoordinates lower width start stop) : offset < width := by
  exact List.mem_range.mp (List.mem_filter.mp member).1

/-- No coordinates exist in a zero-dimensional/empty input. -/
theorem slice_empty_dimension (lower : Int) (start stop : Option Int) :
    arraySliceCoordinates lower 0 start stop = [] := by simp [arraySliceCoordinates]

/-- A successful slice resets each returned dimension's lower bound to one. -/
def arraySliceLowerBounds (widths : List Nat) : List Int := widths.map (fun _ => 1)

/-- Slicing retains the number of result dimensions while normalizing their origins. -/
theorem slice_lower_arity (widths : List Nat) :
    (arraySliceLowerBounds widths).length = widths.length := by simp [arraySliceLowerBounds]

/-- Appending preserves every existing occurrence and retains a NULL element explicitly. -/
def arrayAppend {α : Type} (values : List (Option α)) (value : Option α) := values ++ [value]

/-- Appending increases cardinality by exactly one, including NULL. -/
theorem array_append_cardinality {α : Type} (values : List (Option α)) (value : Option α) :
    (arrayAppend values value).length = values.length + 1 := by simp [arrayAppend]

/-- NULL array input contributes an empty collection, but the appended NULL remains an element. -/
theorem array_append_null_element {α : Type} : arrayAppend (α := α) [] none = [none] := rfl

/-- A missing dimension has SQL NULL length, not a fabricated zero-width dimension. -/
def arrayDimensionLength (widths : List Nat) (dimension : Int) : Option Nat :=
  if dimension ≤ 0 then none else widths[(dimension - 1).toNat]?

/-- An empty array has no first-dimension length. -/
theorem array_length_empty : arrayDimensionLength [] 1 = none := rfl

/-- PostgreSQL reserves representability for the exclusive upper array boundary as well. -/
def arrayBoundsAdmitted (maximum lower : Int) (width : Nat) : Bool :=
  lower + width ≤ maximum

/-- A dimension whose exclusive upper boundary overflows cannot be admitted as stored input. -/
theorem array_bound_overflow (maximum lower : Int) (width : Nat)
    (overflow : maximum < lower + width) : arrayBoundsAdmitted maximum lower width = false := by
  simp [arrayBoundsAdmitted]; omega

/-- Update/insert one object key while retaining every unrelated field. -/
def jsonReplaceKey {α : Type} (fields : List (String × JsonTree α))
    (key : String) (value : JsonTree α) (create : Bool) :=
  if fields.any (fun entry => entry.1 == key) then
    fields.map (fun entry => if entry.1 == key then (key, value) else entry)
  else if create then fields ++ [(key, value)] else fields

/-- Disabling insertion preserves the entire object when its key is absent. -/
theorem json_set_missing_frame {α : Type} (fields : List (String × JsonTree α))
    (key : String) (value : JsonTree α) (absent : fields.any (fun entry => entry.1 == key) = false) :
    jsonReplaceKey fields key value false = fields := by simp [jsonReplaceKey, absent]

/-- A missing final object key can be inserted without creating intermediate objects. -/
theorem json_set_create_empty {α : Type} (key : String) (value : JsonTree α) :
    jsonReplaceKey [] key value true = [(key, value)] := rfl

/-- Resolved JSON path steps distinguish existing slots from the two insertion endpoints. -/
inductive JsonPathStep where
  | field (key : String)
  | index (position : Nat)
  | before
  | after

/-- Nested updates visit existing parents only; insertion is confined to the final path step. -/
def jsonSetPath {α : Type} (value : JsonTree α) (create : Bool) :
    List JsonPathStep → JsonTree α → JsonTree α
  | [], target => target
  | [.field key], .object fields => .object (jsonReplaceKey fields key value create)
  | .field key :: rest, .object fields => .object (fields.map (fun entry =>
      (entry.1, if entry.1 == key then jsonSetPath value create rest entry.2 else entry.2)))
  | [.index index], .array values => .array (values.modify index (fun _ => value))
  | .index index :: rest, .array values =>
      .array (values.modify index (jsonSetPath value create rest))
  | [.before], .array values => if create then .array (value :: values) else .array values
  | [.after], .array values => if create then .array (values ++ [value]) else .array values
  | _, target => target

/-- A missing intermediate object never becomes a fabricated subtree. -/
theorem json_set_no_intermediate {α : Type} (key : String) (step : JsonPathStep)
    (rest : List JsonPathStep) (value : JsonTree α) (create : Bool) :
    jsonSetPath value create (.field key :: step :: rest) (.object []) = .object [] := rfl

/-- An empty path preserves the input instead of replacing its root. -/
theorem json_set_empty_path {α : Type} (target value : JsonTree α) (create : Bool) :
    jsonSetPath value create [] target = target := rfl

/-- Negative JSON array positions count from the end; out-of-range positions are absent. -/
def jsonIndex (length : Nat) (index : Int) : Option Nat :=
  let offset := if index < 0 then (length : Int) + index else index
  if 0 ≤ offset ∧ offset < length then some offset.toNat else none

/-- Field lookup is structural and never treats an object key as an array position. -/
def jsonField {α : Type} (key : String) : JsonTree α → Option (JsonTree α)
  | .object fields => (fields.find? (fun entry => entry.1 == key)).map Prod.snd
  | _ => none

/-- SQL NULL is absence; a selected JSON null is still a present JSON value. -/
def jsonExtract {α : Type} (select : JsonTree α → Option (JsonTree α))
    (source : Option (JsonTree α)) := source.bind select

/-- Text extraction maps JSON null to SQL NULL and unquotes JSON strings. -/
def jsonExtractText {α : Type} (render : JsonTree α → String) : Option (JsonTree α) → Option String
  | none | some .null => none
  | some (.string value) => some value
  | some value => some (render value)

/-- A missing object field differs from a field containing JSON null. -/
theorem json_missing_field {α : Type} (key : String) :
    jsonField (α := α) key (.object []) = none := rfl

/-- A scalar string cannot supply a named object field. -/
theorem json_field_wrong_shape {α : Type} (key value : String) :
    jsonField (α := α) key (.string value) = none := rfl

/-- Extraction from SQL NULL never invokes the structural selector. -/
theorem json_extract_sql_null {α : Type} (select : JsonTree α → Option (JsonTree α)) :
    jsonExtract select none = none := rfl

/-- JSON null becomes SQL NULL only at the text extraction boundary. -/
theorem json_extract_text_null {α : Type} (render : JsonTree α → String) :
    jsonExtractText render (some .null) = none := rfl

/-- Empty JSON strings remain non-NULL empty SQL text. -/
theorem json_extract_text_empty {α : Type} (render : JsonTree α → String) :
    jsonExtractText render (some (.string "")) = some "" := rfl

/-- Minus one names the last occurrence in every nonempty JSON array. -/
theorem json_index_last (length : Nat) : jsonIndex (length + 1) (-1) = some length := by
  simp [jsonIndex]
  omega

/-- An index at the positive length is outside the JSON array. -/
theorem json_index_upper (length : Nat) : jsonIndex length length = none := by
  simp [jsonIndex]
  omega

/-- JSONB integer field extraction exposes its scalar wrapper at zero and minus one. -/
def jsonElement {α : Type} (index : Int) : JsonTree α → Option (JsonTree α)
  | .array values => (jsonIndex values.length index).bind (fun i => values[i]?)
  | .object _ => none
  | scalar => if index = 0 ∨ index = -1 then some scalar else none

/-- JSON null is still a present value when the scalar wrapper is indexed. -/
theorem json_scalar_null_zero {α : Type} : jsonElement (α := α) 0 .null = some .null := by
  simp [jsonElement]

/-- A negative-one field index selects the scalar string without changing its contents. -/
theorem json_scalar_string_last {α : Type} (text : String) :
    jsonElement (α := α) (-1) (.string text) = some (.string text) := by
  simp [jsonElement]

/-- Object fields never participate in scalar-wrapper integer indexing. -/
theorem json_object_integer_absent {α : Type} (fields : List (String × JsonTree α)) (index : Int) :
    jsonElement index (.object fields) = none := rfl

end SQLollaM2
