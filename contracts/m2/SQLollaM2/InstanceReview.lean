import SQLollaM2.Enumeration
import SQLollaM2.Outcome

/-! Human instance labels constrain proposed revisions without changing solver evidence. -/
namespace SQLollaM2

/-- Swipes label business intent for one instance, rather than asserting universal safety. -/
inductive InstanceDecision where
  | acceptedUseCase
  | rejectedSideBehavior
  | unsure
  deriving DecidableEq, Repr

/-- Only a Yes or No decision imposes a replay obligation and enables regeneration. -/
def InstanceDecision.decisive : InstanceDecision → Bool
  | .unsure => false
  | _ => true

/-- A review refers to an immutable instance and the model revision that generated it. -/
structure InstanceReview (Revision Instance : Type) where
  revision : Revision
  observation : Instance
  decision : InstanceDecision

/-- Applying a review to another model revision requires separate revalidation. -/
def ReviewCurrent (revision : Revision) (review : InstanceReview Revision Instance) : Prop :=
  review.revision = revision

/-- A current review cannot silently refer to a different archived model revision. -/
theorem current_review_revision (revision : Revision) (review : InstanceReview Revision Instance)
    (current : ReviewCurrent revision review) : review.revision = revision := current

/-- Proposed behavior must retain accepted observations and exclude rejected observations. -/
def FitsInstanceFeedback (possible : Instance → Prop) (accepted rejected : List Instance) : Prop :=
  (∀ value ∈ accepted, possible value) ∧ (∀ value ∈ rejected, ¬ possible value)

/-- A revision satisfying feedback cannot remove a labeled accepted observation. -/
theorem accepted_observation_retained (possible : Instance → Prop) (accepted rejected : List Instance)
    (fits : FitsInstanceFeedback possible accepted rejected) (value : Instance)
    (selected : value ∈ accepted) : possible value := fits.1 value selected

/-- A revision satisfying feedback cannot continue to permit the same rejected observation. -/
theorem rejected_observation_excluded (possible : Instance → Prop) (accepted rejected : List Instance)
    (fits : FitsInstanceFeedback possible accepted rejected) (value : Instance)
    (selected : value ∈ rejected) : ¬ possible value := fits.2 value selected

/-- Contradictory labels require resolution instead of silently choosing one intent. -/
theorem conflicting_feedback (possible : Instance → Prop) (accepted rejected : List Instance)
    (fits : FitsInstanceFeedback possible accepted rejected) (value : Instance)
    (yes : value ∈ accepted) (no : value ∈ rejected) : False :=
  fits.2 value no (fits.1 value yes)

/-- Accepting one admissible instance does not eliminate other unintended admissible instances. -/
theorem one_acceptance_not_universal_safety :
    FitsInstanceFeedback (fun _ : Bool => True) [true] [] ∧
      ¬ (∀ value : Bool, True → value = true) := by
  constructor
  · simp [FitsInstanceFeedback]
  · intro claimed
    have impossible := claimed false True.intro
    cases impossible

/-- Exact observations keep duplicate output occurrences, poststate, SQL errors and retained effects. -/
structure ReviewBehavior (Row State Effects : Type) where
  rows : List Row
  state : State
  failure : Option Failure
  effects : Effects
  deriving DecidableEq

/-- Input includes the immutable world, typed bindings and invocation context used for replay. -/
structure ReviewObservation (Input Behavior : Type) where
  input : Input
  behavior : Behavior
  deriving DecidableEq

/-- A decision targets an immutable saved instance, including its bundle provenance. -/
structure ReviewKey where
  bundle : String
  instanceId : String
  deriving DecidableEq

/-- A new entry supersedes the prior decision for this key; none withdraws it. -/
structure ReviewEdit where
  key : ReviewKey
  decision : Option InstanceDecision
  deriving DecidableEq

/-- The snapshot identifies both the current source revision and its committed history. -/
structure ReviewStamp where
  revision : String
  digest : String
  deriving DecidableEq

