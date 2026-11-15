import SQLollaM2.Outcome

/-! Catalog changes are explicit state transitions. These abstract contracts do not
prove PostgreSQL parsing, name resolution, stored-query preservation or Rust refinement. -/

namespace SQLollaM2

/-- Missing creation evidence differs from an observed empty explicit PostgreSQL search path. -/
inductive CreationEvidence where
  | missing
  | observed (schema : Option String)
  deriving DecidableEq, Repr

/-- An explicit target overrides the observed creation schema; effective lookup names never choose it. -/
def selectedCreationSchema (explicit : Option String) (observed : CreationEvidence) :
    CreationEvidence :=
  match explicit with
  | some name => .observed (some name)
  | none => observed

/-- Implicit pg_catalog in lookup does not change an explicitly observed public creation schema. -/
theorem observed_creation_preserved (schema : String) :
    selectedCreationSchema none (.observed (some schema)) = .observed (some schema) := rfl

/-- A known empty creation path remains distinct from missing context. -/
theorem no_creation_schema_is_known :
    selectedCreationSchema none (.observed none) ≠ .missing := by decide

/-- A qualified CREATE target remains resolvable even when the current creation schema is absent. -/
theorem explicit_creation_overrides (schema : String) (evidence : CreationEvidence) :
    selectedCreationSchema (some schema) evidence = .observed (some schema) := rfl

/-- A changed namespace invalidates its old complete OID lookup instead of predicting allocation. -/
def lookupAfterDdl (changed : Bool) (lookup : Option Evidence) : Option Evidence :=
  if changed then none else lookup

/-- Successful object creation or removal cannot publish the previous complete identity lookup. -/
theorem changed_ddl_invalidates_lookup (lookup : Option Evidence) :
    lookupAfterDdl true lookup = none := rfl

/-- IF EXISTS/IF NOT EXISTS no-ops preserve all input lookup evidence. -/
theorem skipped_ddl_preserves_lookup (lookup : Option Evidence) :
    lookupAfterDdl false lookup = lookup := rfl

/-- Fresh key metadata records PostgreSQL's primary-key-first index creation profile. -/
def createdKeyOrder (primary : Option Key) (unique : List Key) : List Key :=
  primary.toList ++ unique

/-- A primary key retains priority over every later unique index without reordering unique keys. -/
theorem primary_key_first (primary : Key) (unique : List Key) :
    createdKeyOrder (some primary) unique = primary :: unique := rfl

/-- A catalog identity retains schema and relation spelling independently. -/
structure DdlName where
  schema : String
  relation : String
  deriving DecidableEq, Repr

/-- A view is a stored query definition, not a materialized table bag. -/
inductive DdlKind where
  | table | view
  deriving DecidableEq, Repr

/-- Column order, constraint declarations and original view SQL remain catalog data. -/
structure DdlObject where
  kind : DdlKind
  columns : List String
  constraints : List String
  sourceSql : Option String
  dependencies : List DdlName
  deriving DecidableEq, Repr

/-- The finite Rust map refines this abstract lookup interface only under a separate obligation. -/
abbrev DdlCatalog := DdlName → Option DdlObject

/-- DDL failures preserve distinct PostgreSQL diagnostic families. -/
inductive DdlError where
  | duplicateRelation | undefinedRelation | wrongObjectType | dependentObjects
  deriving DecidableEq, Repr

/-- Stable SQLSTATE strings permit independent locale-free oracle comparison. -/
def DdlError.sqlstate : DdlError → String
  | .duplicateRelation => "42P07"
  | .undefinedRelation => "42P01"
  | .wrongObjectType => "42809"
  | .dependentObjects => "2BP01"

/-- Replace exactly one catalog identity while preserving every unrelated definition. -/
def ddlPut (catalog : DdlCatalog) (name : DdlName) (value : Option DdlObject) : DdlCatalog :=
  fun candidate => if candidate = name then value else catalog candidate

/-- A catalog write is visible at its exact qualified identity. -/
theorem ddl_put_target (catalog : DdlCatalog) (name : DdlName) (value : Option DdlObject) :
    ddlPut catalog name value name = value := by simp [ddlPut]

/-- Catalog changes frame every other qualified relation. -/
theorem ddl_put_frame (catalog : DdlCatalog) (name other : DdlName)
    (value : Option DdlObject) (different : other ≠ name) :
    ddlPut catalog name value other = catalog other := by simp [ddlPut, different]

/-- IF NOT EXISTS skips an occupied identity; it does not assert definition equality. -/
def ddlCreate (catalog : DdlCatalog) (name : DdlName) (value : DdlObject)
    (ifNotExists : Bool) : Except DdlError (DdlCatalog × Bool) :=
  match catalog name with
  | none => .ok (ddlPut catalog name (some value), true)
  | some _ => if ifNotExists then .ok (catalog, false) else .error .duplicateRelation

