import SQLollaM2.TupleVersion

/-! Nominal xid scalar identity is unsigned32; it is not an SQL numeric or ordered type.
Text parsing and binding preservation remain separate validation obligations. -/
namespace SQLollaM2

/-- A stored xid admits all 32-bit identities, including reserved zero. -/
def XidScalarValid (value : Nat) : Prop := value < 4294967296

/-- The xid/int4 equality overload reads the signed integer's unchanged 32-bit pattern. -/
def xidInt32Bits (value : Int) : Nat := (value % 4294967296).toNat

/-- The admitted xid operator domain does not include ordering. -/
inductive XidOperator where
  | equal | notEqual | less | lessEqual | greater | greaterEqual
  deriving DecidableEq

/-- Rejection is distinct from a Boolean false result for an unsupported xid operator. -/
def xidPredicate (op : XidOperator) (left right : Nat) : Option Bool :=
  match op with
  | .equal => some (left == right)
  | .notEqual => some (left != right)
  | _ => none

/-- Identity equality compares raw IDs, not wraparound age or transaction visibility. -/
theorem xid_equal_self (value : Nat) : xidPredicate .equal value value = some true := by
  simp [xidPredicate]

/-- Distinctness remains the Boolean complement of identity equality. -/
theorem xid_not_equal_self (value : Nat) : xidPredicate .notEqual value value = some false := by
  simp [xidPredicate]

/-- A missing ordering operator cannot be replaced by unsigned numeric ordering. -/
theorem xid_less_rejected (left right : Nat) : xidPredicate .less left right = none := rfl

/-- Signed minus one preserves all set bits in the xid/int4 equality overload. -/
theorem xid_negative_one_bits : xidInt32Bits (-1) = 4294967295 := by decide

/-- Zero remains the invalid-header identity while still being a valid scalar. -/
theorem xid_zero_scalar : XidScalarValid 0 ∧ xidInt32Bits 0 = 0 := by
  simp [XidScalarValid, xidInt32Bits]

/-- Every signed32 interpretation produces a scalar within the unsigned32 identity domain. -/
theorem xid_bits_valid (value : Int) : XidScalarValid (xidInt32Bits value) := by
  have lower := Int.emod_nonneg value (by decide : (4294967296 : Int) ≠ 0)
  have upper := Int.emod_lt_of_pos value (by decide : (0 : Int) < 4294967296)
  unfold XidScalarValid xidInt32Bits
  omega

end SQLollaM2
