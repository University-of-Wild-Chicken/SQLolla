import SQLollaM2.Outcome

/-!
Explicit finite catalog/session contracts. Catalog snapshots retain every row of
their declared column projection. These contracts do not prove catalog extraction,
name parsing, PostgreSQL privileges, or Rust implementation preservation.
-/

namespace SQLollaM2

/-- Capacity admission retains the whole value or refuses it, never a truncated prefix. -/
def admitRows (capacity : Nat) (rows : List α) : Option (List α) :=
  if rows.length ≤ capacity then some rows else none

/-- Every admitted row occurrence is preserved, including duplicates. -/
theorem admitted_rows_exact (capacity : Nat) (rows : List α)
    (fits : rows.length ≤ capacity) : admitRows capacity rows = some rows := by
  simp [admitRows, fits]

/-- Capacity exhaustion is not an empty successful relation. -/
theorem rows_over_capacity (capacity : Nat) (rows : List α)
    (outside : ¬ rows.length ≤ capacity) : admitRows capacity rows = none := by
  simp [admitRows, outside]

/-- Raising an explicit capacity cannot alter an already admitted relation. -/
theorem capacity_preserves_admission (small large : Nat) (rows : List α)
    (fits : rows.length ≤ small) (growth : small ≤ large) :
    admitRows large rows = some rows := admitted_rows_exact large rows (Nat.le_trans fits growth)

/-- A declared column projection retains all source occurrences in their original order. -/
def catalogProjection (project : α → β) (rows : List α) : List β := rows.map project

/-- Projection cannot remove duplicate rows or silently reduce snapshot size. -/
theorem catalog_projection_length (project : α → β) (rows : List α) :
    (catalogProjection project rows).length = rows.length := by simp [catalogProjection]

/-- Expressions depending only on exposed columns agree with the full source snapshot. -/
theorem catalog_projection_observation (project : α → β) (observe : β → γ) (rows : List α) :
    (catalogProjection project rows).map observe = rows.map (observe ∘ project) := by
  simp [catalogProjection, List.map_map]

/-- OIDs remain unsigned32 values with nominal type separate from signed SQL integers. -/
def catalogOid (value : Nat) : Option Nat :=
  if value < 2 ^ 32 then some value else none

/-- The top unsigned OID is admitted without converting it to a negative integer. -/
theorem catalog_oid_unsigned_boundary :
    catalogOid 4294967295 = some 4294967295 ∧ catalogOid 4294967296 = none := ⟨rfl, rfl⟩

/-- OID to int32 casts reinterpret the stored unsigned32 bits as signed two's complement. -/
def catalogOidInteger (oid : Fin (2 ^ 32)) : Int :=
  if oid.val < 2 ^ 31 then Int.ofNat oid.val else Int.ofNat oid.val - 2 ^ 32

/-- Signed32 to OID casts preserve the same low-order bits. -/
def integerCatalogOid (value : Int) : Nat := (value % (2 ^ 32)).toNat

/-- The signed/unsigned cast boundary differs from an overflow-checked bigint conversion. -/
theorem catalog_oid_integer_boundary :
    catalogOidInteger ⟨4294967295, by decide⟩ = -1 ∧ integerCatalogOid (-1) = 4294967295 :=
  ⟨rfl, rfl⟩

/-- Internal catalog char comparison uses an unsigned byte, unlike its integer cast. -/
def catalogCharInteger (byte : Fin 256) : Int :=
  if byte.val < 128 then Int.ofNat byte.val else Int.ofNat byte.val - 256

/-- High-bit internal chars have negative integer casts but retain unsigned comparison order. -/
theorem catalog_char_signed_cast : catalogCharInteger ⟨255, by decide⟩ = -1 ∧
    (127 : Nat) < 255 := ⟨rfl, by decide⟩

/-- Omitted modeling evidence differs from an observed absent server setting. -/
inductive SessionResolution (α : Type) where
  | value (cell : Option α)
  | absentSetting
  | unmodeled
  deriving DecidableEq

/-- The outer option is evidence availability; the inner option is SQL NULL/absence. -/
def resolveSession (missingOk : Bool) (supplied : Option (Option α)) : SessionResolution α :=
  match supplied with
  | none => .unmodeled
  | some none => if missingOk then .value none else .absentSetting
  | some (some value) => .value (some value)

/-- Missing evidence can never be reported as a proved SQL error or harmless NULL. -/
theorem missing_session_evidence (missingOk : Bool) :
    resolveSession (α := α) missingOk none = .unmodeled := rfl

/-- Explicit absence respects current_setting's declared missing_ok policy. -/
theorem observed_absent_setting :
    resolveSession (α := α) true (some none) = .value none ∧
    resolveSession (α := α) false (some none) = .absentSetting := ⟨rfl, rfl⟩

/-- Relation lookup carries object identity independently of its display spelling. -/
structure CatalogIdentity where
  schema : String
  name : String
  oid : Nat
  deriving DecidableEq

/-- Resolve an exact qualified identifier in an explicitly complete supplied inventory. -/
def qualifiedRelation (inventory : List CatalogIdentity) (schema name : String) : Option Nat :=
  (inventory.find? (fun value => value.schema == schema && value.name == name)).map (·.oid)

/-- The first existing relation along the effective search path determines unqualified lookup. -/
def pathRelation (inventory : List CatalogIdentity) (name : String) : List String → Option Nat
  | [] => none
  | schema :: rest => match qualifiedRelation inventory schema name with
    | some oid => some oid
    | none => pathRelation inventory name rest

