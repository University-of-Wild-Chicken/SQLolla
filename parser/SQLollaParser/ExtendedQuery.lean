import SQLollaParser.ExtendedExpression
import SQLollaParser.Query

/-! Explicit complete query modifiers; order, limits and locking are never omitted. -/
open Lean (Json toJson)
open SQLean SQLean.Advanced
namespace SQLollaParser

/-- Export projection positions with the full expression callback. -/
def fullItemJson (emit : Advanced.Expr → Except String Json) : Advanced.SelectItem → Except String Json
  | .all qualifier schema => pure (node "wildcard"
      [("qualifier", toJson qualifier), ("schema", toJson schema)])
  | .expression value alias => do pure (node "expression" [("value", ← emit value), ("alias", toJson alias)])

/-- Preserve explicit NULL placement and the direction normalized by the upstream AST. -/
def orderJson (emit : Advanced.Expr → Except String Json) (order : Advanced.OrderBy) : Except String Json := do
  pure (Json.mkObj [("expression", ← emit order.expr),
    ("direction", toJson (if order.descending then "desc" else "asc")),
    ("nulls", toJson (order.nullsFirst.map fun first => if first then "first" else "last"))])

/-- Retain every lock clause in source order, including targeted aliases and wait policy. -/
def lockingJson (locking : LockingClause) : Json :=
  Json.mkObj [("strength", toJson (match locking.strength with
      | .update => "update" | .noKeyUpdate => "no_key_update"
      | .share => "share" | .keyShare => "key_share")),
    ("targets", toJson locking.targets),
    ("wait_policy", toJson (match locking.waitPolicy with
      | .block => "block" | .nowait => "nowait" | .skipLocked => "skip_locked"))]

/-- Preserve SELECT grouping and DISTINCT ON independently from the ordinary DISTINCT flag. -/
def fullSelectJson (emit : Advanced.Expr → Except String Json)
    (source : From → Except String Json) (select : Advanced.Select) : Except String Json := do
  pure (node "select" [("items", toJson (← select.items.mapM (fullItemJson emit))),
    ("source", ← exportOptional source select.source),
    ("where", ← exportOptional emit select.whereClause), ("distinct", toJson select.distinct),
    ("group_by", toJson (← select.groupBy.mapM emit)), ("having", ← exportOptional emit select.having),
    ("distinct_on", toJson (← select.distinctOn.mapM emit))])

/-- Keep set-operation branches as complete queries, including branch-local modifiers. -/
def fullBodyJson (emit : Advanced.Expr → Except String Json)
    (query : Advanced.Query → Except String Json) (source : From → Except String Json) :
    QueryBody → Except String Json
  | .select select => fullSelectJson emit source select
  | .values rows => do pure (node "values" [("rows", toJson (← rows.mapM (fun row => row.mapM emit)))])
  | .set op all left right => do pure (node "set_operation" [("op", toJson (setLabel op)),
      ("all", toJson all), ("left", ← query left), ("right", ← query right)])

/-- Export every Query field so semantic consumers can reject any unmodeled modifier. -/
def queryShape (emit : Advanced.Expr → Except String Json)
    (query : Advanced.Query → Except String Json) (source : From → Except String Json)
    (value : Advanced.Query) : Except String Json := do
  let ctes ← value.ctes.mapM fun cte => do
    pure (Json.mkObj [("name", toJson cte.name), ("columns", toJson cte.columns),
      ("query", ← query cte.query)])
  pure (node "query" [("body", ← fullBodyJson emit query source value.body),
    ("ctes", toJson ctes), ("recursive", toJson value.recursive),
    ("order_by", toJson (← value.orderBy.mapM (orderJson emit))),
    ("limit", ← exportOptional emit value.limit), ("offset", ← exportOptional emit value.offset),
    ("offset_first", toJson value.offsetFirst), ("locking", toJson (value.locking.map lockingJson))])

/-- Keep USING and NATURAL joins structurally distinct from predicates and CROSS joins. -/
def conditionJson (emit : Advanced.Expr → Except String Json) : JoinCondition → Except String Json
  | .none => pure (node "none")
  | .natural => pure (node "natural")
  | .using columns => pure (node "using" [("columns", toJson columns)])
  | .on expression => do pure (node "on" [("expression", ← emit expression)])

/-- Preserve anonymous derived syntax, LATERAL and every join condition without rewriting. -/
def sourceShape (emit : Advanced.Expr → Except String Json)
    (query : Advanced.Query → Except String Json) (source : From → Except String Json) : From → Except String Json
  | .table name alias columns schema => pure (node "table" [("name", toJson name),
      ("alias", toJson alias), ("columns", toJson columns), ("schema", toJson schema)])
  | .derived value alias columns lateral => do pure (node "derived"
      [("query", ← query value), ("alias", toJson alias), ("columns", toJson columns),
       ("lateral", toJson lateral)])
  | .derivedAnonymous value lateral => do pure (node "derived_anonymous"
      [("query", ← query value), ("lateral", toJson lateral)])
  | .join kind left right condition => do pure (node "join"
      [("kind", toJson (joinLabel kind)), ("left", ← source left), ("right", ← source right),
       ("condition", ← conditionJson emit condition)])

mutual
  /-- Bound expression traversal including nested query syntax; emit no partial tree. -/
  def expressionV2 : Nat → Advanced.Expr → Except String Json
    | 0, _ => .error "resource.expression_depth"
    | fuel + 1, value => expressionShape (expressionV2 fuel) (queryV2 fuel) value

  /-- Bound complete query traversal across CTEs, sources and query-bearing expressions. -/
  def queryV2 : Nat → Advanced.Query → Except String Json
    | 0, _ => .error "resource.query_depth"
    | fuel + 1, value => queryShape (expressionV2 fuel) (queryV2 fuel) (sourceV2 fuel) value

  /-- Use the same decreasing budget for nested relational sources. -/
  def sourceV2 : Nat → From → Except String Json
    | 0, _ => .error "resource.source_depth"
    | fuel + 1, value => sourceShape (expressionV2 fuel) (queryV2 fuel) (sourceV2 fuel) value
end

end SQLollaParser
