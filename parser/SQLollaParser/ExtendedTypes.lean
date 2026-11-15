import SQLollaParser.Expression

/-! Complete syntax labels for v2; type resolution remains a Rust obligation. -/
open Lean (Json toJson)
open SQLean SQLean.Advanced
namespace SQLollaParser

/-- Name every interval range without collapsing its declared precision boundary. -/
def intervalLabel : IntervalFields → String
  | .year => "year" | .month => "month" | .day => "day" | .hour => "hour"
  | .minute => "minute" | .second => "second" | .yearToMonth => "year_to_month"
  | .dayToHour => "day_to_hour" | .dayToMinute => "day_to_minute"
  | .dayToSecond => "day_to_second" | .hourToMinute => "hour_to_minute"
  | .hourToSecond => "hour_to_second" | .minuteToSecond => "minute_to_second"

/-- Preserve all scalar type constructors; wrappers are handled by the recursive exporter. -/
def scalarTypeLabel : TypeName → String
  | .int => "int" | .bigInt => "bigint" | .text => "text" | .varchar => "varchar"
  | .bool => "bool" | .real => "real" | .doublePrecision => "double_precision"
  | .numeric => "numeric" | .smallInt => "smallint" | .char => "char"
  | .date => "date" | .time => "time" | .timeTZ => "timetz"
  | .timestamp => "timestamp" | .timestampTZ => "timestamptz"
  | .interval => "interval" | .uuid => "uuid" | .json => "json"
  | .jsonb => "jsonb" | .bytea => "bytea" | .name => "name"
  | .array _ => "array" | .intervalRange _ => "interval_range"

/-- Export declared array nesting and interval ranges with a bounded structural traversal. -/
def typeNameJson : Nat → TypeName → Except String Json
  | 0, _ => .error "resource.type_depth"
  | fuel + 1, .array element => do
      pure (node "array_type" [("element", ← typeNameJson fuel element)])
  | _ + 1, .intervalRange fields => pure (node "interval_range_type"
      [("fields", toJson (intervalLabel fields))])
  | _ + 1, typeValue => pure (node "scalar_type" [("name", toJson (scalarTypeLabel typeValue))])

/-- Keep precision and scale modifiers as exact natural numbers. -/
def dataTypeJson (type : DataType) : Except String Json := do
  pure (Json.mkObj [("name", ← typeNameJson 64 type.name), ("modifiers", toJson type.modifiers)])

/-- Preserve every binary constructor, without asserting overflow or NULL semantics. -/
def fullBinaryLabel : BinOp → String
  | .eq => "eq" | .ne => "ne" | .lt => "lt" | .le => "le" | .gt => "gt" | .ge => "ge"
  | .and => "and" | .or => "or" | .add => "add" | .sub => "subtract"
  | .mul => "multiply" | .div => "divide"

/-- Retain the aggregate family separately from DISTINCT and FILTER syntax. -/
def aggregateLabel : AggregateFn → String
  | .count => "count" | .sum => "sum" | .avg => "avg" | .min => "min" | .max => "max"

/-- Preserve bare context spellings rather than treating them as ordinary function calls. -/
def contextLabel : ContextValue → String
  | .currentSchema => "current_schema" | .currentCatalog => "current_catalog"
  | .currentUser => "current_user" | .user => "user" | .currentRole => "current_role"
  | .sessionUser => "session_user" | .systemUser => "system_user"

/-- Retain SOME spelling independently of its eventual SQL interpretation. -/
def quantifierLabel : ArrayQuantifier → String
  | .any => "any" | .some => "some" | .all => "all"

/-- Keep SUBSTRING source argument order and special grammar form. -/
def substringLabel : SubstringForm → String
  | .from => "from" | .fromFor => "from_for" | .forFrom => "for_from"
  | .forOnly => "for_only" | .similar => "similar"

/-- Distinguish JSON-returning extraction from its text-returning counterpart. -/
def jsonAccessLabel : JsonAccessOp → String
  | .field => "field" | .fieldText => "field_text"
  | .path => "path" | .pathText => "path_text"

/-- Export optional syntax as JSON null without inventing an expression. -/
def exportOptional (exporter : α → Except String Json) : Option α → Except String Json
  | none => pure Json.null
  | some value => exporter value

/-- Preserve both numbered and anonymous parameter identities without SQL type admission. -/
def fullParameterJson : Parameter → Json
  | .numbered index => node "numbered_parameter" [("index", toJson index)]
  | .anonymous ordinal => node "anonymous_parameter" [("ordinal", toJson ordinal)]

end SQLollaParser
