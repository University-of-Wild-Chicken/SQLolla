import SQLollaParser.Expression

/-! Complete-field export of the admitted query/source fragment.
Every remaining query modifier or source constructor is rejected explicitly.
-/

open Lean (Json toJson)
open SQLean
open SQLean.Advanced

namespace SQLollaParser

/-- Keep set-operation kind separate from its duplicate-preservation flag. -/
def setLabel : SetOp → String
  | .union => "union" | .intersect => "intersect" | .except => "except"

/-- Preserve the exact outer-join flavor without treating it as an inner join. -/
def joinLabel : JoinKind → String
  | .inner => "inner" | .left => "left" | .right => "right"
  | .full => "full" | .cross => "cross"

/-- Reject all query-level syntax not carried by this version of the AST contract. -/
def checkQueryModifiers (query : SQLean.Advanced.Query) : Except String Unit := do
  if query.recursive then throw "unsupported.recursive_cte"
  if !query.orderBy.isEmpty then throw "unsupported.order_by"
  if query.limit.isSome || query.offset.isSome || query.offsetFirst then
    throw "unsupported.pagination"
  if !query.locking.isEmpty then throw "unsupported.locking"

/-- Export SELECT only after excluding unrepresented grouping and DISTINCT ON fields. -/
def selectJson (sourceExport : From → Except String Json) (select : Select) :
    Except String Json := do
  if !select.groupBy.isEmpty || select.having.isSome then throw "unsupported.grouping"
  if !select.distinctOn.isEmpty then throw "unsupported.distinct_on"
  let source ← match select.source with
    | none => pure Json.null
    | some fromSource => sourceExport fromSource
  pure (node "select" [("items", toJson (← select.items.mapM itemJson)),
    ("source", source), ("where", ← optionalExpression select.whereClause),
    ("distinct", toJson select.distinct)])

/-- Admit only condition-free CROSS joins or ON joins; never drop USING/NATURAL semantics. -/
def joinJson (sourceExport : From → Except String Json) (kind : JoinKind)
    (left right : From) (condition : JoinCondition) : Except String Json := do
  match kind, condition with
  | .cross, .none => pure (node "product"
      [("left", ← sourceExport left), ("right", ← sourceExport right)])
  | .cross, _ => .error "unsupported.cross_join_condition"
  | _, .on expr => pure (node "join" [("kind", toJson (joinLabel kind)),
      ("left", ← sourceExport left), ("right", ← sourceExport right),
      ("on", ← expressionJson 64 expr)])
  | _, _ => .error "unsupported.join_condition"

/-- Export one query body using depth-bounded callbacks for recursive syntax. -/
def bodyJson (queryExport : SQLean.Advanced.Query → Except String Json)
    (sourceExport : From → Except String Json) : QueryBody → Except String Json
  | .select select => selectJson sourceExport select
  | .values rows => do
      pure (node "values" [("rows", toJson (← rows.mapM (fun row => row.mapM (expressionJson 64))))])
  | .set op all left right => do
      pure (node "set_operation" [("op", toJson (setLabel op)), ("all", toJson all),
        ("left", ← queryExport left), ("right", ← queryExport right)])

mutual
  /-- Preserve ordered nonrecursive CTE declarations and bound the whole query/source tree. -/
  def queryJson : Nat → SQLean.Advanced.Query → Except String Json
    | 0, _ => .error "resource.query_depth"
    | fuel + 1, query => do
      checkQueryModifiers query
      let body ← bodyJson (queryJson fuel) (sourceJson fuel) query.body
      if query.ctes.isEmpty then return body
      let ctes ← query.ctes.mapM fun cte => do
        pure (Json.mkObj [("name", toJson cte.name), ("columns", toJson cte.columns),
          ("query", ← queryJson fuel cte.query)])
      pure (node "with" [("ctes", toJson ctes), ("body", body)])

  /-- Preserve source aliases and column lists; require an explicit non-LATERAL derived alias. -/
  def sourceJson : Nat → From → Except String Json
    | 0, _ => .error "resource.source_depth"
    | fuel + 1, source => do
      match source with
      | .table name alias columns schema => pure (node "table"
          [("name", toJson name), ("alias", toJson alias),
           ("columns", toJson columns), ("schema", toJson schema)])
      | .derived query alias columns false => pure (node "derived"
          [("query", ← queryJson fuel query), ("alias", toJson alias),
           ("columns", toJson columns)])
      | .derived _ _ _ true => .error "unsupported.lateral"
      | .derivedAnonymous _ _ => .error "unsupported.anonymous_derived"
      | .join kind left right condition =>
          joinJson (sourceJson fuel) kind left right condition
end

/-- Restrict this adapter version to query statements; all DML and DDL fail closed. -/
def statementJson : SQLean.Advanced.Statement → Except String Json
  | .query query => queryJson 64 query
  | _ => .error "unsupported.statement"

end SQLollaParser