/-- Lookup refuses missing modeling context; observed absence is a NULL regclass identity. -/
def completeRelationLookup (complete : Bool) (result : Option Nat) : SessionResolution Nat :=
  if complete then .value result else .unmodeled

/-- An earlier matching schema shadows later relations of the same name. -/
theorem relation_path_first (inventory : List CatalogIdentity) (name schema : String)
    (rest : List String) (oid : Nat) (found : qualifiedRelation inventory schema name = some oid) :
    pathRelation inventory name (schema :: rest) = some oid := by simp [pathRelation, found]

/-- A missing relation in one existing schema allows lookup to continue along the path. -/
theorem relation_path_fallback (inventory : List CatalogIdentity) (name schema : String)
    (rest : List String) (absent : qualifiedRelation inventory schema name = none) :
    pathRelation inventory name (schema :: rest) = pathRelation inventory name rest := by
  simp [pathRelation, absent]

/-- An empty effective path cannot resolve an unqualified relation. -/
theorem relation_empty_path (inventory : List CatalogIdentity) (name : String) :
    pathRelation inventory name [] = none := rfl

/-- An incomplete catalog never establishes that a relation is absent. -/
theorem incomplete_relation_lookup (result : Option Nat) :
    completeRelationLookup false result = .unmodeled := rfl

/-- A complete inventory may establish absence without inventing a SQL failure. -/
theorem complete_absent_relation : completeRelationLookup true none = .value none := rfl

/-- A regclass and an OID can share numeric identity without erasing their nominal type. -/
inductive CatalogScalar where
  | oid (value : Nat)
  | regclass (value : Nat)
  deriving DecidableEq

/-- Scalar transport retains the nominal object-reference family. -/
theorem catalog_nominal_identity (value : Nat) : CatalogScalar.oid value ≠ .regclass value := by
  intro equality
  cases equality

/-- Namespace absence and denied schema USAGE are observably different lookup results. -/
def namespaceLookup (usage : Option Bool) (resolved : Option Nat) : Except String (Option Nat) :=
  match usage with
  | none => .ok none
  | some false => .error "42501"
  | some true => .ok resolved

/-- A relation in an inaccessible schema must not be exposed as a successful lookup. -/
theorem denied_namespace_lookup (resolved : Option Nat) :
    namespaceLookup (some false) resolved = .error "42501" := rfl

/-- An absent schema remains a NULL lookup, independently of the placeholder resolution value. -/
theorem absent_namespace_lookup (resolved : Option Nat) : namespaceLookup none resolved = .ok none := rfl

/-- Schema USAGE permits the observed relation identity or its genuine absence to pass through. -/
theorem allowed_namespace_lookup (resolved : Option Nat) :
    namespaceLookup (some true) resolved = .ok resolved := rfl

/-- Primitive text admission excludes PostgreSQL-impossible NUL and enforces an explicit byte budget. -/
def textAdmissible (budget : Nat) (value : String) : Bool :=
  !value.toList.contains (Char.ofNat 0) && value.utf8ByteSize ≤ budget

/-- Admitted text is retained exactly; this transport gate does not reinterpret bytes as SQL errors. -/
def admitText (budget : Nat) (value : String) : Option String :=
  if textAdmissible budget value then some value else none

/-- No caller can obtain a truncated successful text value from this admission function. -/
theorem admitted_text_exact (budget : Nat) (value : String)
    (accepted : textAdmissible budget value = true) : admitText budget value = some value := by
  simp [admitText, accepted]

/-- A NUL codepoint cannot be represented by PostgreSQL text even when the byte budget permits it. -/
theorem nul_text_refused : admitText 100 (String.singleton (Char.ofNat 0)) = none := rfl

/-- Capacity refusal preserves the distinction between empty text and omitted evidence. -/
theorem text_byte_limit : admitText 0 "x" = none ∧ admitText 0 "" = some "" := ⟨rfl, rfl⟩

/-- Strict regclass input separates unavailable evidence from observed missing-object SQL failure. -/
def strictRelationInput (complete : Bool) (resolved : Option Nat) : Option (Except String Nat) :=
  if !complete then none else some (match resolved with
    | none => .error "42P01"
    | some oid => .ok oid)

/-- Missing catalog evidence cannot be relabeled as an absent PostgreSQL relation. -/
theorem strict_relation_missing_context (resolved : Option Nat) :
    strictRelationInput false resolved = none := rfl

/-- An observed absent relation raises an error in a cast, unlike to_regclass's NULL result. -/
theorem strict_relation_absent : strictRelationInput true none = some (.error "42P01") := rfl

/-- Successful strict input retains the exact catalog object identity. -/
theorem strict_relation_found (oid : Nat) :
    strictRelationInput true (some oid) = some (.ok oid) := rfl

/-- A strict nextval call on SQL NULL performs no allocation and preserves the supplied state. -/
def nullableAllocation (input : Option α) (state : State) (allocate : α → State → State × Value) :
    State × Option Value :=
  match input with
  | none => (state, none)
  | some value => let result := allocate value state; (result.1, some result.2)

/-- SQL NULL and a missing sequence identity have different observable behavior. -/
theorem null_allocation_retains_state (state : State) (allocate : α → State → State × Value) :
    nullableAllocation none state allocate = (state, none) := rfl

end SQLollaM2
