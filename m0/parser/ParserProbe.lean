import SQLean.AdvancedParser
import SQLean.AdvancedPretty
import Lean.Data.Json

/-! M0 feasibility specimen, not a production parser adapter or semantic proof.
Only an explicitly small AST fragment is exported. Other nodes fail closed.
No SQL is executed, bound against a catalog, or translated to relational algebra.
-/

open Lean (Json toJson)
open SQLean
open SQLean.Advanced

namespace SQLolla.M0.ParserProbe

/-- Assemble a tagged specimen node without converting arbitrary Lean Repr text. -/
def node (tag : String) (fields : List (String × Json) := []) : Json :=
  Json.mkObj (("tag", toJson tag) :: fields)

/-- Encode unsigned literal magnitudes as decimal strings to avoid JSON precision loss. -/
def valueJson : Value → Json
  | .int value => node "integer" [("magnitude", toJson (toString value))]
  | .text value => node "text" [("value", toJson value)]
  | .bool value => node "boolean" [("value", toJson value)]

/-- Retain the distinction between anonymous ordinals and numbered parameter indexes. -/
def parameterJson : Parameter → Json
  | .numbered index => node "numbered_parameter" [("index", toJson index)]
  | .anonymous ordinal => node "anonymous_parameter" [("ordinal", toJson ordinal)]

/-- Assign explicit stable specimen labels to the finite binary operator inventory. -/
def binaryLabel : BinOp → String
  | .eq => "eq" | .ne => "ne" | .lt => "lt" | .le => "le" | .gt => "gt" | .ge => "ge"
  | .and => "and" | .or => "or"
  | .add => "add" | .sub => "sub" | .mul => "mul" | .div => "div"

/-- Assign explicit stable specimen labels to unary operators. -/
def unaryLabel : UnOp → String
  | .pos => "positive" | .neg => "negative" | .not => "not"

/-- Keep set-operation kind separate from its duplicate-preservation flag. -/
def setLabel : SetOp → String
  | .union => "union" | .intersect => "intersect" | .except => "except"

/-- Export only the expression forms used by this feasibility matrix, with bounded depth. -/
def exprJson : Nat → SQLean.Advanced.Expr → Except String Json
  | 0, _ => .error "probe.expression_depth"
  | fuel + 1, expr => do
    match expr with
    | .literal value => pure (valueJson value)
    | .null => pure (node "sql_null")
    | .decimal lexeme => pure (node "decimal" [("lexeme", toJson lexeme)])
    | .parameter value => pure (parameterJson value)
    | .column qualifier name schema => pure (node "column"
        [("name", toJson name), ("qualifier", toJson qualifier), ("schema", toJson schema)])
    | .binary op left right => pure (node "binary"
        [("op", toJson (binaryLabel op)), ("left", ← exprJson fuel left),
         ("right", ← exprJson fuel right)])
    | .unary op arg => pure (node "unary"
        [("op", toJson (unaryLabel op)), ("arg", ← exprJson fuel arg)])
    | .isNull arg negated => pure (node "is_null"
        [("arg", ← exprJson fuel arg), ("negated", toJson negated)])
    | _ => .error "probe.unsupported_expression"

/-- Encode an optional expression while distinguishing absent syntax from SQL NULL. -/
def optionalExpr (expr : Option SQLean.Advanced.Expr) : Except String Json :=
  match expr with
  | none => pure Json.null
  | some value => exprJson 64 value

/-- Preserve projection order, aliases and wildcard qualification. -/
def itemJson : SQLean.Advanced.SelectItem → Except String Json
  | .all qualifier schema => pure (node "wildcard"
      [("qualifier", toJson qualifier), ("schema", toJson schema)])
  | .expression expr alias => do
      pure (node "expression" [("value", ← exprJson 64 expr), ("alias", toJson alias)])

/-- Export only base-table FROM sources; complex source trees remain unsupported here. -/
def sourceJson : Option From → Except String Json
  | none => pure Json.null
  | some (.table name alias columns schema) => pure (node "table"
      [("name", toJson name), ("alias", toJson alias),
       ("columns", toJson columns), ("schema", toJson schema)])
  | _ => .error "probe.unsupported_source"

