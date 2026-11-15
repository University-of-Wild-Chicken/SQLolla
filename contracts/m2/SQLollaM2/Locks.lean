import SQLollaM2.Outcome

/-! Fixed lock snapshots model immediate acquisition/skip/error, not concurrent schedules. -/

namespace SQLollaM2

/-- PostgreSQL row-lock strengths are distinct from relation lock modes. -/
inductive RowLockMode where
  | keyShare | share | noKeyUpdate | update
  deriving DecidableEq, Repr

/-- PostgreSQL row conflict matrix; ownership is checked independently. -/
def rowConflicts : RowLockMode → RowLockMode → Bool
  | .update, _ | _, .update => true
  | .noKeyUpdate, .share | .share, .noKeyUpdate => true
  | .noKeyUpdate, .noKeyUpdate => true
  | _, _ => false

/-- Conflict is symmetric for every pair of row modes. -/
theorem row_conflict_symmetric (left right : RowLockMode) :
    rowConflicts left right = rowConflicts right left := by
  cases left <;> cases right <;> rfl

/-- KEY SHARE remains compatible with NO KEY UPDATE. -/
theorem key_share_compatible : rowConflicts .keyShare .noKeyUpdate = false := rfl

/-- A transaction's own row locks never conflict with its later lock acquisition. -/
def ownedConflict (current owner : Nat) (wanted held : RowLockMode) : Bool :=
  current != owner && rowConflicts wanted held

/-- Ownership removes self-conflict even for the strongest row-lock mode. -/
theorem lock_owner_reentrant (owner : Nat) (wanted held : RowLockMode) :
    ownedConflict owner owner wanted held = false := by simp [ownedConflict]

/-- NOWAIT and SKIP LOCKED affect row conflicts without inventing a completed wait. -/
inductive LockWait where
  | block | nowait | skipLocked
  deriving DecidableEq, Repr

/-- An unresolved wait is inconclusive, distinct from SQL error or successful skipping. -/
inductive LockDecision where
  | acquired | skipped | unavailable | unresolvedWait
  deriving DecidableEq, Repr

/-- Resolve only the immediate behavior of the declared fixed lock snapshot. -/
def decideLock (conflict : Bool) (policy : LockWait) : LockDecision :=
  if !conflict then .acquired else match policy with
    | .block => .unresolvedWait
    | .nowait => .unavailable
    | .skipLocked => .skipped

/-- A blocked ordinary request cannot become successful under a fixed snapshot. -/
theorem blocked_not_acquired : decideLock true .block = .unresolvedWait := rfl

/-- SKIP LOCKED and NOWAIT preserve their distinct outcomes. -/
theorem conflict_policies :
    decideLock true .skipLocked = .skipped ∧ decideLock true .nowait = .unavailable := ⟨rfl, rfl⟩

/-- LIMIT zero requests no row locks, including rows otherwise skipped by OFFSET. -/
def lockPrefix (rows : List α) (offset : Nat) (limit : Option Nat) : List α :=
  match limit with
  | some 0 => []
  | some count => rows.take (offset + count)
  | none => rows

/-- OFFSET-consumed rows remain part of the locked prefix. -/
theorem offset_rows_locked : lockPrefix [1, 2, 3] 1 (some 1) = [1, 2] := rfl

/-- Zero result capacity must not lock OFFSET rows by eager evaluation. -/
theorem limit_zero_no_rows (rows : List α) (offset : Nat) :
    lockPrefix rows offset (some 0) = [] := rfl

/-- Successful output begins after OFFSET within the acquired-row stream. -/
def lockedOutput (rows : List α) (offset : Nat) (limit : Option Nat) : List α :=
  (lockPrefix rows offset limit).drop offset

/-- The example's discarded OFFSET row is locked but is not returned. -/
theorem offset_row_not_output : lockedOutput [1, 2, 3] 1 (some 1) = [2] := rfl

/-- Joined occurrences retain both physical origins independently of visible values. -/
def joinedOrigins (pairs : List (Nat × Nat)) : List Nat × List Nat :=
  (pairs.map Prod.fst, pairs.map Prod.snd)

/-- One-to-many join matches retain the repeated left ownership identity. -/
theorem joined_repeated_origin : joinedOrigins [(0, 2), (0, 3)] = ([0, 0], [2, 3]) := rfl

/-- Join provenance preserves one identity per input pair on each side. -/
theorem joined_origin_lengths (pairs : List (Nat × Nat)) :
    (joinedOrigins pairs).1.length = pairs.length ∧
    (joinedOrigins pairs).2.length = pairs.length := by
  simp [joinedOrigins]

/-- OFFSET without LIMIT may remove a different occurrence under each unordered permutation. -/
theorem unordered_offset_alternatives :
    lockedOutput [1, 2, 3] 1 none = [2, 3] ∧
    lockedOutput [2, 1, 3] 1 none = [1, 3] := ⟨rfl, rfl⟩

end SQLollaM2
