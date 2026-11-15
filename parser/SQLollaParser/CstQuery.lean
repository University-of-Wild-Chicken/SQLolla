import SQLollaParser.CstRelation

/-! Query decoration and statement-valued CTE lowering over pinned certified productions. -/
open Lean (Json toJson)
open SQLean.PostgresGrammar
namespace SQLollaParser

/-- Preserve every CTE declaration field and reject SEARCH/CYCLE until their AST fields exist. -/
def cstCte (source : String) (statement : GrammarTree → Except String Json) : GrammarTree → Except String Json
  | .rule 1690 [name, columns, _, materialized, _, body, _, search, cycle] => do
      requireEmpty search
      requireEmpty cycle
      let columns ← match columns with
        | .rule 1550 [] => pure [] | .rule 1549 [_, names, _] => cstNames source names
        | _ => throw "unsupported.cst_cte_columns"
      let materialized ← match materialized with
        | .rule 1693 [] => pure Json.null | .rule 1691 [_] => pure (toJson true)
        | .rule 1692 [_, _] => pure (toJson false) | _ => throw "unsupported.cst_cte_materialized"
      pure (Json.mkObj [("name", toJson (← identifier source name)), ("columns", toJson columns),
        ("materialized", materialized), ("statement", ← statement body)])
  | _ => .error "unsupported.cst_cte"

/-- Keep WITH over mutations and writable CTE bodies separate from ordinary query-only CTEs. -/
def cstWith (source : String) (statement : GrammarTree → Except String Json)
    (clause : GrammarTree) (body : Json) : Except String Json := do
  let (recursive, ctes) ← match clause with
    | .rule 1685 [_, ctes] | .rule 1686 [_, ctes] => pure (false, ctes)
    | .rule 1687 [_, _, ctes] => pure (true, ctes)
    | _ => throw "unsupported.cst_with"
  pure (node "with_statement" [("recursive", toJson recursive),
    ("ctes", toJson (← (← treeList [1688, 1689] 512 ctes).mapM (cstCte source statement))), ("body", body)])

/-- Handle the optional WITH prefix on DML without silently discarding it. -/
def cstOptionalWith (source : String) (statement : GrammarTree → Except String Json)
    (clause : GrammarTree) (body : Json) : Except String Json :=
  match clause with
  | .rule 1701 [] => pure body
  | .rule 1700 [withClause] => cstWith source statement withClause body
  | _ => .error "unsupported.cst_optional_with"

/-- Export every VALUES cell, including DEFAULT syntax handled by the expression lowerer. -/
def cstRows (emit : GrammarTree → Except String Json) : Nat → GrammarTree → Except String (List Json)
  | 0, _ => .error "resource.cst_values_depth"
  | _ + 1, .rule 1785 [_, _, expressions, _] => do pure [toJson (← cstExpressionList emit expressions)]
  | fuel + 1, .rule 1786 [prior, _, _, expressions, _] => do
      pure ((← cstRows emit fuel prior) ++ [toJson (← cstExpressionList emit expressions)])
  | _, _ => .error "unsupported.cst_values"

/-- Preserve ALL versus duplicate-eliminating set operations. -/
def cstSetAll : GrammarTree → Except String Bool
  | .rule 1715 [_] => pure true
  | .rule 1716 [_] | .rule 1717 [] => pure false
  | _ => .error "unsupported.cst_set_quantifier"

/-- Keep limit and offset expressions independent from evaluation or runtime sign checking. -/
def cstPaginationValue (emit : GrammarTree → Except String Json) : GrammarTree → Except String Json
  | .rule 1737 [_, .rule 1745 [value]] | .rule 1743 [_, .rule 1747 [value]] => emit value
  | _ => .error "unsupported.cst_pagination_form"

/-- Decode exactly the four LIMIT/OFFSET combinations into explicit syntax fields. -/
def cstPaginationInner (emit : GrammarTree → Except String Json) : GrammarTree → Except String (Json × Json × Bool)
  | .rule 1731 [limit, offset] => do pure (← cstPaginationValue emit limit, ← cstPaginationValue emit offset, false)
  | .rule 1732 [offset, limit] => do pure (← cstPaginationValue emit limit, ← cstPaginationValue emit offset, true)
  | .rule 1733 [limit] => do pure (← cstPaginationValue emit limit, Json.null, false)
  | .rule 1734 [offset] => do pure (Json.null, ← cstPaginationValue emit offset, false)
  | _ => .error "unsupported.cst_pagination"

/-- Retain pagination spelling order; FETCH/WITH TIES require additional explicit fields. -/
def cstPagination (emit : GrammarTree → Except String Json) : GrammarTree → Except String (Json × Json × Bool)
  | .rule 1736 [] => pure (Json.null, Json.null, false)
  | .rule 1735 [value] => cstPaginationInner emit value
  | value => cstPaginationInner emit value

