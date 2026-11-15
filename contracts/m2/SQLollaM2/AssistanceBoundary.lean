import SQLollaM2.InstanceReview
import SQLolla.Instance

/-! Assistance admission precedes host/provider implementation. These conditional
contracts do not prove provider prose, identity authentication, hashes, transport,
logging, process isolation, or credential erasure. Public-export independence holds
only for the specified projection with its public request fixed. -/
namespace SQLollaM2.AssistanceBoundary

/-- Every production assistance flow uses this exact model identifier without substitution. -/
def pinnedModel : String := "gpt-6.1-sol"

/-- An explanation binds both immutable evidence identity and the exact transmitted packet. -/
structure EvidenceIdentity where
  revision : String
  instanceId : String
  packetDigest : String
  deriving DecidableEq, Repr

variable {Model Behavior Fact : Type}
variable {spec : SQLolla.InstanceContract Model Behavior Fact}
variable {witness : SQLolla.ValidatedInstance spec}

/-- Selected facts retain the existing instance-view authenticity obligation. -/
structure ExplanationPacket (spec : SQLolla.InstanceContract Model Behavior Fact)
    (witness : SQLolla.ValidatedInstance spec) where
  identity : EvidenceIdentity
  view : SQLolla.InstanceView spec witness

/-- Provider prose and citations are untrusted input, separate from canonical evidence. -/
structure Annotation (Fact : Type) where
  identity : EvidenceIdentity
  requestedModel : String
  servedModel : String
  prose : String
  references : List Fact

/-- Admission checks identities and citation closure; it does not certify the prose. -/
structure AnnotationAdmission (expected : EvidenceIdentity)
    (packet : ExplanationPacket spec witness) (annotation : Annotation Fact) : Prop where
  packetCurrent : packet.identity = expected
  annotationCurrent : annotation.identity = expected
  requestedPinned : annotation.requestedModel = pinnedModel
  servedPinned : annotation.servedModel = pinnedModel
  referenceClosed : ∀ fact ∈ annotation.references, fact ∈ packet.view.visibleFacts

variable {expected : EvidenceIdentity} {packet : ExplanationPacket spec witness}
variable {annotation : Annotation Fact}

/-- A validated annotation cannot switch production model names silently. -/
theorem annotation_model_exact (valid : AnnotationAdmission expected packet annotation) :
    annotation.requestedModel = "gpt-6.1-sol" ∧ annotation.servedModel = "gpt-6.1-sol" :=
  ⟨valid.requestedPinned, valid.servedPinned⟩

/-- The annotation identifies the same source revision as its current packet. -/
theorem annotation_revision_current (valid : AnnotationAdmission expected packet annotation) :
    annotation.identity.revision = expected.revision :=
  congrArg EvidenceIdentity.revision valid.annotationCurrent

/-- A stale packet cannot be accepted by changing only the annotation's identity. -/
theorem stale_packet_rejected (different : packet.identity ≠ expected) :
    ¬ AnnotationAdmission expected packet annotation := fun valid => different valid.packetCurrent

/-- Every admitted citation belongs to the decoded source, including a selected partial view. -/
theorem annotation_reference_authentic (valid : AnnotationAdmission expected packet annotation)
    (fact : Fact) (cited : fact ∈ annotation.references) :
    fact ∈ spec.facts witness.behavior :=
  packet.view.authentic fact (valid.referenceClosed fact cited)

/-- Absence transfers only with the inherited complete-view obligation, never from redaction alone. -/
theorem packet_absence_requires_completeness (packet : ExplanationPacket spec witness)
    (complete : SQLolla.ViewComplete spec witness packet.view) (fact : Fact)
    (omitted : fact ∉ packet.view.visibleFacts) : fact ∉ spec.facts witness.behavior :=
  SQLolla.complete_view_absence spec witness packet.view complete fact omitted

/-- Approved redacted conclusions expose rule shape and occurrence positions. -/
inductive RedactedRuleObservation where
  | requiredValueMismatch (column occurrence : Nat)
  | forbiddenValueReturned (column occurrence : Nat)
  | rowLimitExceeded (occurrences : Nat)
  | executionError
  deriving DecidableEq, Repr

