import SQLollaM2.SqlFunction

/-! Name visibility, call coercion and ordered index rejection contracts.
The Rust candidate inventory and PostgreSQL metadata extraction remain separate obligations. -/
namespace SQLollaM2

/-- The system schema is implicitly searched first unless explicitly positioned. -/
def functionSearchPath (path : List String) : List String :=
  if "pg_catalog" ∈ path then path else "pg_catalog" :: path

/-- An explicitly positioned system schema preserves the supplied ordering. -/
theorem explicit_catalog_position (path : List String) (present : "pg_catalog" ∈ path) :
    functionSearchPath path = path := by simp [functionSearchPath, present]

/-- Omitting the system schema puts it before user schemas. -/
theorem implicit_catalog_first (path : List String) (absent : "pg_catalog" ∉ path) :
    functionSearchPath path = "pg_catalog" :: path := by simp [functionSearchPath, absent]

/-- A restricted numeric-call family distinguishes widening from assignment casts. -/
inductive CallType where
  | int16 | int32 | int64 | numeric | float64 | text | boolean
  deriving DecidableEq

/-- Only enumerated implicit function-argument conversions belong to this contract. -/
def implicitCallCast (source target : CallType) : Bool :=
  source == target || match source, target with
    | .int16, .int32 | .int16, .int64 | .int16, .numeric | .int16, .float64 => true
    | .int32, .int64 | .int32, .numeric | .int32, .float64 => true
    | .int64, .numeric | .int64, .float64 | .numeric, .float64 => true
    | _, _ => false

/-- An explicit bigint-to-integer cast does not imply a matching integer function overload. -/
theorem call_rejects_narrowing : implicitCallCast .int64 .int32 = false := rfl

/-- Typed text cannot acquire an integer signature through unknown-literal rules. -/
theorem call_rejects_typed_text : implicitCallCast .text .int32 = false := rfl

/-- Small integer arguments can reach a larger integer function signature. -/
theorem call_allows_widening : implicitCallCast .int32 .int64 = true := rfl

/-- Exact matches take priority; later coercion ranking is deliberately a separate obligation. -/
def exactOrCandidates (exact : α → Bool) (compatible : α → Bool) (candidates : List α) : List α :=
  let selected := candidates.filter exact
  if selected.isEmpty then candidates.filter compatible else selected

/-- A nonempty exact-match set cannot be displaced by merely coercible candidates. -/
theorem exact_candidates_win (exact compatible : α → Bool) (candidates : List α)
    (present : (candidates.filter exact).isEmpty = false) :
    exactOrCandidates exact compatible candidates = candidates.filter exact := by
  simp [exactOrCandidates, present]

/-- The first failing index in the supplied executor order determines the diagnostic. -/
def firstIndexFailure (check : α → Option β) : List α → Option β
  | [] => none
  | index :: rest => match check index with
    | some failure => some failure
    | none => firstIndexFailure check rest

/-- A preceding successful index does not hide a later failure. -/
theorem successful_index_continues (check : α → Option β) (index : α) (rest : List α)
    (passes : check index = none) :
    firstIndexFailure check (index :: rest) = firstIndexFailure check rest := by
  simp [firstIndexFailure, passes]

/-- Once an earlier index rejects, later indexes cannot replace its diagnostic. -/
theorem first_index_rejection (check : α → Option β) (index : α) (rest : List α) (failure : β)
    (fails : check index = some failure) :
    firstIndexFailure check (index :: rest) = some failure := by
  simp [firstIndexFailure, fails]

/-- Admitted index expressions depend on their row, never the supplied external session. -/
def ImmutableIndexExpression (evaluate : Context → Row → Result) : Prop :=
  ∀ left right row, evaluate left row = evaluate right row

/-- A row-only evaluator satisfies the external-session independence obligation. -/
theorem row_only_index_immutable (evaluate : Row → Result) :
    ImmutableIndexExpression (fun (_ : Context) row => evaluate row) := fun _ _ _ => rfl

/-- A statement-only executor lacks the later boundary required by initially deferred checks. -/
def statementTimingAdmitted (hasDeferredChecks hasTransactionBoundary : Bool) : Bool :=
  !hasDeferredChecks || hasTransactionBoundary

/-- A deferred context cannot silently become statement-immediate without a transaction model. -/
theorem deferred_requires_transaction_boundary : statementTimingAdmitted true false = false := rfl

/-- Ordinary immediate constraints do not require the unavailable transaction ending interface. -/
theorem immediate_statement_timing : statementTimingAdmitted false false = true := rfl

/-- Absolute value has exactly one numeric argument; NULL values do not change resolution. -/
def absCallResult : List CallType → Option CallType
  | [ty] => match ty with
    | .int16 | .int32 | .int64 | .numeric | .float64 => some ty
    | .text | .boolean => none
  | _ => none

/-- Boolean identity cannot be mistaken for a numeric absolute-value overload. -/
theorem abs_rejects_boolean : absCallResult [.boolean] = none := rfl

/-- Typed text cannot pass the numeric overload gate by formatting its value. -/
theorem abs_rejects_typed_text : absCallResult [.text] = none := rfl

/-- Missing arguments reject before any strict-NULL execution rule. -/
theorem abs_rejects_missing : absCallResult [] = none := rfl

/-- Extra arguments cannot be ignored by a unary function. -/
theorem abs_rejects_extra (left right : CallType) : absCallResult [left, right] = none := rfl

/-- Numeric resolution keeps the numeric family independently of storage precision. -/
theorem abs_numeric_family : absCallResult [.numeric] = some .numeric := rfl

/-- A lone unknown literal chooses the preferred floating overload, before parsing its value. -/
def resolveAbsArgument (known : Option CallType) : Option CallType :=
  absCallResult [known.getD .float64]

/-- Unknown text and NULL share preferred overload selection but retain their own value semantics. -/
theorem abs_unknown_float : resolveAbsArgument none = some .float64 := rfl

/-- A typed Boolean never receives the unknown-literal fallback. -/
theorem abs_known_boolean : resolveAbsArgument (some .boolean) = none := rfl

end SQLollaM2
