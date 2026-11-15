import SQLollaParser.ExtendedTypes

/-! Lossless typed-expression field export; successful syntax is not semantic admission. -/
open Lean (Json toJson)
open SQLean SQLean.Advanced
namespace SQLollaParser

/-- Export two expression operands under their declared field names. -/
def expressionPair (emit : Advanced.Expr → Except String Json) (tag : String)
    (left right : Advanced.Expr) (fields : List (String × Json) := []) : Except String Json := do
  pure (node tag ([("left", ← emit left), ("right", ← emit right)] ++ fields))

/-- Preserve named argument spelling and position without resolving function overloads. -/
def callArgumentJson (emit : Advanced.Expr → Except String Json) : CallArgument → Except String Json
  | .positional value => do pure (node "positional_argument" [("value", ← emit value)])
  | .named name value spelling => do
      pure (node "named_argument" [("name", toJson name), ("value", ← emit value),
        ("spelling", toJson (match spelling with | .arrow => "arrow" | .assignment => "assignment"))])

/-- Retain omitted slice bounds separately from explicit NULL expressions. -/
def subscriptJson (emit : Advanced.Expr → Except String Json) : Subscript → Except String Json
  | .index expr => do pure (node "index" [("expression", ← emit expr)])
  | .slice lower upper => do pure (node "slice"
      [("lower", ← exportOptional emit lower), ("upper", ← exportOptional emit upper)])

/-- Preserve query-bearing expressions using the same bounded query traversal. -/
def queryExpressionJson (emit : Advanced.Expr → Except String Json)
    (query : Advanced.Query → Except String Json) : Advanced.Expr → Except String Json
  | .quantifiedQuery op quantifier arg value => do
      pure (node "quantified_query" [("op", toJson (fullBinaryLabel op.toBinOp)),
        ("quantifier", toJson (quantifierLabel quantifier)), ("arg", ← emit arg),
        ("query", ← query value)])
  | .inQuery arg value => do pure (node "in_query" [("arg", ← emit arg), ("query", ← query value)])
  | .exists value => do pure (node "exists" [("query", ← query value)])
  | .scalarQuery value => do pure (node "scalar_subquery" [("query", ← query value)])
  | _ => .error "invariant.expression_dispatch"

/-- Export conditional, collection and callable expressions with all operand order retained. -/
def collectionExpressionJson (emit : Advanced.Expr → Except String Json)
    (query : Advanced.Query → Except String Json) : Advanced.Expr → Except String Json
  | .array values => do pure (node "array" [("elements", toJson (← values.mapM emit))])
  | .row values => do pure (node "row" [("elements", toJson (← values.mapM emit))])
  | .call name args => do pure (node "call" [("name", toJson name), ("args", toJson (← args.mapM emit))])
  | .namedCall name args => do pure (node "named_call"
      [("name", toJson name), ("args", toJson (← args.mapM (callArgumentJson emit)))])
  | .caseExpr operand branches otherwise => do
      let branches ← branches.mapM fun (whenExpr, thenExpr) => do
        pure (Json.mkObj [("when", ← emit whenExpr), ("then", ← emit thenExpr)])
      pure (node "case" [("operand", ← exportOptional emit operand),
        ("branches", toJson branches), ("else", ← exportOptional emit otherwise)])
  | .coalesce args => do pure (node "coalesce" [("args", toJson (← args.mapM emit))])
  | .greatest args => do pure (node "greatest" [("args", toJson (← args.mapM emit))])
  | .least args => do pure (node "least" [("args", toJson (← args.mapM emit))])
  | .nullIf left right => expressionPair emit "null_if" left right
  | .inList arg values => do pure (node "in_list"
      [("arg", ← emit arg), ("values", toJson (← values.mapM emit))])
  | .between arg lower upper negated => do pure (node "between"
      [("arg", ← emit arg), ("lower", ← emit lower), ("upper", ← emit upper), ("negated", toJson negated)])
  | .filteredAggregate aggregate predicate => do pure (node "filtered_aggregate"
      [("aggregate", ← emit aggregate), ("predicate", ← emit predicate)])
  | value => queryExpressionJson emit query value

