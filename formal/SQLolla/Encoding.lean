import SQLolla.Analysis

/-!
# Conditional encoding obligations

This module proves what a correct encoding would imply. It does not construct
an Alloy encoding or prove PostgreSQL equivalence. The assumptions deliberately
separate witness soundness from completeness needed to trust an UNSAT result.
-/

namespace SQLolla

/-- Bad behaviors in the semantic model and models of the SAT failure formula. -/
structure FailureEncoding (Behavior Model : Type) where
  semanticFailure : Behavior → Prop
  encodedFailure : Model → Prop
  decodes : Model → Behavior → Prop

/-- Every reported model must correspond to a real modeled failure. -/
def WitnessSound (encoding : FailureEncoding Behavior Model) : Prop :=
  ∀ model, encoding.encodedFailure model →
    ∃ behavior, encoding.decodes model behavior ∧ encoding.semanticFailure behavior

/-- Every modeled failure must have an encoding; necessary for UNSAT assurance. -/
def FailureComplete (encoding : FailureEncoding Behavior Model) : Prop :=
  ∀ behavior, encoding.semanticFailure behavior →
    ∃ model, encoding.encodedFailure model ∧ encoding.decodes model behavior

/-- A sound SAT witness establishes a semantic failure exists. -/
theorem sat_transfers (encoding : FailureEncoding Behavior Model)
    (sound : WitnessSound encoding) (sat : ∃ model, encoding.encodedFailure model) :
    ∃ behavior, encoding.semanticFailure behavior := by
  obtain ⟨model, witness⟩ := sat
  obtain ⟨behavior, _, bad⟩ := sound model witness
  exact ⟨behavior, bad⟩

/-- UNSAT transfers only with completeness of the admitted failure encoding. -/
theorem unsat_transfers (encoding : FailureEncoding Behavior Model)
    (complete : FailureComplete encoding)
    (unsat : ¬ ∃ model, encoding.encodedFailure model) :
    ¬ ∃ behavior, encoding.semanticFailure behavior := by
  intro failure
  obtain ⟨behavior, bad⟩ := failure
  obtain ⟨model, encoded, _⟩ := complete behavior bad
  exact unsat ⟨model, encoded⟩

/-- An empty encoding is witness-sound vacuously, even if real failures exist. -/
def emptyEncoding : FailureEncoding Unit Unit :=
  ⟨fun _ => True, fun _ => False, fun _ _ => True⟩

/-- Witness replay alone cannot establish completeness. -/
theorem empty_encoding_sound : WitnessSound emptyEncoding := by
  intro _ impossible
  exact False.elim impossible

/-- This sound-but-empty encoding omits a genuine semantic failure. -/
theorem empty_encoding_not_complete : ¬ FailureComplete emptyEncoding := by
  intro complete
  obtain ⟨_, impossible, _⟩ := complete () True.intro
  exact impossible

/-- Its formula is UNSAT despite the existence of a semantic failure. -/
theorem empty_encoding_false_assurance :
    (¬ ∃ model, emptyEncoding.encodedFailure model) ∧
    (∃ behavior, emptyEncoding.semanticFailure behavior) := by
  constructor
  · rintro ⟨_, impossible⟩
    exact impossible
  · exact ⟨(), True.intro⟩

end SQLolla
