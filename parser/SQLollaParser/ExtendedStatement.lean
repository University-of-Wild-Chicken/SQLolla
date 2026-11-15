import SQLollaParser.ExtendedQuery

/-! Structural DML/DDL export only. M2 outcome contracts require separate execution coverage. -/
open Lean (Json toJson)
open SQLean SQLean.Advanced
namespace SQLollaParser

/-- Preserve assignment order and decoded target names without executing an update. -/
def assignmentJson (assignment : Advanced.Assignment) : Except String Json := do
  pure (Json.mkObj [("column", toJson assignment.column), ("value", ← expressionV2 64 assignment.value)])

/-- Preserve INSERT source form; DEFAULT VALUES is not an empty VALUES bag. -/
def insertSourceJson : InsertSource → Except String Json
  | .values rows => do pure (node "values"
      [("rows", toJson (← rows.mapM (fun row => row.mapM (expressionV2 64))))])
  | .query query => do pure (node "query_source" [("query", ← queryV2 64 query)])
  | .defaultValues => pure (node "default_values")

/-- Retain named constraints separately from inferred conflict-column targets. -/
def conflictTargetJson : ConflictTarget → Json
  | .columns names => node "columns" [("names", toJson names)]
  | .constraint name => node "constraint" [("name", toJson name)]

/-- Export conflict actions without converting them into unconditional assignments. -/
def conflictActionJson : ConflictAction → Except String Json
  | .nothing => pure (node "nothing")
  | .update assignments predicate => do pure (node "update"
      [("assignments", toJson (← assignments.mapM assignmentJson)),
       ("where", ← exportOptional (expressionV2 64) predicate)])

/-- Retain absent conflict targets and action predicates exactly as typed upstream. -/
def conflictJson (conflict : OnConflict) : Except String Json := do
  pure (Json.mkObj [("target", conflict.target.map conflictTargetJson |>.getD Json.null),
    ("action", ← conflictActionJson conflict.action)])

/-- Export every INSERT field, including RETURNING and conflict handling. -/
def insertJson (insert : Advanced.Insert) : Except String Json := do
  pure (node "insert" [("table", toJson insert.table), ("alias", toJson insert.alias),
    ("columns", toJson insert.columns), ("source", ← insertSourceJson insert.source),
    ("on_conflict", ← exportOptional conflictJson insert.onConflict),
    ("returning", toJson (← insert.returning.mapM (fullItemJson (expressionV2 64))))])

/-- Export every UPDATE field; absent FROM is an upstream AST limitation, not a dropped field. -/
def updateJson (update : Advanced.Update) : Except String Json := do
  pure (node "update" [("table", toJson update.table), ("alias", toJson update.alias),
    ("assignments", toJson (← update.assignments.mapM assignmentJson)),
    ("where", ← exportOptional (expressionV2 64) update.whereClause),
    ("returning", toJson (← update.returning.mapM (fullItemJson (expressionV2 64))))])

/-- Export every DELETE field without simulating statement effects. -/
def deleteJson (delete : Advanced.Delete) : Except String Json := do
  pure (node "delete" [("table", toJson delete.table), ("alias", toJson delete.alias),
    ("where", ← exportOptional (expressionV2 64) delete.whereClause),
    ("returning", toJson (← delete.returning.mapM (fullItemJson (expressionV2 64))))])

/-- Preserve declaration defaults, nullable spelling and independent key flags. -/
def columnDeclarationJson (column : Advanced.Column) : Except String Json := do
  pure (Json.mkObj [("name", toJson column.name), ("type", ← dataTypeJson column.type),
    ("nullable", toJson column.nullable), ("default", ← exportOptional (expressionV2 64) column.defaultValue),
    ("primary_key", toJson column.primaryKey), ("unique", toJson column.unique)])

/-- Keep table-level key columns ordered and separate from column-level declarations. -/
def tableConstraintJson : TableConstraint → Json
  | .primaryKey columns => node "primary_key" [("columns", toJson columns)]
  | .unique columns => node "unique" [("columns", toJson columns)]

/-- Export CREATE TABLE fields without constructing a database catalog implicitly. -/
def createTableJson (create : Advanced.CreateTable) : Except String Json := do
  pure (node "create_table" [("name", toJson create.name), ("if_not_exists", toJson create.ifNotExists),
    ("columns", toJson (← create.columns.mapM columnDeclarationJson)),
    ("constraints", toJson (create.constraints.map tableConstraintJson))])

/-- Preserve all ALTER operations and flags in their original action order. -/
def alterActionJson : AlterAction → Except String Json
  | .addColumn column ifNotExists => do pure (node "add_column"
      [("column", ← columnDeclarationJson column), ("if_not_exists", toJson ifNotExists)])
  | .dropColumn name ifExists cascade => pure (node "drop_column"
      [("name", toJson name), ("if_exists", toJson ifExists), ("cascade", toJson cascade)])
  | .renameColumn name newName => pure (node "rename_column"
      [("name", toJson name), ("new_name", toJson newName)])
  | .renameTable newName => pure (node "rename_table" [("new_name", toJson newName)])
  | .setType name type => do pure (node "set_type" [("name", toJson name), ("type", ← dataTypeJson type)])
  | .setNotNull name => pure (node "set_not_null" [("name", toJson name)])
  | .dropNotNull name => pure (node "drop_not_null" [("name", toJson name)])
  | .setDefault name value => do pure (node "set_default" [("name", toJson name), ("value", ← expressionV2 64 value)])
  | .dropDefault name => pure (node "drop_default" [("name", toJson name)])

/-- Cover every current upstream Statement constructor without an opaque SQL fallback. -/
def statementV2 : Advanced.Statement → Except String Json
  | .query query => queryV2 64 query
  | .insert insert => insertJson insert
  | .update update => updateJson update
  | .delete delete => deleteJson delete
  | .createTable create => createTableJson create
  | .alterTable name actions => do pure (node "alter_table"
      [("name", toJson name), ("actions", toJson (← actions.mapM alterActionJson))])
  | .dropTable name ifExists cascade => pure (node "drop_table"
      [("name", toJson name), ("if_exists", toJson ifExists), ("cascade", toJson cascade)])

end SQLollaParser