/-- Reject every unexported SELECT modifier before returning a specimen AST. -/
def selectJson (select : Select) : Except String Json := do
  if !select.groupBy.isEmpty || select.having.isSome || !select.distinctOn.isEmpty then
    throw "probe.unsupported_select_modifier"
  pure (node "select" [("items", toJson (← select.items.mapM itemJson)),
    ("source", ← sourceJson select.source), ("where", ← optionalExpr select.whereClause),
    ("distinct", toJson select.distinct)])

/-- Preserve VALUES duplicates and set-operation ALL flags; reject other query modifiers. -/
def queryJson : Nat → SQLean.Advanced.Query → Except String Json
  | 0, _ => .error "probe.query_depth"
  | fuel + 1, query => do
    if !query.ctes.isEmpty || query.recursive || !query.orderBy.isEmpty ||
        query.limit.isSome || query.offset.isSome || !query.locking.isEmpty then
      throw "probe.unsupported_query_modifier"
    match query.body with
    | .select select => selectJson select
    | .values rows => pure (node "values"
        [("rows", toJson (← rows.mapM (fun row => row.mapM (exprJson 64))))])
    | .set op all left right => pure (node "set_operation"
        [("op", toJson (setLabel op)), ("all", toJson all),
         ("left", ← queryJson fuel left), ("right", ← queryJson fuel right)])

/-- Encode an assignment with its original target column and unevaluated expression. -/
def assignmentJson (assignment : SQLean.Advanced.Assignment) : Except String Json := do
  pure (node "assignment"
    [("column", toJson assignment.column), ("value", ← exprJson 64 assignment.value)])

/-- Export queries and UPDATE specimens only; parsing and export coverage stay separate. -/
def statementJson : SQLean.Advanced.Statement → Except String Json
  | .query query => queryJson 64 query
  | .update update => do
      pure (node "update" [("table", toJson update.table), ("alias", toJson update.alias),
        ("assignments", toJson (← update.assignments.mapM assignmentJson)),
        ("where", ← optionalExpr update.whereClause),
        ("returning", toJson (← update.returning.mapM itemJson))])
  | _ => .error "probe.unsupported_statement"

/-- Reparse canonical SQL and compare syntax trees; this is not execution equivalence. -/
def roundTrip (statement : SQLean.Advanced.Statement) (sql : String) : Bool :=
  match SQLean.Advanced.parse sql with
  | .ok reparsed => statement == reparsed
  | .error _ => false

/-- Package parsed syntax with an explicit assurance boundary and fail-closed export status. -/
def parsedReport (statement : SQLean.Advanced.Statement) : Json :=
  let canonical := SQLean.Advanced.render statement
  let fields := [("parse_status", toJson "parsed"), ("canonical_sql", toJson canonical),
    ("canonical_ast_roundtrip", toJson (roundTrip statement canonical))]
  match statementJson statement with
  | .ok ast => Json.mkObj (fields ++ [("export_status", toJson "exported"), ("ast", ast)])
  | .error code => Json.mkObj (fields ++ [("export_status", toJson "unsupported"),
      ("diagnostic", toJson code)])

/-- Return exactly one JSON report for one supplied SQL string; do not execute SQL. -/
def probe (sql : String) : Json :=
  let result := match SQLean.Advanced.parse sql with
    | .ok statement => parsedReport statement
    | .error error => Json.mkObj [("parse_status", toJson "rejected"),
        ("diagnostic", toJson error.message), ("line", toJson error.position.line),
        ("column", toJson error.position.column), ("offset", toJson error.position.offset)]
  Json.mkObj [("protocol", toJson "sqlolla.m0.parser.specimen.v0"),
    ("parser_commit", toJson "ad15c585155ff7fc2c5fc4c9746905e4959ff5eb"),
    ("assurance", toJson "syntax-only"), ("semantic_preservation", toJson false),
    ("result", result)]

end SQLolla.M0.ParserProbe

/-- Native feasibility entry point; command-line argument transport is not a production protocol. -/
def main (args : List String) : IO UInt32 := do
  match args with
  | [sql] => IO.println (SQLolla.M0.ParserProbe.probe sql).compress; pure 0
  | _ => (← IO.getStderr).putStrLn "usage: parser_probe ONE_SQL_ARGUMENT"; pure 2