/-- The replayed conclusion and its private source values are separate parts of disclosure. -/
structure RuleDisclosure (Private : Type) where
  replayed : Option RedactedRuleObservation
  privatePayload : Private

/-- Redacted packets carry only the approved conclusion from replay. -/
def redactedObservation {Private : Type} (disclosure : RuleDisclosure Private) :
    Option RedactedRuleObservation := disclosure.replayed

/-- With the approved conclusion fixed, private SQL and values cannot change its projection. -/
theorem redacted_observation_independent_of_payload {Private : Type}
    (first second : RuleDisclosure Private) (same : first.replayed = second.replayed) :
    redactedObservation first = redactedObservation second := same

/-- The projection cannot manufacture a rule diagnosis when no replayed conclusion exists. -/
theorem redacted_observation_requires_replay {Private : Type}
    (payload : Private) : redactedObservation (⟨none, payload⟩ : RuleDisclosure Private) = none := rfl

/-- Private authentication material is deliberately separate from public request content. -/
structure PrivateInvocation (Credential PublicRequest : Type) where
  credential : Credential
  request : PublicRequest

/-- This public projection exports the fixed request and no authentication field. -/
def publicExport (invocation : PrivateInvocation Credential PublicRequest) : PublicRequest :=
  invocation.request

/-- Rotating a credential cannot change this projection when public content stays fixed. -/
theorem credential_export_noninterference (first second : Credential) (request : PublicRequest) :
    publicExport (⟨first, request⟩ : PrivateInvocation Credential PublicRequest) =
      publicExport (⟨second, request⟩ : PrivateInvocation Credential PublicRequest) := rfl

/-- Comparing arbitrary invocations still requires equality of their public request. -/
theorem equal_public_requests_equal_exports (first second : PrivateInvocation Credential PublicRequest)
    (same : first.request = second.request) : publicExport first = publicExport second := same

/-- Parts of the recorded analysis context whose exact values must survive proposal binding. -/
structure AnalysisContext (Catalog Parameters Domain Execution : Type) where
  catalog : Catalog
  parameters : Parameters
  domain : Domain
  execution : Execution
  deriving DecidableEq

/-- Exact source semantics and ordered output schema are independent of saved bundle identity. -/
structure AnalysisBasis (Context Property Bounds Schema : Type) where
  context : Context
  property : Property
  bounds : Bounds
  outputSchema : Schema
  deriving DecidableEq

/-- Instantiate the source basis with the separately preserved parts of the execution context. -/
abbrev ReviewBasis (Catalog Parameters Domain Execution Property Bounds Schema : Type) :=
  AnalysisBasis (AnalysisContext Catalog Parameters Domain Execution) Property Bounds Schema

/-- Source query and prompt are pinned for regeneration while old reviews can carry across queries. -/
structure AnalysisSource (Basis : Type) where
  revision : String
  query : String
  instruction : String
  basis : Basis

/-- A provider-generated proposal records both model identities; manual proposals need neither. -/
inductive ProposalOrigin where
  | manual
  | provider (requestedModel servedModel : String)

/-- AI refinement uses the same exact model identifier as AI instance summaries. -/
def ProposalOrigin.pinned : ProposalOrigin → Prop
  | .manual => True
  | .provider requested served => requested = pinnedModel ∧ served = pinnedModel

/-- Provider or low-code output is an unvalidated query tied to its source basis. -/
structure Proposed (Basis Query : Type) where
  origin : ProposalOrigin
  sourceRevision : String
  sourceQuery : String
  instruction : String
  basis : Basis
  query : Query

/-- Recorded SQL failures belong inside Behavior; incomplete engine executions are distinct. -/
inductive ReplayAttempt (Behavior : Type) where
  | complete (outcomes : List Behavior)
  | unsupported | failed | incomplete | budgetLimited

/-- Match exact canonical behavior on the archived input only after a complete attempt. -/
def checkReview [DecidableEq Behavior] (replay : Input → ReplayAttempt Behavior)
    (entry : ReviewObservation Input Behavior × InstanceDecision) : Bool :=
  match entry.2 with
  | .acceptedUseCase =>
    match replay entry.1.input with
    | .complete outcomes => decide (entry.1.behavior ∈ outcomes)
    | _ => false
  | .rejectedSideBehavior =>
    match replay entry.1.input with
    | .complete outcomes => decide (entry.1.behavior ∉ outcomes)
    | _ => false
  | .unsure => true

