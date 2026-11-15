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

/-- Exact semantic inputs remain fixed while a proposed query receives a new analysis. -/
structure AnalysisBasis (Context Property Bounds : Type) where
  revision : String
  context : Context
  property : Property
  bounds : Bounds

/-- Provider or low-code output is only an unvalidated proposal tied to its source basis. -/
structure Proposed (Basis Query : Type) where
  basis : Basis
  query : Query

/-- Deterministic binding and semantic obligations must be supplied by the implementation. -/
structure Checks (Basis Query Plan Observation : Type) where
  bind : Basis → Query → Option Plan
  possible : Basis → Plan → Observation → Prop
  analyzed : Basis → Plan → Prop

/-- Binding carries its source proposal, unchanged basis, and exact deterministic result. -/
structure Bound (checks : Checks Basis Query Plan Observation) (basis : Basis) where
  proposed : Proposed Basis Query
  unchanged : proposed.basis = basis
  plan : Plan
  binding : checks.bind basis proposed.query = some plan

/-- Replay must enumerate all admitted observations, including SQL errors and retained effects. -/
structure Replayed (checks : Checks Basis Query Plan Observation) (basis : Basis) where
  bound : Bound checks basis
  observations : List Observation
  complete : ∀ value, value ∈ observations ↔ checks.possible basis bound.plan value

/-- Feedback is revision-bound and remains separate from solver evidence. -/
structure Feedback (Observation : Type) where
  revision : String
  accepted : List Observation
  rejected : List Observation

/-- Promotion requires the whole prior chain plus analysis and conditional intent preservation. -/
structure Analyzed (checks : Checks (AnalysisBasis Context Property Bounds) Query Plan Observation)
    (basis : AnalysisBasis Context Property Bounds) (feedback : Feedback Observation) where
  replayed : Replayed checks basis
  analysis : checks.analyzed basis replayed.bound.plan
  feedbackCurrent : feedback.revision = basis.revision
  fits : FitsInstanceFeedback (checks.possible basis replayed.bound.plan)
    feedback.accepted feedback.rejected

/-- Workflow states retain prior witnesses; raw provider output has no analyzed constructor evidence. -/
inductive Progress (checks : Checks (AnalysisBasis Context Property Bounds) Query Plan Observation)
    (basis : AnalysisBasis Context Property Bounds) (feedback : Feedback Observation) where
  | proposed (value : Proposed (AnalysisBasis Context Property Bounds) Query)
  | bound (value : Bound checks basis)
  | replayed (value : Replayed checks basis)
  | analyzed (value : Analyzed checks basis feedback)

variable {Context Property Bounds Query Plan Observation : Type}
variable {checks : Checks (AnalysisBasis Context Property Bounds) Query Plan Observation}
variable {basis : AnalysisBasis Context Property Bounds} {feedback : Feedback Observation}

/-- Only the analyzed state has all declared promotion obligations. -/
def ready (progress : Progress checks basis feedback) : Bool :=
  match progress with
  | .analyzed _ => true
  | _ => false

/-- An unchecked proposal cannot acquire a checked status merely by containing SQL text. -/
theorem proposal_not_ready (value : Proposed (AnalysisBasis Context Property Bounds) Query) :
    ready (Progress.proposed (checks := checks) (basis := basis) (feedback := feedback) value) = false := rfl

/-- Successful binding alone does not establish replay completeness or property analysis. -/
theorem binding_not_ready (value : Bound checks basis) :
    ready (Progress.bound (feedback := feedback) value) = false := rfl

/-- Complete replay alone still lacks the analysis and feedback obligations. -/
theorem replay_not_ready (value : Replayed checks basis) :
    ready (Progress.replayed (feedback := feedback) value) = false := rfl

/-- A promoted candidate retains the exact compiler result for its proposed query. -/
theorem analyzed_binding (value : Analyzed checks basis feedback) :
    checks.bind basis value.replayed.bound.proposed.query = some value.replayed.bound.plan :=
  value.replayed.bound.binding

/-- Two successful bindings of the same input cannot select different plans. -/
theorem deterministic_binding_unique (checks : Checks Basis Query Plan Observation)
    (basis : Basis) (query : Query) (first second : Plan)
    (left : checks.bind basis query = some first) (right : checks.bind basis query = some second) :
    first = second := Option.some.inj (left.symm.trans right)

/-- Promotion preserves all source context, property and finite bounds together. -/
theorem analyzed_basis_unchanged (value : Analyzed checks basis feedback) :
    value.replayed.bound.proposed.basis = basis := value.replayed.bound.unchanged

/-- A candidate cannot satisfy feedback by silently replacing the execution context. -/
theorem analyzed_context_unchanged (value : Analyzed checks basis feedback) :
    value.replayed.bound.proposed.basis.context = basis.context :=
  congrArg AnalysisBasis.context value.replayed.bound.unchanged

/-- A candidate cannot replace the property being checked while claiming the same analysis. -/
theorem analyzed_property_unchanged (value : Analyzed checks basis feedback) :
    value.replayed.bound.proposed.basis.property = basis.property :=
  congrArg AnalysisBasis.property value.replayed.bound.unchanged

/-- Reducing the declared finite domain cannot itself count as excluding an unwanted observation. -/
theorem analyzed_bounds_unchanged (value : Analyzed checks basis feedback) :
    value.replayed.bound.proposed.basis.bounds = basis.bounds :=
  congrArg AnalysisBasis.bounds value.replayed.bound.unchanged

/-- Every accepted observation occurs in the candidate's complete replay under these assumptions. -/
theorem accepted_replayed (value : Analyzed checks basis feedback) (observation : Observation)
    (accepted : observation ∈ feedback.accepted) : observation ∈ value.replayed.observations :=
  (value.replayed.complete observation).mpr (value.fits.1 observation accepted)

/-- No rejected observation occurs in the same complete replay under these assumptions. -/
theorem rejected_not_replayed (value : Analyzed checks basis feedback) (observation : Observation)
    (rejected : observation ∈ feedback.rejected) : observation ∉ value.replayed.observations := by
  intro present
  exact value.fits.2 observation rejected ((value.replayed.complete observation).mp present)

/-- Stale review labels cannot construct a promoted candidate in the current source revision. -/
theorem stale_feedback_rejected (different : feedback.revision ≠ basis.revision) :
    ¬ Nonempty (Analyzed checks basis feedback) := by
  rintro ⟨candidate⟩
  exact different candidate.feedbackCurrent

/-- Contradictory accepted and rejected labels block this promotion contract. -/
theorem contradictory_feedback_rejected (observation : Observation)
    (accepted : observation ∈ feedback.accepted) (rejected : observation ∈ feedback.rejected) :
    ¬ Nonempty (Analyzed checks basis feedback) := by
  rintro ⟨value⟩
  exact value.fits.2 observation rejected (value.fits.1 observation accepted)

end SQLollaM2.AssistanceBoundary