/-- Creating a missing relation records its complete supplied definition. -/
theorem ddl_create_missing (catalog : DdlCatalog) (name : DdlName) (value : DdlObject)
    (skip : Bool) (missing : catalog name = none) :
    ddlCreate catalog name value skip = .ok (ddlPut catalog name (some value), true) := by
  simp [ddlCreate, missing]

/-- A skipped CREATE cannot replace columns or constraints with the attempted declaration. -/
theorem ddl_create_skip_preserves (catalog : DdlCatalog) (name : DdlName)
    (old proposed : DdlObject) (present : catalog name = some old) :
    ddlCreate catalog name proposed true = .ok (catalog, false) := by
  simp [ddlCreate, present]

/-- An occupied identity without the explicit skip flag yields duplicate_relation. -/
theorem ddl_create_duplicate (catalog : DdlCatalog) (name : DdlName)
    (old proposed : DdlObject) (present : catalog name = some old) :
    ddlCreate catalog name proposed false = .error .duplicateRelation := by
  simp [ddlCreate, present]

/-- DROP VIEW checks object kind and dependency evidence before removing a definition. -/
def ddlDropView (catalog : DdlCatalog) (name : DdlName)
    (ifExists hasDependents : Bool) : Except DdlError DdlCatalog :=
  match catalog name with
  | none => if ifExists then .ok catalog else .error .undefinedRelation
  | some value =>
    if value.kind ≠ .view then .error .wrongObjectType
    else if hasDependents then .error .dependentObjects
    else .ok (ddlPut catalog name none)

/-- Missing IF EXISTS targets preserve the whole catalog. -/
theorem ddl_drop_absent_skip (catalog : DdlCatalog) (name : DdlName)
    (dependencies : Bool) (missing : catalog name = none) :
    ddlDropView catalog name true dependencies = .ok catalog := by
  simp [ddlDropView, missing]

/-- IF EXISTS does not authorize dropping a table through DROP VIEW. -/
theorem ddl_drop_wrong_kind (catalog : DdlCatalog) (name : DdlName) (value : DdlObject)
    (present : catalog name = some value) (table : value.kind = .table) (skip deps : Bool) :
    ddlDropView catalog name skip deps = .error .wrongObjectType := by
  simp [ddlDropView, present, table]

/-- RESTRICT dependency failures cannot be hidden by removing the catalog object first. -/
theorem ddl_drop_dependency (catalog : DdlCatalog) (name : DdlName) (value : DdlObject)
    (present : catalog name = some value) (view : value.kind = .view) (skip : Bool) :
    ddlDropView catalog name skip true = .error .dependentObjects := by
  simp [ddlDropView, present, view]

/-- Successful DROP VIEW removes its exact identity while leaving table data outside this operation. -/
theorem ddl_drop_view (catalog : DdlCatalog) (name : DdlName) (value : DdlObject)
    (present : catalog name = some value) (view : value.kind = .view) (skip : Bool) :
    ddlDropView catalog name skip false = .ok (ddlPut catalog name none) := by
  simp [ddlDropView, present, view]

/-- Physical table rows are separate from view definitions and catalog object identity. -/
abbrev DdlRows (Row : Type) := DdlName → List Row

/-- New tables start empty; an explicitly skipped creation preserves existing occurrences. -/
def ddlCreateRows (rows : DdlRows Row) (name : DdlName) (created : Bool) : DdlRows Row :=
  if created then fun candidate => if candidate = name then [] else rows candidate else rows

/-- Creating a table does not borrow data from another table or view. -/
theorem ddl_new_table_empty (rows : DdlRows Row) (name : DdlName) :
    ddlCreateRows rows name true name = [] := by simp [ddlCreateRows]

/-- IF NOT EXISTS preserves every existing row occurrence, including duplicates. -/
theorem ddl_skipped_table_rows (rows : DdlRows Row) (name : DdlName) :
    ddlCreateRows rows name false = rows := rfl

/-- Table creation frames data stored at every other qualified identity. -/
theorem ddl_create_rows_frame (rows : DdlRows Row) (name other : DdlName)
    (different : other ≠ name) :
    ddlCreateRows rows name true other = rows other := by simp [ddlCreateRows, different]

/-- Prepared view syntax must be regenerated from its exact original SQL under a supplied parser. -/
def PreparedView (parse : String → Except String Syntax) (source : String) (ast : Syntax) : Prop :=
  parse source = .ok ast

/-- Preparing one source text gives no permission to reuse its syntax for a different parser result. -/
theorem ddl_changed_view_requires_preparation (parse : String → Except String Syntax)
    (source : String) (ast : Syntax) (different : parse source ≠ .ok ast) :
    ¬ PreparedView parse source ast := different

end SQLollaM2
