import SQLolla.Encoding

/-!
# Instance presentation boundary

Users can inspect satisfying examples and counterexamples. A view may select
facts from a decoded instance but may not invent solver evidence. AI prose and
reference validity are distinct: valid references do not prove prose truthful.
Rendering, authentication, hashes and provider behavior remain implementation work.
-/

namespace SQLolla

/-- A satisfying example is not itself a claim that a safety property holds. -/
inductive InstanceKind where
  | satisfyingExample
  | counterexample
  deriving DecidableEq, Repr

/-- Semantic obligations required before a solver instance becomes trusted evidence. -/
structure InstanceContract (Model Behavior Fact : Type) where
  modelHolds : Model → Prop
  decodes : Model → Behavior → Prop
  facts : Behavior → List Fact
  violates : Behavior → Prop

/-- A decoded instance retains its kind and justification for a counterexample label. -/
structure ValidatedInstance (spec : InstanceContract Model Behavior Fact) where
  model : Model
  behavior : Behavior
  kind : InstanceKind
  modelValid : spec.modelHolds model
  decodingValid : spec.decodes model behavior
  counterexampleValid : kind = .counterexample → spec.violates behavior

/-- Views display selected evidence; selection does not modify the underlying instance. -/
structure InstanceView (spec : InstanceContract Model Behavior Fact)
    (witness : ValidatedInstance spec) where
  visibleFacts : List Fact
  authentic : ∀ fact, fact ∈ visibleFacts → fact ∈ spec.facts witness.behavior

/-- Every fact displayed by a well-formed view comes from its decoded instance. -/
theorem view_fact_authentic (spec : InstanceContract Model Behavior Fact)
    (witness : ValidatedInstance spec) (view : InstanceView spec witness)
    (fact : Fact) (displayed : fact ∈ view.visibleFacts) :
    fact ∈ spec.facts witness.behavior := view.authentic fact displayed

/-- Completeness is a separate obligation; an authentic subset may hide evidence. -/
def ViewComplete (spec : InstanceContract Model Behavior Fact)
    (witness : ValidatedInstance spec) (view : InstanceView spec witness) : Prop :=
  ∀ fact, fact ∈ spec.facts witness.behavior → fact ∈ view.visibleFacts

/-- Absence in a view transfers to the instance only when that view is complete. -/
theorem complete_view_absence (spec : InstanceContract Model Behavior Fact)
    (witness : ValidatedInstance spec) (view : InstanceView spec witness)
    (complete : ViewComplete spec witness view) (fact : Fact)
    (notShown : fact ∉ view.visibleFacts) : fact ∉ spec.facts witness.behavior := by
  intro present
  exact notShown (complete fact present)

/-- A truncated view can omit a fact that exists in the retained evidence. -/
theorem truncated_view_absence_example :
    2 ∉ ([1] : List Nat) ∧ 2 ∈ ([1, 2] : List Nat) := by
  decide

/-- A counterexample label requires a violation in this same decoded behavior. -/
theorem counterexample_label_justified (spec : InstanceContract Model Behavior Fact)
    (witness : ValidatedInstance spec) (label : witness.kind = .counterexample) :
    spec.violates witness.behavior := witness.counterexampleValid label

/-- AI text is untrusted annotation; references must identify existing instance facts. -/
structure Explanation (spec : InstanceContract Model Behavior Fact)
    (witness : ValidatedInstance spec) where
  text : String
  references : List Fact
  validReferences : ∀ fact, fact ∈ references → fact ∈ spec.facts witness.behavior

/-- Validating a cited fact establishes membership only, not the truth of AI prose. -/
theorem explanation_reference_exists (spec : InstanceContract Model Behavior Fact)
    (witness : ValidatedInstance spec) (explanation : Explanation spec witness)
    (fact : Fact) (cited : fact ∈ explanation.references) :
    fact ∈ spec.facts witness.behavior := explanation.validReferences fact cited

end SQLolla
