import SQLollaM2.RelationalEffects

/-! Relation opening precedes executor row demand. The exact analyzed-query footprint
is a separate compiler obligation; this model does not prove PostgreSQL analysis. -/
namespace SQLollaM2.RelationInitialization

/-- Relation modes remain distinct from tuple lock strengths and are not silently deduplicated by strength. -/
inductive Mode where
  | accessShare | rowShare | rowExclusive
  deriving DecidableEq, Repr

/-- A resolved physical identity, not an SQL alias, names each required initialization lock. -/
structure Request (Relation : Type) where
  relation : Relation
  mode : Mode
  deriving DecidableEq, Repr

/-- Ownership records relation locks independently of tuple ownership and output rows. -/
structure Ownership (Relation Row : Type) where
  relations : Request Relation → Prop
  rows : Row → Prop

/-- Initialization acquires the supplied analyzed-query footprint without consuming any row. -/
def acquireFootprint (before : Ownership Relation Row) (footprint : List (Request Relation)) :
    Ownership Relation Row :=
  ⟨fun request => before.relations request ∨ request ∈ footprint, before.rows⟩

/-- A resolved required relation lock remains owned even if the consumer requests no tuples. -/
theorem required_relation_retained (before : Ownership Relation Row)
    (footprint : List (Request Relation)) (request : Request Relation)
    (required : request ∈ footprint) : (acquireFootprint before footprint).relations request :=
  Or.inr required

/-- Relation initialization cannot fabricate row ownership. -/
theorem no_tuple_lock_from_initialization (before : Ownership Relation Row)
    (footprint : List (Request Relation)) : (acquireFootprint before footprint).rows = before.rows := rfl

/-- Existing relation ownership is retained without assuming it originated in this statement. -/
theorem prior_relation_retained (before : Ownership Relation Row)
    (footprint : List (Request Relation)) (request : Request Relation)
    (held : before.relations request) : (acquireFootprint before footprint).relations request := Or.inl held

/-- Initialization admits exactly prior ownership or explicitly listed relation requests. -/
theorem initialization_exact (before : Ownership Relation Row)
    (footprint : List (Request Relation)) (request : Request Relation) :
    (acquireFootprint before footprint).relations request ↔ before.relations request ∨ request ∈ footprint := Iff.rfl

/-- Repeated initialization does not invent another relation-ownership fact. -/
theorem initialization_idempotent (before : Ownership Relation Row)
    (footprint : List (Request Relation)) (request : Request Relation) :
    (acquireFootprint (acquireFootprint before footprint) footprint).relations request ↔
      (acquireFootprint before footprint).relations request := by
  simp only [acquireFootprint, or_assoc, or_self]

/-- Consumer tuple demand changes neither the initialization footprint nor its acquired ownership. -/
def initializedConsumer (before : Ownership Relation Row) (footprint : List (Request Relation))
    (_demandedRows : List Row) : Ownership Relation Row := acquireFootprint before footprint

/-- Empty and nonempty consumers share relation ownership, while tuple effects require separate execution. -/
theorem demand_does_not_erase_relations (before : Ownership Relation Row)
    (footprint : List (Request Relation)) (rows : List Row) :
    (initializedConsumer before footprint rows).relations =
      (initializedConsumer before footprint []).relations := rfl

/-- An exact analyzed-query footprint is an explicit external compiler/refinement obligation. -/
def ExactFootprint (opened : Request Relation → Prop) (footprint : List (Request Relation)) : Prop :=
  ∀ request, opened request ↔ request ∈ footprint

/-- Only an exact footprint permits transferring analyzed-query relation openings to ownership. -/
theorem exact_footprint_transfer (before : Ownership Relation Row)
    (opened : Request Relation → Prop) (footprint : List (Request Relation))
    (certificate : ExactFootprint opened footprint) (request : Request Relation) :
    (acquireFootprint before footprint).relations request ↔ before.relations request ∨ opened request := by
  rw [initialization_exact, ← certificate request]

/-- A WITH definition has its own locking scope; a derived FROM subquery may inherit parent locking. -/
inductive Boundary where
  | direct | derived | withDefinition
  deriving DecidableEq, Repr

/-- This adapter assumes alias/OF targeting has already resolved the Boolean locking inputs. -/
def rowShareRequested (directlyLocked inherited : Bool) : Boundary → Bool
  | .withDefinition => directlyLocked
  | .direct | .derived => directlyLocked || inherited

/-- FOR UPDATE outside an unlocked WITH definition cannot turn it into a locked base relation. -/
theorem with_does_not_inherit_parent : rowShareRequested false true .withDefinition = false := rfl

/-- An explicit locking clause inside WITH remains effective independently of consumer demand. -/
theorem with_local_lock_retained (inherited : Bool) :
    rowShareRequested true inherited .withDefinition = true := rfl

/-- A resolved derived-subquery inheritance request retains its relation-lock mode. -/
theorem derived_inherits_parent (directlyLocked : Bool) : rowShareRequested directlyLocked true .derived = true := by
  cases directlyLocked <;> rfl

/-- A statement-boundary rollback restores prior ownership, separate from nontransactional journals. -/
def rollback (before : Ownership Relation Row) (_attempt : Ownership Relation Row) := before

/-- Failed initialization/execution does not publish newly acquired relation ownership after rollback. -/
theorem rollback_relation_frame (before attempt : Ownership Relation Row) :
    (rollback before attempt).relations = before.relations := rfl

end SQLollaM2.RelationInitialization
