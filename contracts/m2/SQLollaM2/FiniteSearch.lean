import SQLollaM2.Coverage

/-!
A finite execution table is an explicit bounded model, not an independent SQL
interpreter. Its correctness still requires evaluator/reference correspondence.
Table membership and a selected model are kept separate from property meaning.
-/

namespace SQLollaM2

/-- Enumerate every supplied outcome of every input without selecting a preferred tie. -/
def executionTable (inputs : List Input) (evaluate : Input → List Output) :
    List (Input × Output) := inputs.flatMap fun input =>
  (evaluate input).map fun output => (input, output)

/-- The table contains exactly the admitted input/evaluator-output pairs. -/
theorem execution_table_membership (inputs : List Input) (evaluate : Input → List Output)
    (input : Input) (output : Output) :
    (input, output) ∈ executionTable inputs evaluate ↔
      input ∈ inputs ∧ output ∈ evaluate input := by
  simp [executionTable]

/-- A finite model may select only an explicitly enumerated transition. -/
structure SelectedTransition (table : List (Input × Output)) where
  input : Input
  output : Output
  included : (input, output) ∈ table

/-- Decoding a selected transition cannot invent an evaluator outcome. -/
theorem selected_transition_sound (inputs : List Input) (evaluate : Input → List Output)
    (selected : SelectedTransition (executionTable inputs evaluate)) :
    selected.input ∈ inputs ∧ selected.output ∈ evaluate selected.input :=
  (execution_table_membership inputs evaluate selected.input selected.output).mp selected.included

/-- Every enumerated evaluator outcome has a selectable transition witness. -/
def selectEnumerated (inputs : List Input) (evaluate : Input → List Output)
    (input : Input) (output : Output) (within : input ∈ inputs)
    (produced : output ∈ evaluate input) : SelectedTransition (executionTable inputs evaluate) :=
  ⟨input, output, (execution_table_membership inputs evaluate input output).mpr ⟨within, produced⟩⟩

/-- No bad table member excludes violations only over those inputs and all supplied outcomes. -/
theorem finite_no_violation (inputs : List Input) (evaluate : Input → List Output)
    (bad : Input → Output → Prop)
    (safe : ∀ pair ∈ executionTable inputs evaluate, ¬ bad pair.1 pair.2)
    (input : Input) (output : Output) (within : input ∈ inputs)
    (produced : output ∈ evaluate input) : ¬ bad input output :=
  safe (input, output) ((execution_table_membership inputs evaluate input output).mpr ⟨within, produced⟩)

/-- State and retained effects are observations, independently of returned result rows. -/
structure CompleteObservation (State Rows Effects : Type) where
  before : State
  after : State
  rows : Rows
  effects : Effects

/-- Authentic decoding preserves the complete observation, not only a result projection. -/
def ObservationAuthentic (expected decoded : CompleteObservation State Rows Effects) : Prop :=
  expected = decoded

/-- Complete observation identity implies poststate and retained-effect identity. -/
theorem observation_effects_preserved (expected decoded : CompleteObservation State Rows Effects)
    (authentic : ObservationAuthentic expected decoded) :
    expected.after = decoded.after ∧ expected.effects = decoded.effects := by
  cases authentic
  exact ⟨rfl, rfl⟩

/-- An Alloy assertion separates the admissible query model from the requested property. -/
def ModelAssertion (semantics property : World → Prop) : Prop :=
  ∀ world, semantics world → property world

/-- Checking negates the assertion and seeks an admissible violating world. -/
def CheckCounterexample (semantics property : World → Prop) : Prop :=
  ∃ world, semantics world ∧ ¬ property world

/-- A failed assertion is exactly an admissible counterexample, within the same world domain. -/
theorem check_negates_assertion (semantics property : World → Prop) :
    ¬ ModelAssertion semantics property ↔ CheckCounterexample semantics property := by
  classical
  simp [ModelAssertion, CheckCounterexample]

/-- Every check witness satisfies query semantics; invalid worlds cannot refute the property. -/
theorem check_witness_admissible (semantics property : World → Prop)
    (witness : CheckCounterexample semantics property) : ∃ world, semantics world := by
  obtain ⟨world, admissible, _⟩ := witness
  exact ⟨world, admissible⟩

end SQLollaM2
