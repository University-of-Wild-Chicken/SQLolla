import SQLean.AdvancedSyntax
import Lean.Data.Json

/-! Explicit JSON expression encoding. Unsupported operators never lose fields.
The Rust binder owns exact SQL typing, integer ranges and expression semantics.
-/

open Lean (Json toJson)
open SQLean
open SQLean.Advanced

namespace SQLollaParser

/-- Build a tagged AST node with explicit protocol keys. -/
def node (tag : String) (fields : List (String × Json) := []) : Json :=
  Json.mkObj (("tag", toJson tag) :: fields)

/-- Preserve literal integer magnitude without JSON floating-point conversion. -/
def valueJson : Value → Json
  | .int value => node "integer" [("magnitude", toJson (toString value))]
  | .text value => node "text" [("value", toJson value)]
  | .bool value => node "boolean" [("value", toJson value)]

/-- Admit PostgreSQL numbered parameters; anonymous placeholders are outside this profile. -/
def parameterJson : Parameter → Except String Json
  | .numbered index =>
      if index == 0 || index > 65535 then .error "unsupported.parameter_index"
      else pure (node "numbered_parameter" [("index", toJson index)])
  | .anonymous _ => .error "unsupported.anonymous_parameter"

/-- Reject arithmetic instead of passing an unmodeled operation into the common AST. -/
def binaryLabel : BinOp → Except String String
  | .eq => pure "eq" | .ne => pure "ne" | .lt => pure "lt"
  | .le => pure "le" | .gt => pure "gt" | .ge => pure "ge"
  | .and => pure "and" | .or => pure "or"
  | _ => .error "unsupported.arithmetic"

/-- Assign fixed protocol labels to the admitted unary syntax. -/
def unaryLabel : UnOp → String
  | .pos => "positive" | .neg => "negative" | .not => "not"

/-- Restrict numeric signs to integer literals while retaining general logical negation. -/
def checkUnary (op : UnOp) (arg : SQLean.Advanced.Expr) : Except String Unit :=
  match op, arg with
  | .not, _ | .pos, .literal (.int _) | .neg, .literal (.int _) => pure ()
  | _, _ => .error "unsupported.numeric_unary_expression"

/-- Export the admitted expression fragment with a strict recursive depth budget. -/
def expressionJson : Nat → SQLean.Advanced.Expr → Except String Json
  | 0, _ => .error "resource.expression_depth"
  | fuel + 1, expr => do
    match expr with
    | .literal value => pure (valueJson value)
    | .null => pure (node "sql_null")
    | .parameter value => parameterJson value
    | .column qualifier name schema => pure (node "column"
        [("name", toJson name), ("qualifier", toJson qualifier), ("schema", toJson schema)])
    | .binary op left right => pure (node "binary"
        [("op", toJson (← binaryLabel op)), ("left", ← expressionJson fuel left),
         ("right", ← expressionJson fuel right)])
    | .unary op arg => do
        checkUnary op arg
        pure (node "unary" [("op", toJson (unaryLabel op)),
          ("arg", ← expressionJson fuel arg)])
    | .isNull arg negated => pure (node "is_null"
        [("arg", ← expressionJson fuel arg), ("negated", toJson negated)])
    | _ => .error "unsupported.expression"

/-- Distinguish absent optional syntax from an explicit SQL NULL expression. -/
def optionalExpression (expr : Option SQLean.Advanced.Expr) : Except String Json :=
  match expr with
  | none => pure Json.null
  | some value => expressionJson 64 value

/-- Preserve projection order, aliases and wildcard qualification. -/
def itemJson : SQLean.Advanced.SelectItem → Except String Json
  | .all qualifier schema => pure (node "wildcard"
      [("qualifier", toJson qualifier), ("schema", toJson schema)])
  | .expression expr alias => do
      pure (node "expression" [("value", ← expressionJson 64 expr), ("alias", toJson alias)])

end SQLollaParser
