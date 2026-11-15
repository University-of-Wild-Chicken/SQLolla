import SQLollaM2.Outcome

/-!
Occurrence keys are internal identities, not user primary keys or result order.
Finite support, SQL range checking and expression errors remain separate duties.
The total candidate fragment below is useful only after expression admission.
-/

namespace SQLollaM2

/-- Optional rows distinguish deletion from nullable cell values inside a row. -/
abbrev Store (Key Row : Type) := Key → Option Row

/-- UPDATE selects old rows using SQL TRUE and computes assignments from that old row. -/
def updateRows (predicate : Row → SQLolla.Truth) (assign : Row → Row)
    (before : Store Key Row) (key : Key) : Option Row :=
  match before key with
  | none => none
  | some row => if (predicate row).keepsRow then some (assign row) else some row

/-- Missing occurrences are not invented by UPDATE. -/
theorem update_absent (predicate : Row → SQLolla.Truth) (assign : Row → Row)
    (before : Store Key Row) (key : Key) (absent : before key = none) :
    updateRows predicate assign before key = none := by
  simp [updateRows, absent]

/-- Exact frame: every nonmatching row retains its complete original value. -/
theorem update_frame (predicate : Row → SQLolla.Truth) (assign : Row → Row)
    (before : Store Key Row) (key : Key) (row : Row) (old : before key = some row)
    (outside : (predicate row).keepsRow = false) :
    updateRows predicate assign before key = some row := by
  simp [updateRows, old, outside]

/-- UNKNOWN does not select a row for mutation. -/
theorem update_unknown (assign : Row → Row) (before : Store Key Row) (key : Key)
    (row : Row) (old : before key = some row) :
    updateRows (fun _ => .unknown) assign before key = some row := by
  simp [updateRows, old, SQLolla.Truth.keepsRow]

/-- DELETE removes only selected occurrences, preserving the identities of all others. -/
def deleteRows (predicate : Row → SQLolla.Truth)
    (before : Store Key Row) (key : Key) : Option Row :=
  match before key with
  | none => none
  | some row => if (predicate row).keepsRow then none else some row

/-- Exact DELETE frame includes rows whose predicate is UNKNOWN. -/
theorem delete_frame (predicate : Row → SQLolla.Truth) (before : Store Key Row)
    (key : Key) (row : Row) (old : before key = some row)
    (outside : (predicate row).keepsRow = false) :
    deleteRows predicate before key = some row := by
  simp [deleteRows, old, outside]

/-- Materialize one insert at an externally allocated fresh occurrence identity. -/
def insertAt [DecidableEq Key] (before : Store Key Row) (fresh : Key)
    (row : Row) (key : Key) : Option Row :=
  if key = fresh then some row else before key

/-- INSERT preserves every distinct occurrence; freshness must be separately established. -/
theorem insert_frame [DecidableEq Key] (before : Store Key Row) (fresh key : Key)
    (row : Row) (other : key ≠ fresh) : insertAt before fresh row key = before key := by
  simp [insertAt, other]

/-- A fresh occurrence is inserted without assigning a user key or removing duplicate values. -/
theorem insert_fresh [DecidableEq Key] (before : Store Key Row) (fresh : Key)
    (row : Row) (_unused : before fresh = none) :
    insertAt before fresh row fresh = some row := by
  simp [insertAt]

/-- Relation frames preserve every nontarget table as an exact store value. -/
def replaceTable [DecidableEq Table] (before : Table → Data) (target : Table)
    (candidate : Data) (table : Table) : Data :=
  if table = target then candidate else before table

/-- Single-table effects cannot alter a different table through this constructor. -/
theorem table_frame [DecidableEq Table] (before : Table → Data) (target table : Table)
    (candidate : Data) (other : table ≠ target) :
    replaceTable before target candidate table = before table := by
  simp [replaceTable, other]

/-- Simultaneous assignments read the same old tuple, including cross-column references. -/
def swapColumns (old : Int × Int) : Int × Int := (old.2, old.1)

/-- Sequentially overwriting the first column would fail this required swap behavior. -/
theorem assignment_uses_old_row : swapColumns (1, 2) = (2, 1) := rfl

end SQLollaM2
