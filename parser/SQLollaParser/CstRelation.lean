import SQLollaParser.CstExpression

/-! Complete-field lowering of selected query/source grammar productions. -/
open Lean (Json toJson)
open SQLean.PostgresGrammar
namespace SQLollaParser

/-- Decode ordered names from the explicitly supported identifier-list grammars. -/
def cstNames (source : String) (tree : GrammarTree) : Except String (List String) := do
  (← treeList [534, 535, 2327, 2328] 512 tree).mapM (identifier source)

/-- Preserve optional aliases and their positional column-renaming lists. -/
def cstAlias (source : String) : GrammarTree → Except String (Json × Json)
  | .rule 1812 [] => pure (Json.null, toJson ([] : List String))
  | .rule 1811 [.rule 1808 [_, name]] | .rule 1811 [.rule 1810 [name]] => do
      pure (toJson (← identifier source name), toJson ([] : List String))
  | .rule 1811 [.rule 1807 [_, name, _, columns, _]]
  | .rule 1811 [.rule 1809 [name, _, columns, _]] => do
      pure (toJson (← identifier source name), toJson (← cstNames source columns))
  | _ => .error "unsupported.cst_alias"

/-- Admit ordinary table references while rejecting ONLY and inheritance modifiers. -/
def cstTable (source : String) (relation : GrammarTree) (alias columns : Json) : Except String Json := do
  let name ← match relation with
    | .rule 1828 [name] => pure name | _ => throw "unsupported.cst_relation_modifier"
  match ← identifierParts source name with
  | [name] => pure (node "table" [("name", toJson name), ("schema", Json.null),
      ("alias", alias), ("columns", columns)])
  | [schema, name] => pure (node "table" [("name", toJson name), ("schema", toJson schema),
      ("alias", alias), ("columns", columns)])
  | _ => throw "unsupported.cst_catalog_qualification"

/-- Build a derived source while retaining alias absence and LATERAL separately. -/
def cstDerived (source : String) (query : GrammarTree → Except String Json)
    (body alias : GrammarTree) (lateral : Bool) : Except String Json := do
  let (alias, columns) ← cstAlias source alias
  let query ← query body
  if alias == Json.null then
    pure (node "derived_anonymous" [("query", query), ("lateral", toJson lateral)])
  else pure (node "derived" [("query", query), ("alias", alias),
    ("columns", columns), ("lateral", toJson lateral)])

/-- Map each explicit join flavor without accepting unknown join grammar. -/
def cstJoinKind : GrammarTree → Except String String
  | .rule 1820 [_, _] => pure "full" | .rule 1821 [_, _] => pure "left"
  | .rule 1822 [_, _] => pure "right" | .rule 1823 [_] => pure "inner"
  | _ => .error "unsupported.cst_join_kind"

/-- Preserve ON predicates and reject USING aliases unless a dedicated AST field exists. -/
def cstJoinCondition (source : String) (emit : GrammarTree → Except String Json) : GrammarTree → Except String Json
  | .rule 1827 [_, expression] => do pure (node "on" [("expression", ← emit expression)])
  | .rule 1826 [_, _, names, _, alias] => do
      requireEmpty alias
      pure (node "using" [("columns", toJson (← cstNames source names))])
  | _ => .error "unsupported.cst_join_condition"

/-- Build explicit join nodes from already classified source productions. -/
def cstJoin (emit : GrammarTree → Except String Json) (kind : String)
    (left right : GrammarTree) (condition : Json) : Except String Json := do
  pure (node "join" [("kind", toJson kind), ("left", ← emit left),
    ("right", ← emit right), ("condition", condition)])

/-- Lower supported joins with complete child-shape matching. -/
def cstJoined (source : String) (expression emit : GrammarTree → Except String Json) : GrammarTree → Except String Json
  | .rule 1801 [_, value, _] => emit value
  | .rule 1802 [left, _, _, right] => cstJoin emit "cross" left right (node "none")
  | .rule 1803 [left, kind, _, right, condition] => do
      cstJoin emit (← cstJoinKind kind) left right (← cstJoinCondition source expression condition)
  | .rule 1804 [left, _, right, condition] => do
      cstJoin emit "inner" left right (← cstJoinCondition source expression condition)
  | .rule 1805 [left, _, kind, _, right] => do cstJoin emit (← cstJoinKind kind) left right (node "natural")
  | .rule 1806 [left, _, _, right] => cstJoin emit "inner" left right (node "natural")
  | _ => .error "unsupported.cst_join"

/-- Fold comma-separated FROM occurrences into explicit CROSS joins. -/
def cstFromList (emit : GrammarTree → Except String Json) (tree : GrammarTree) : Except String Json := do
  let sources ← (← treeList [1789, 1790] 512 tree).mapM emit
  match sources with
  | [] => throw "invariant.cst_empty_from"
  | first :: rest => pure (rest.foldl (fun left right => node "join" [("kind", toJson "cross"),
      ("left", left), ("right", right), ("condition", node "none")]) first)

/-- Preserve each relational source, rejecting functions/TABLESAMPLE/other unmapped source forms. -/
def cstSourceShape (source : String) (expression query emit : GrammarTree → Except String Json)
    (tree : GrammarTree) : Except String Json := do
  match tree with
  | .rule 1791 [relation, alias] =>
      let (alias, columns) ← cstAlias source alias
      cstTable source relation alias columns
  | .rule 1797 [body, alias] => cstDerived source query body alias false
  | .rule 1798 [_, body, alias] => cstDerived source query body alias true
  | .rule 1799 [joined] => emit joined
  | _ =>
      if treeName tree == "from_list" then cstFromList emit tree
      else cstJoined source expression emit tree

