import SQLollaParser.CstQuery

/-! Certified statement grammar extensions retain candidate effects as syntax, not execution. -/
open Lean (Json toJson)
open SQLean.PostgresGrammar
namespace SQLollaParser

/-- Require a plain DML column target; field/subscript assignment needs additional explicit syntax. -/
def cstAssignment (source : String) (emit : GrammarTree → Except String Json) : GrammarTree → Except String Json
  | .rule 1630 [.rule 1632 [column, indirection], _, value] => do
      requireEmpty indirection
      pure (Json.mkObj [("column", toJson (← identifier source column)), ("value", ← emit value)])
  | _ => .error "unsupported.cst_assignment"

/-- Preserve assignment evaluation order and every target occurrence. -/
def cstAssignments (source : String) (emit : GrammarTree → Except String Json)
    (tree : GrammarTree) : Except String (List Json) := do
  (← treeList [1628, 1629] 512 tree).mapM (cstAssignment source emit)

/-- Preserve ordinary unqualified mutation targets; schema-qualified DML needs its own AST field. -/
def cstDmlTarget (source : String) : GrammarTree → Except String (String × Json)
  | .rule 1835 [.rule 1828 [name]] | .rule 1588 [name] => do
      pure (← identifier source name, Json.null)
  | .rule 1836 [.rule 1828 [name], alias] | .rule 1837 [.rule 1828 [name], _, alias]
  | .rule 1589 [name, _, alias] => do pure (← identifier source name, toJson (← identifier source alias))
  | _ => .error "unsupported.cst_dml_target"

/-- Decode explicit INSERT columns without erasing indirection. -/
def cstInsertColumns (source : String) (tree : GrammarTree) : Except String (List String) := do
  (← treeList [1597, 1598] 512 tree).mapM fun column => do
    match column with
    | .rule 1599 [name, indirection] => do requireEmpty indirection; identifier source name
    | _ => throw "unsupported.cst_insert_column"

/-- Preserve VALUES as an insertion source only when its whole query has no modifiers. -/
def insertionQuery (value : Json) : Json :=
  match value.getObjVal? "body" with
  | .ok body =>
      if (body.getObjValAs? String "tag").toOption == some "values" && value == plainQuery body then body
      else node "query_source" [("query", value)]
  | _ => node "query_source" [("query", value)]

/-- Account for the whole INSERT source; OVERRIDING variants remain explicit refusals. -/
def cstInsertRest (source : String) (query : GrammarTree → Except String Json) :
    GrammarTree → Except String (List String × Json)
  | .rule 1590 [body] => do pure ([], insertionQuery (← query body))
  | .rule 1592 [_, columns, _, body] => do
      pure (← cstInsertColumns source columns, insertionQuery (← query body))
  | .rule 1594 [_, _] => pure ([], node "default_values")
  | _ => .error "unsupported.cst_insert_source"

/-- Require plain conflict columns; collation/operator classes/order are not silently erased. -/
def cstIndexColumn (source : String) : GrammarTree → Except String String
  | .rule 1083 [name, .rule 1081 options] => do
      options.forM requireEmpty
      identifier source name
  | _ => .error "unsupported.cst_conflict_index"

/-- Retain partial-index inference predicates separately from DO UPDATE predicates. -/
def cstConflictTarget (source : String) (emit : GrammarTree → Except String Json) : GrammarTree → Except String Json
  | .rule 1605 [] => pure Json.null
  | .rule 1604 [_, _, name] => do pure (node "constraint" [("name", toJson (← identifier source name))])
  | .rule 1603 [_, columns, _, predicate] => do
      pure (node "columns" [("names", toJson (← (← treeList [1079, 1080] 512 columns).mapM (cstIndexColumn source))),
        ("where", ← cstPredicateClause emit predicate)])
  | _ => .error "unsupported.cst_conflict_target"

/-- Export the complete ON CONFLICT action and both independently scoped predicates. -/
def cstConflict (source : String) (emit : GrammarTree → Except String Json) : GrammarTree → Except String Json
  | .rule 1602 [] => pure Json.null
  | .rule 1601 [_, _, target, _, _] => do pure (Json.mkObj
      [("target", ← cstConflictTarget source emit target), ("action", node "nothing")])
  | .rule 1600 [_, _, target, _, _, _, assignments, predicate] => do
      pure (Json.mkObj [("target", ← cstConflictTarget source emit target),
        ("action", node "update" [("assignments", toJson (← cstAssignments source emit assignments)),
          ("where", ← cstPredicateClause emit predicate)])])
  | _ => .error "unsupported.cst_conflict"