/-- The finite gate requires a Yes or No and checks every decisive observation. -/
def checkFeedback [DecidableEq Behavior] (replay : Input → ReplayAttempt Behavior)
    (feedback : List (ReviewObservation Input Behavior × InstanceDecision)) : Bool :=
  (feedback.any fun entry => entry.2.decisive) && feedback.all (checkReview replay)

/-- A passing snapshot contains a decisive label and each of its labels passes its check. -/
theorem feedback_passes [DecidableEq Behavior] (replay : Input → ReplayAttempt Behavior)
    (feedback : List (ReviewObservation Input Behavior × InstanceDecision)) :
    checkFeedback replay feedback = true ↔
      (∃ entry ∈ feedback, entry.2.decisive = true) ∧
      ∀ entry ∈ feedback, checkReview replay entry = true := by
  simp [checkFeedback, List.any_eq_true]

/-- An Unsure label imposes no outcome check, including when that input cannot be replayed. -/
theorem unsure_ignores_outcomes [DecidableEq Behavior] (replay : Input → ReplayAttempt Behavior)
    (observation : ReviewObservation Input Behavior) : checkReview replay (observation, .unsure) = true := rfl

/-- Failure, refusal, incompleteness and resource limits cannot pass either decision. -/
theorem passing_replay_complete [DecidableEq Behavior] (replay : Input → ReplayAttempt Behavior)
    (entry : ReviewObservation Input Behavior × InstanceDecision)
    (decisive : entry.2.decisive = true)
    (passes : checkReview replay entry = true) : ∃ outcomes, replay entry.1.input = .complete outcomes := by
  cases result : replay entry.1.input with
  | complete outcomes => exact ⟨outcomes, rfl⟩
  | _ =>
    cases decision : entry.2 <;> simp [checkReview, result, decision, InstanceDecision.decisive] at decisive passes

/-- Replay accuracy, immutable evidence resolution, and analysis are implementation obligations. -/
structure Checks (Basis Query Plan Input Behavior Schema : Type) where
  bind : Basis → Query → Option Plan
  outputSchema : Plan → Schema
  replay : Basis → Plan → Input → ReplayAttempt Behavior
  possible : Basis → Plan → Input → Behavior → Prop
  analyzed : Basis → Plan → Prop
  saved : ReviewKey → Option (SavedObservation Basis Input Behavior)
  authentic : ReviewKey → SavedObservation Basis Input Behavior → Prop
  digest : List ReviewEdit → String

/-- Binding carries the same source basis and the deterministic result for the proposed query. -/
structure Bound (checks : Checks Basis Query Plan Input Behavior Schema) (basis : Basis) where
  proposed : Proposed Basis Query
  unchanged : proposed.basis = basis
  plan : Plan
  binding : checks.bind basis proposed.query = some plan

/-- Exact deterministic binding results are unique. -/
theorem deterministic_binding_unique (checks : Checks Basis Query Plan Input Behavior Schema)
    (basis : Basis) (query : Query) (first second : Plan)
    (left : checks.bind basis query = some first) (right : checks.bind basis query = some second) :
    first = second := Option.some.inj (left.symm.trans right)

/-- Every reviewed input has a complete replay of candidate behaviors in the original basis. -/
structure Replayed (checks : Checks Basis Query Plan Input Behavior Schema) (basis : Basis)
    (feedback : List (ReviewObservation Input Behavior × InstanceDecision)) where
  bound : Bound checks basis
  complete : ∀ entry ∈ feedback, entry.2.decisive = true → ∃ outcomes,
    checks.replay basis bound.plan entry.1.input = .complete outcomes ∧
    ∀ behavior, behavior ∈ outcomes ↔ checks.possible basis bound.plan entry.1.input behavior