/-- Preserve qualifier and schema components without splitting quoted names on dots. -/
def wildcardNames : List String → Except String Json
  | [qualifier] => pure (node "wildcard" [("qualifier", toJson qualifier), ("schema", Json.null)])
  | [schema, qualifier] => pure (node "wildcard" [("qualifier", toJson qualifier), ("schema", toJson schema)])
  | _ => .error "unsupported.cst_wildcard_qualification"

/-- Schema-qualified expansion retains its exact positional binding identity. -/
theorem wildcardNames_schema (schema qualifier : String) :
    wildcardNames [schema, qualifier] = .ok (node "wildcard"
      [("qualifier", toJson qualifier), ("schema", toJson schema)]) := rfl

/-- Recognize only a complete column reference ending in a star; other expressions keep normal dispatch. -/
def cstWildcard (source : String) : GrammarTree → Except String (Option Json)
  | .rule 1958 [.rule 2047 [.rule 2269 [name, indirection]]] => do
      let items ← treeList [2276, 2277] 512 indirection
      match items.getLast? with
      | some (.rule 2271 [_, _]) => do
          let components ← items.dropLast.mapM fun item => do
            match item with
            | .rule 2270 [_, field] => identifier source field
            | _ => throw "unsupported.cst_wildcard_indirection"
          pure (some (← wildcardNames ((← identifier source name) :: components)))
      | _ => pure none
  | _ => pure none

/-- Export one target in original positional order, keeping alias syntax separate. -/
def cstTarget (source : String) (emit : GrammarTree → Except String Json) : GrammarTree → Except String Json
  | .rule 2319 [value, _, alias] | .rule 2320 [value, alias] => do
      pure (node "expression" [("value", ← emit value), ("alias", toJson (← identifier source alias))])
  | .rule 2321 [value] => do
      if let some wildcard ← cstWildcard source value then pure wildcard
      else pure (node "expression" [("value", ← emit value), ("alias", Json.null)])
  | .rule 2322 [_] => pure (node "wildcard" [("qualifier", Json.null), ("schema", Json.null)])
  | _ => .error "unsupported.cst_target"

/-- Keep full projection and RETURNING lists, including the permitted empty SELECT list. -/
def cstTargets (source : String) (emit : GrammarTree → Except String Json)
    (tree : GrammarTree) : Except String (List Json) := do
  match tree with
  | .rule 2316 [] | .rule 1607 [] => pure []
  | .rule 2315 [items] | .rule 1606 [_, items] =>
      (← treeList [2317, 2318] 512 items).mapM (cstTarget source emit)
  | _ => (← treeList [2317, 2318] 512 tree).mapM (cstTarget source emit)

/-- Lower WHERE/HAVING only when their complete ordinary-expression shape is represented. -/
def cstPredicateClause (emit : GrammarTree → Except String Json) : GrammarTree → Except String Json
  | .rule 1851 [] | .rule 1854 [] | .rule 1771 [] => pure Json.null
  | .rule 1850 [_, expression] | .rule 1852 [_, expression] | .rule 1770 [_, expression] => emit expression
  | _ => .error "unsupported.cst_predicate_clause"

/-- Preserve ordinary grouping; GROUPING SETS and GROUP BY DISTINCT remain separate refusals. -/
def cstGrouping (emit : GrammarTree → Except String Json) : GrammarTree → Except String (List Json)
  | .rule 1758 [] => pure []
  | .rule 1757 [_, _, quantifier, items] => do
      requireEmpty quantifier
      (← treeList [1759, 1760] 512 items).mapM fun item => match item with
        | .rule 1761 [expression] => emit expression | _ => .error "unsupported.cst_grouping_item"
  | _ => .error "unsupported.cst_grouping"

/-- Reject INTO and named WINDOW clauses before constructing a complete SELECT body. -/
def cstSelectQuantifier (emit : GrammarTree → Except String Json) : GrammarTree → Except String (Bool × List Json)
  | .rule 1721 [] | .rule 1720 [_] => pure (false, [])
  | .rule 1718 [_] => pure (true, [])
  | .rule 1719 [_, _, _, items, _] => do pure (false, ← cstExpressionList emit items)
  | _ => .error "unsupported.cst_distinct"

/-- Reject INTO and named WINDOW clauses before constructing a complete SELECT body. -/
def cstSelect (source : String) (expression fromExport : GrammarTree → Except String Json)
    (children : List GrammarTree) : Except String Json := do
  match children with
  | [_, all, targets, into, fromClause, predicate, grouping, having, window] =>
      requireEmpty into
      requireEmpty window
      let (distinct, distinctOn) ← cstSelectQuantifier expression all
      let fromValue ← match fromClause with
        | .rule 1788 [] => pure Json.null | .rule 1787 [_, list] => fromExport list
        | _ => throw "unsupported.cst_from_clause"
      pure (node "select" [("items", toJson (← cstTargets source expression targets)),
        ("source", fromValue), ("where", ← cstPredicateClause expression predicate),
        ("group_by", toJson (← cstGrouping expression grouping)),
        ("having", ← cstPredicateClause expression having), ("distinct", toJson distinct),
        ("distinct_on", toJson distinctOn)])
  | _ => throw "unsupported.cst_select_shape"

end SQLollaParser
