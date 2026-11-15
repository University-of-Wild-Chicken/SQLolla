import SQLollaM2.Outcome

/-! A restricted heap-header transition contract, not a PostgreSQL MVCC implementation.
Admission requires an exclusive top-level writer, explicit tuple headers and no MultiXact.
The actual write transaction identity is supplied before executing the query. -/
namespace SQLollaM2.TupleVersion

/-- Normal 32-bit transaction identities exclude the reserved invalid/bootstrap/frozen values. -/
def NormalXid (xid : Nat) : Prop := 3 ≤ xid ∧ xid < 4294967296

/-- Only states covered by the single-writer profile appear in this transition domain. -/
inductive XmaxState where
  | invalid | ownLock | abortedUpdater
  deriving DecidableEq, Repr

/-- Raw header identity is separate from transaction/lock interpretation. -/
structure Header where
  xmin : Nat
  rawXmax : Nat
  state : XmaxState
  deriving DecidableEq, Repr

/-- Header admission retains raw values even when the transaction encoded by xmax aborted. -/
def HeaderValid (writer : Nat) (header : Header) : Prop :=
  header.xmin < 4294967296 ∧ header.rawXmax < 4294967296 ∧
  (header.state = .ownLock → header.rawXmax = writer)

/-- Fresh ordinary heap insertion initializes xmax to invalid zero. -/
def freshInsert (writer : Nat) : Header := ⟨writer, 0, .invalid⟩

/-- Conflict handling locks the old version before checking the update predicate. -/
def lockForConflict (writer : Nat) (before : Header) : Header :=
  { before with rawXmax := writer, state := .ownLock }

/-- Update preserves a surviving local lock; otherwise the new version has invalid xmax. -/
def updatedVersion (writer : Nat) (before : Header) : Header :=
  if before.state = .ownLock then ⟨writer, before.rawXmax, .ownLock⟩
  else freshInsert writer

/-- ON CONFLICT UPDATE composes the lock and ordinary version transition. -/
def conflictUpdate (writer : Nat) (before : Header) : Header :=
  updatedVersion writer (lockForConflict writer before)

/-- Aborting a write restores the old version's data visibility, not its raw xmax bytes. -/
def abortedOriginal (writer : Nat) (before : Header) : Header :=
  { before with rawXmax := writer, state := .abortedUpdater }

/-- Fresh insertion does not encode an update by giving the tuple a nonzero xmax. -/
theorem fresh_xmax_zero (writer : Nat) : (freshInsert writer).rawXmax = 0 := rfl

/-- The new tuple's inserting transaction is the supplied current writer. -/
theorem fresh_xmin_writer (writer : Nat) : (freshInsert writer).xmin = writer := rfl

/-- A preexisting local lock, rather than an SQL command label, supplies the new xmax. -/
theorem local_lock_survives (writer : Nat) (before : Header) (locked : before.state = .ownLock) :
    (updatedVersion writer before).rawXmax = before.rawXmax := by
  simp [updatedVersion, locked]

/-- An invalid or aborted predecessor does not retain its xmax in an ordinary new version. -/
theorem unlocked_update_clears (writer : Nat) (before : Header) (unlocked : before.state ≠ .ownLock) :
    (updatedVersion writer before).rawXmax = 0 := by simp [updatedVersion, unlocked, freshInsert]

/-- The same lock-first transition explains the observed conflict-update xmax. -/
theorem conflict_xmax_writer (writer : Nat) (before : Header) :
    (conflictUpdate writer before).rawXmax = writer := by
  simp [conflictUpdate, lockForConflict, updatedVersion]

/-- A normal writer makes this particular conflict-updated version's xmax nonzero. -/
theorem conflict_xmax_nonzero (writer : Nat) (before : Header) (normal : NormalXid writer) :
    (conflictUpdate writer before).rawXmax ≠ 0 := by
  rw [conflict_xmax_writer]
  have lower := normal.1
  omega

/-- WHERE FALSE does not undo the earlier conflict lock or create a new xmin. -/
theorem skipped_update_retains_old_xmin (writer : Nat) (before : Header) :
    (lockForConflict writer before).xmin = before.xmin := rfl

/-- Aborted raw xmax is still observable as a nonzero transaction identity. -/
theorem aborted_raw_xmax_retained (writer : Nat) (before : Header) :
    (abortedOriginal writer before).rawXmax = writer := rfl

/-- Rollback cannot identify the restored version with a freshly inserted version. -/
theorem abort_preserves_original_xmin (writer : Nat) (before : Header) :
    (abortedOriginal writer before).xmin = before.xmin := rfl

/-- Normal transaction bounds ensure a fresh header stays in the 32-bit domain. -/
theorem fresh_header_valid (writer : Nat) (normal : NormalXid writer) :
    HeaderValid writer (freshInsert writer) := by
  simp [HeaderValid, freshInsert, normal.2]

/-- A lock operation preserves well-formedness under the explicitly supplied writer identity. -/
theorem locked_header_valid (writer : Nat) (before : Header) (normal : NormalXid writer)
    (valid : HeaderValid writer before) : HeaderValid writer (lockForConflict writer before) := by
  simp [HeaderValid, lockForConflict, valid.1, normal.2]

/-- Logical row contents and physical header observations are distinct state components. -/
structure VisibleVersion (Row : Type) where
  row : Row
  header : Header

/-- A failed update restores data while retaining the aborted updater's physical header identity. -/
def rollbackUpdate (writer : Nat) (before : VisibleVersion Row) : VisibleVersion Row :=
  ⟨before.row, abortedOriginal writer before.header⟩

/-- Existing table-data rollback invariants remain true despite changed header observations. -/
theorem rollback_data_frame (writer : Nat) (before : VisibleVersion Row) :
    (rollbackUpdate writer before).row = before.row := rfl

end SQLollaM2.TupleVersion