variable {Catalog Parameters Domain Execution Property Bounds Schema Query Plan Input Row State Effects : Type}
variable [DecidableEq Catalog] [DecidableEq Parameters] [DecidableEq Domain] [DecidableEq Execution]
variable [DecidableEq Property] [DecidableEq Bounds] [DecidableEq Schema]
variable [DecidableEq Row] [DecidableEq State] [DecidableEq Effects]

/-- Promotion requires exact binding, schema, analysis, authentic nonempty feedback and complete replay. -/
structure Analyzed (checks : Checks (ReviewBasis Catalog Parameters Domain Execution Property Bounds Schema)
    Query Plan Input (ReviewBehavior Row State Effects) Schema)
    (source : AnalysisSource (ReviewBasis Catalog Parameters Domain Execution Property Bounds Schema))
    (snapshot : ReviewSnapshot) where
  feedback : List (ReviewObservation Input (ReviewBehavior Row State Effects) × InstanceDecision)
  replayed : Replayed checks source.basis feedback
  outputPreserved : checks.outputSchema replayed.bound.plan = source.basis.outputSchema
  analysis : checks.analyzed source.basis replayed.bound.plan
  modelPinned : replayed.bound.proposed.origin.pinned
  sourceCurrent : snapshot.stamp.revision = source.revision
  proposalCurrent : replayed.bound.proposed.sourceRevision = source.revision
  sourceQueryPreserved : replayed.bound.proposed.sourceQuery = source.query
  instructionPreserved : replayed.bound.proposed.instruction = source.instruction
  digestAuthentic : snapshot.stamp.digest = checks.digest snapshot.history
  regenerationAllowed : canRegenerate snapshot.history = true
  resolved : resolveReviews source.basis checks.saved snapshot = some feedback
  archived : ∀ entry ∈ activeReviews snapshot.history, ∃ saved,
    checks.saved entry.1 = some saved ∧ checks.authentic entry.1 saved
  passes : checkFeedback (checks.replay source.basis replayed.bound.plan) feedback = true

/-- Only analyzed candidates carry evidence for promotion. -/
inductive Progress (checks : Checks (ReviewBasis Catalog Parameters Domain Execution Property Bounds Schema)
    Query Plan Input (ReviewBehavior Row State Effects) Schema)
    (source : AnalysisSource (ReviewBasis Catalog Parameters Domain Execution Property Bounds Schema))
    (snapshot : ReviewSnapshot) where
  | proposed (value : Proposed (ReviewBasis Catalog Parameters Domain Execution Property Bounds Schema) Query)
  | bound (value : Bound checks source.basis)
  | replayed (feedback : List (ReviewObservation Input (ReviewBehavior Row State Effects) × InstanceDecision))
      (value : Replayed checks source.basis feedback)
  | analyzed (value : Analyzed checks source snapshot)

variable {checks : Checks (ReviewBasis Catalog Parameters Domain Execution Property Bounds Schema)
    Query Plan Input (ReviewBehavior Row State Effects) Schema}
variable {source : AnalysisSource (ReviewBasis Catalog Parameters Domain Execution Property Bounds Schema)}
variable {snapshot : ReviewSnapshot}

/-- Publication and use both compare the certified source revision and review digest against current state. -/
def ready (progress : Progress checks source snapshot) (current : ReviewStamp) : Bool :=
  match progress with
  | .analyzed _ => decide (snapshot.stamp = current)
  | _ => false

/-- An unchecked proposal cannot acquire a checked status merely by containing SQL text. -/
theorem proposal_not_ready
    (value : Proposed (ReviewBasis Catalog Parameters Domain Execution Property Bounds Schema) Query)
    (current : ReviewStamp) :
    ready (Progress.proposed (checks := checks) (source := source) (snapshot := snapshot) value) current = false := rfl

/-- Successful binding alone lacks the complete replay, property analysis and feedback obligations. -/
theorem binding_not_ready (value : Bound checks source.basis) (current : ReviewStamp) :
    ready (Progress.bound (snapshot := snapshot) value) current = false := rfl

/-- Complete replay alone lacks the analysis and feedback obligations. -/
theorem replay_not_ready
    (feedback : List (ReviewObservation Input (ReviewBehavior Row State Effects) × InstanceDecision))
    (value : Replayed checks source.basis feedback) (current : ReviewStamp) :
    ready (Progress.replayed (snapshot := snapshot) feedback value) current = false := rfl