/-- Preserve lock strength and wait policy without asserting isolation or concurrency semantics. -/
def cstLock (source : String) : GrammarTree → Except String Json
  | .rule 1778 [strength, targets, policy] => do
      let strength ← match strength with
        | .rule 1779 [_, _] => pure "update" | .rule 1780 [_, _, _, _] => pure "no_key_update"
        | .rule 1781 [_, _] => pure "share" | .rule 1782 [_, _, _] => pure "key_share"
        | _ => throw "unsupported.cst_lock_strength"
      let targets ← match targets with
        | .rule 1784 [] => pure []
        | .rule 1783 [_, names] => (← treeList [2323, 2324] 512 names).mapM (identifier source)
        | _ => throw "unsupported.cst_lock_targets"
      let policy ← match policy with
        | .rule 1624 [_] => pure "nowait" | .rule 1625 [_, _] => pure "skip_locked"
        | .rule 1626 [] => pure "block" | _ => throw "unsupported.cst_lock_policy"
      pure (Json.mkObj [("strength", toJson strength), ("targets", toJson targets), ("wait_policy", toJson policy)])
  | _ => .error "unsupported.cst_lock"

/-- Preserve ordered lock clauses and reject FOR READ ONLY as an unrepresented decoration. -/
def cstLocking (source : String) : GrammarTree → Except String (List Json)
  | .rule 1775 [] => pure []
  | .rule 1774 [.rule 1772 [items]] | .rule 1772 [items] => do
      (← treeList [1776, 1777] 512 items).mapM (cstLock source)
  | _ => .error "unsupported.cst_locking_clause"

/-- Apply each represented query modifier without erasing prior fields. -/
def cstModifier (source : String) (emit : GrammarTree → Except String Json)
    (query : Json) (modifier : GrammarTree) : Except String Json := do
  match treeName modifier with
  | "sort_clause" | "opt_sort_clause" => replaceField query "order_by" (toJson (← cstOrdering emit modifier))
  | "for_locking_clause" | "opt_for_locking_clause" =>
      replaceField query "locking" (toJson (← cstLocking source modifier))
  | "select_limit" | "opt_select_limit" =>
      let (limit, offset, first) ← cstPagination emit modifier
      let query ← replaceField query "limit" limit
      let query ← replaceField query "offset" offset
      replaceField query "offset_first" (toJson first)
  | _ => throw s!"unsupported.cst_query_modifier.{treeName modifier}"

/-- Decorate one query body using complete pinned grammar alternatives. -/
def cstDecoratedQuery (source : String) (emit query statement : GrammarTree → Except String Json)
    (tree : GrammarTree) : Except String Json := do
  let (withClause, body, modifiers) ← match tree with
    | .rule 1669 [body, sorting] => pure (none, body, [sorting])
    | .rule 1670 [body, sorting, locking, limits] => pure (none, body, [sorting, locking, limits])
    | .rule 1671 [body, sorting, limits, locking] => pure (none, body, [sorting, limits, locking])
    | .rule 1672 [withClause, body] => pure (some withClause, body, [])
    | .rule 1673 [withClause, body, sorting] => pure (some withClause, body, [sorting])
    | .rule 1674 [withClause, body, sorting, locking, limits] => pure (some withClause, body, [sorting, locking, limits])
    | .rule 1675 [withClause, body, sorting, limits, locking] => pure (some withClause, body, [sorting, limits, locking])
    | _ => throw "unsupported.cst_query"
  let body ← modifiers.foldlM (cstModifier source emit) (← query body)
  match withClause with
  | none => pure body
  | some clause => cstWith source statement clause body

/-- Lower query bodies and wrappers while rejecting all unmatched PostgreSQL productions. -/
def cstQueryShape (source : String) (emit query fromExport statement : GrammarTree → Except String Json)
    (tree : GrammarTree) : Except String Json := do
  match tree with
  | .rule id [child] =>
      if [1664, 1665, 1668, 1676, 1677].contains id then query child
      else if id == 1680 then pure (plainQuery (node "values" [("rows", toJson (← cstRows emit 512 child))]))
      else throw s!"unsupported.cst_query_wrapper.{id}"
  | .rule 1666 [_, child, _] | .rule 1667 [_, child, _] => query child
  | .rule 1678 children | .rule 1679 children => plainQuery <$> cstSelect source emit fromExport children
  | .rule id [left, _, all, right] =>
      if [1682, 1683, 1684].contains id then
        pure (plainQuery (node "set_operation"
          [("op", toJson (if id == 1682 then "union" else if id == 1683 then "intersect" else "except")),
           ("all", toJson (← cstSetAll all)), ("left", ← query left), ("right", ← query right)]))
      else cstDecoratedQuery source emit query statement tree
  | _ => cstDecoratedQuery source emit query statement tree

end SQLollaParser