/-- Preserve INSERT with optional statement-valued CTEs and complete RETURNING data. -/
def cstInsert (source : String) (emit query statement : GrammarTree → Except String Json) : GrammarTree → Except String Json
  | .rule 1587 [withClause, _, _, target, rest, conflict, returning] => do
      let (table, alias) ← cstDmlTarget source target
      let (columns, input) ← cstInsertRest source query rest
      let body := node "insert" [("table", toJson table), ("alias", alias), ("columns", toJson columns),
        ("source", input), ("on_conflict", ← cstConflict source emit conflict),
        ("returning", toJson (← cstTargets source emit returning))]
      cstOptionalWith source statement withClause body
  | _ => .error "unsupported.cst_insert"

/-- Distinguish missing FROM/USING from a present relational source. -/
def cstMutationSource (fromExport : GrammarTree → Except String Json) : GrammarTree → Except String Json
  | .rule 1788 [] | .rule 1610 [] => pure Json.null
  | .rule 1787 [_, items] | .rule 1609 [_, items] => fromExport items
  | _ => .error "unsupported.cst_mutation_source"

/-- Export UPDATE FROM with every assignment, predicate and returned column retained. -/
def cstUpdate (source : String) (emit fromExport statement : GrammarTree → Except String Json) : GrammarTree → Except String Json
  | .rule 1627 [withClause, _, target, _, assignments, fromClause, predicate, returning] => do
      let (table, alias) ← cstDmlTarget source target
      let body := node "update" [("table", toJson table), ("alias", alias),
        ("assignments", toJson (← cstAssignments source emit assignments)),
        ("from", ← cstMutationSource fromExport fromClause), ("where", ← cstPredicateClause emit predicate),
        ("returning", toJson (← cstTargets source emit returning))]
      cstOptionalWith source statement withClause body
  | _ => .error "unsupported.cst_update"

/-- Export DELETE USING without converting it to an unconditional table deletion. -/
def cstDelete (source : String) (emit fromExport statement : GrammarTree → Except String Json) : GrammarTree → Except String Json
  | .rule 1608 [withClause, _, _, target, usingClause, predicate, returning] => do
      let (table, alias) ← cstDmlTarget source target
      let body := node "delete" [("table", toJson table), ("alias", alias),
        ("using", ← cstMutationSource fromExport usingClause), ("where", ← cstPredicateClause emit predicate),
        ("returning", toJson (← cstTargets source emit returning))]
      cstOptionalWith source statement withClause body
  | _ => .error "unsupported.cst_delete"

/-- Export the corpus CREATE VIEW form only after checking all currently unmodeled clauses. -/
def cstCreateView (source : String) (query : GrammarTree → Except String Json) : GrammarTree → Except String Json
  | .rule 1459 [_, temporary, _, name, columns, options, _, body, check] => do
      requireEmpty temporary
      requireEmpty options
      requireEmpty check
      let columns ← match columns with
        | .rule 533 [] => pure [] | .rule 532 [_, names, _] => cstNames source names
        | _ => throw "unsupported.cst_view_columns"
      pure (node "create_view" [("name", toJson (← identifierParts source name)), ("columns", toJson columns),
        ("query", ← query body), ("replace", toJson false), ("temporary", toJson false),
        ("recursive", toJson false), ("options", toJson ([] : List Json)), ("check_option", Json.null)])
  | _ => .error "unsupported.cst_create_view"

/-- Preserve DROP mode while distinguishing its explicit names from other object families. -/
def cstDropMode : GrammarTree → Except String Bool
  | .rule 142 [_] => pure true | .rule 143 [_] | .rule 144 [] => pure false
  | _ => .error "unsupported.cst_drop_mode"

/-- Lower only VIEW drops; an unrelated DROP object cannot inherit VIEW semantics. -/
def cstDropView (source : String) (tree : GrammarTree) : Except String Json := do
  let (names, mode, ifExists) ← match tree with
    | .rule 892 [_, .rule 905 [_], names, mode] => pure (names, mode, false)
    | .rule 891 [_, .rule 905 [_], _, _, names, mode] => pure (names, mode, true)
    | _ => throw "unsupported.cst_drop_view"
  pure (node "drop_view" [("names", toJson (← (← treeList [932, 933] 512 names).mapM (identifierParts source))),
    ("if_exists", toJson ifExists), ("cascade", toJson (← cstDropMode mode))])

end SQLollaParser