/-- Preserve overloaded SQL syntax without selecting an operator or function implementation. -/
def specialExpressionJson (emit : Advanced.Expr → Except String Json)
    (query : Advanced.Query → Except String Json) : Advanced.Expr → Except String Json
  | .jsonAccess op arg key => do pure (node "json_access" [("arg", ← emit arg), ("key", ← emit key),
      ("op", toJson (jsonAccessLabel op))])
  | .jsonExistence op arg keys => do pure (node "json_existence"
      [("arg", ← emit arg), ("keys", ← emit keys),
       ("op", toJson (match op with | .key => "key" | .any => "any" | .all => "all"))])
  | .containment left right containedBy =>
      expressionPair emit "containment" left right [("contained_by", toJson containedBy)]
  | .overlap left right => expressionPair emit "overlap" left right
  | .extract field arg => do pure (node "extract" [("field", toJson field), ("arg", ← emit arg)])
  | .substring form args => do pure (node "substring"
      [("form", toJson (substringLabel form)), ("args", toJson (← args.mapM emit))])
  | .cast arg type => do pure (node "cast" [("arg", ← emit arg), ("type", ← dataTypeJson type)])
  | .collate arg name => do pure (node "collate" [("arg", ← emit arg), ("name", toJson name)])
  | .quantifiedComparison op quantifier left right => expressionPair emit "quantified_comparison"
      left right [("op", toJson (fullBinaryLabel op.toBinOp)),
        ("quantifier", toJson (quantifierLabel quantifier))]
  | .subscript base indices => do pure (node "subscript"
      [("base", ← emit base), ("indices", toJson (← indices.mapM (subscriptJson emit)))])
  | value => collectionExpressionJson emit query value

/-- Export predicate and aggregate decorations without discarding their independent flags. -/
def decoratedExpressionJson (emit : Advanced.Expr → Except String Json)
    (query : Advanced.Query → Except String Json) : Advanced.Expr → Except String Json
  | .countAll => pure (node "count_all")
  | .aggregate fn arg distinct => do pure (node "aggregate"
      [("fn", toJson (aggregateLabel fn)), ("arg", ← emit arg), ("distinct", toJson distinct)])
  | .isNull arg negated => do pure (node "is_null" [("arg", ← emit arg), ("negated", toJson negated)])
  | .distinctFrom left right negated =>
      expressionPair emit "distinct_from" left right [("negated", toJson negated)]
  | .pattern arg pattern insensitive => do pure (node "pattern"
      [("arg", ← emit arg), ("pattern", ← emit pattern), ("insensitive", toJson insensitive)])
  | .regex arg pattern insensitive negated => do pure (node "regex"
      [("arg", ← emit arg), ("pattern", ← emit pattern),
       ("insensitive", toJson insensitive), ("negated", toJson negated)])
  | value => specialExpressionJson emit query value

/-- Dispatch every typed scalar node; leaf values remain exact and unresolved. -/
def expressionShape (emit : Advanced.Expr → Except String Json)
    (query : Advanced.Query → Except String Json) : Advanced.Expr → Except String Json
  | .literal value => pure (valueJson value)
  | .null => pure (node "sql_null")
  | .contextValue value => pure (node "context_value" [("value", toJson (contextLabel value))])
  | .decimal lexeme => pure (node "decimal" [("lexeme", toJson lexeme)])
  | .parameter parameter => pure (fullParameterJson parameter)
  | .column qualifier name schema => pure (node "column"
      [("name", toJson name), ("qualifier", toJson qualifier), ("schema", toJson schema)])
  | .binary op left right => expressionPair emit "binary" left right [("op", toJson (fullBinaryLabel op))]
  | .concat left right => expressionPair emit "binary" left right [("op", toJson "concat")]
  | .unary op arg => do pure (node "unary" [("op", toJson (unaryLabel op)), ("arg", ← emit arg)])
  | value => decoratedExpressionJson emit query value

end SQLollaParser