/-- Promotion cannot substitute source semantics or change the candidate's ordered output schema. -/
theorem analyzed_basis_and_schema (value : Analyzed checks source snapshot) :
    value.replayed.bound.proposed.basis = source.basis ∧
    checks.outputSchema value.replayed.bound.plan = source.basis.outputSchema :=
  ⟨value.replayed.bound.unchanged, value.outputPreserved⟩

/-- A new candidate keeps the reviewed source query and original instruction in its provenance. -/
theorem source_instruction_preserved (value : Analyzed checks source snapshot) :
    value.replayed.bound.proposed.sourceQuery = source.query ∧
    value.replayed.bound.proposed.instruction = source.instruction :=
  ⟨value.sourceQueryPreserved, value.instructionPreserved⟩

/-- A provider-generated candidate cannot pass with another requested or served model. -/
theorem analyzed_model_exact (value : Analyzed checks source snapshot) (requested served : String)
    (provider : value.replayed.bound.proposed.origin = .provider requested served) :
    requested = "gpt-6.1-sol" ∧ served = "gpt-6.1-sol" := by
  simpa [provider, ProposalOrigin.pinned, pinnedModel] using value.modelPinned

/-- Every accepted exact observation still occurs for its archived input. -/
theorem accepted_replayed (value : Analyzed checks source snapshot)
    (observation : ReviewObservation Input (ReviewBehavior Row State Effects))
    (accepted : (observation, .acceptedUseCase) ∈ value.feedback) :
    checks.possible source.basis value.replayed.bound.plan observation.input observation.behavior := by
  obtain ⟨outcomes, replay, complete⟩ := value.replayed.complete _ accepted rfl
  have passed := (feedback_passes _ _).mp value.passes |>.2 _ accepted
  exact (complete _).mp (by simpa [checkReview, replay] using passed)

/-- Every rejected exact observation is absent from all candidate behaviors for its archived input. -/
theorem rejected_not_replayed (value : Analyzed checks source snapshot)
    (observation : ReviewObservation Input (ReviewBehavior Row State Effects))
    (rejected : (observation, .rejectedSideBehavior) ∈ value.feedback) :
    ¬ checks.possible source.basis value.replayed.bound.plan observation.input observation.behavior := by
  obtain ⟨outcomes, replay, complete⟩ := value.replayed.complete _ rejected rfl
  have passed := (feedback_passes _ _).mp value.passes |>.2 _ rejected
  exact fun present => (by simpa [checkReview, replay] using passed : observation.behavior ∉ outcomes)
    ((complete _).mpr present)

/-- Conflicting active labels for the same input and behavior make certification impossible. -/
theorem contradictory_feedback_rejected (value : Analyzed checks source snapshot)
    (observation : ReviewObservation Input (ReviewBehavior Row State Effects))
    (accepted : (observation, .acceptedUseCase) ∈ value.feedback)
    (rejected : (observation, .rejectedSideBehavior) ∈ value.feedback) : False :=
  rejected_not_replayed value observation rejected (accepted_replayed value observation accepted)

/-- An empty or entirely Unsure snapshot cannot certify a candidate. -/
theorem undecided_reviews_not_analyzed (undecided : canRegenerate snapshot.history = false) :
    ¬ Nonempty (Analyzed checks source snapshot) := by
  rintro ⟨value⟩
  exact Bool.false_ne_true (undecided.symm.trans value.regenerationAllowed)

/-- Stale source revisions cannot construct an analyzed candidate. -/
theorem stale_feedback_rejected (different : snapshot.stamp.revision ≠ source.revision) :
    ¬ Nonempty (Analyzed checks source snapshot) := by
  rintro ⟨value⟩
  exact different value.sourceCurrent

/-- A changed review or source invalidates previously analyzed candidates at publication and use. -/
theorem stale_candidate_not_ready (value : Analyzed checks source snapshot) (current : ReviewStamp)
    (different : snapshot.stamp ≠ current) : ready (.analyzed value) current = false := by
  simp [ready, different]

end SQLollaM2.AssistanceBoundary