/-- Edits are recorded in commit order, with the oldest edit first. -/
structure ReviewSnapshot where
  stamp : ReviewStamp
  history : List ReviewEdit

/-- The active projection changes just the edited record; the ledger keeps its earlier entries. -/
def applyReviewEdit (active : List (ReviewKey × InstanceDecision)) (edit : ReviewEdit) :
    List (ReviewKey × InstanceDecision) :=
  let others := active.filter fun entry => decide (entry.1 ≠ edit.key)
  match edit.decision with
  | some decision => others ++ [(edit.key, decision)]
  | none => others

/-- Only the last committed decision for each saved instance feeds candidate checks. -/
def activeReviews (history : List ReviewEdit) : List (ReviewKey × InstanceDecision) :=
  history.foldl applyReviewEdit []

/-- An appended withdrawal removes the key from the active projection. -/
theorem appended_withdrawal (history : List ReviewEdit) (key : ReviewKey) (decision : InstanceDecision) :
    (key, decision) ∉ activeReviews (history ++ [⟨key, none⟩]) := by
  simp [activeReviews, applyReviewEdit]

/-- An appended replacement makes exactly the new decision active for its key. -/
theorem appended_replacement (history : List ReviewEdit) (key : ReviewKey)
    (old new : InstanceDecision) :
    (key, old) ∈ activeReviews (history ++ [⟨key, some new⟩]) ↔ old = new := by
  simp [activeReviews, applyReviewEdit]

/-- Regeneration requires at least one Yes or No among the current decisions. -/
def canRegenerate (history : List ReviewEdit) : Bool :=
  (activeReviews history).any fun entry => entry.2.decisive

/-- Withdrawing the last decisive label or moving it to Unsure disables regeneration. -/
theorem reconsideration_blocks_regeneration (history : List ReviewEdit) (key : ReviewKey)
    (decision : Option InstanceDecision) (undecided : decision = none ∨ decision = some .unsure)
    (others : ∀ entry ∈ activeReviews history, entry.1 ≠ key → entry.2 = .unsure) :
    canRegenerate (history ++ [⟨key, decision⟩]) = false := by
  have absent : ¬ ∃ entry ∈ activeReviews history, entry.1 ≠ key ∧ entry.2.decisive = true := by
    rintro ⟨entry, member, different, decisive⟩
    simp [others entry member different, InstanceDecision.decisive] at decisive
  rcases undecided with rfl | rfl <;>
    simpa [canRegenerate, activeReviews, applyReviewEdit, List.any_eq_true, InstanceDecision.decisive,
      and_assoc] using absent

/-- Saved artifact revision is provenance; exact semantic basis determines compatibility. -/
structure SavedObservation (Basis Input Behavior : Type) where
  artifactRevision : String
  basis : Basis
  observation : ReviewObservation Input Behavior

/-- Compare source semantics across bundles independently of example or counterexample provenance. -/
def ReviewCompatible (basis : Basis) (saved : SavedObservation Basis Input Behavior) : Prop :=
  saved.basis = basis

/-- Resolve every effective label against immutable evidence and an unchanged semantic basis. -/
def resolveReviews [DecidableEq Basis] (basis : Basis)
    (lookup : ReviewKey → Option (SavedObservation Basis Input Behavior)) (snapshot : ReviewSnapshot) :
    Option (List (ReviewObservation Input Behavior × InstanceDecision)) :=
  (activeReviews snapshot.history).mapM fun (key, decision) => do
    let saved ← lookup key
    if saved.basis = basis then some (saved.observation, decision) else none

/-- Different artifact revisions with the same basis are compatible with the same review. -/
theorem provenance_independent (basis : Basis) (saved : SavedObservation Basis Input Behavior)
    (revision : String) :
    ReviewCompatible basis { saved with artifactRevision := revision } ↔ ReviewCompatible basis saved := Iff.rfl

end SQLollaM2
