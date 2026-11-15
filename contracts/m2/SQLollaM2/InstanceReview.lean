import SQLollaM2.Enumeration

/-! Human instance labels constrain proposed revisions without changing solver evidence. -/
namespace SQLollaM2

/-- Swipes label business intent for one instance, rather than asserting universal safety. -/
inductive InstanceDecision where
  | acceptedUseCase
  | rejectedSideBehavior
  deriving DecidableEq, Repr

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

end SQLollaM2
