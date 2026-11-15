/-! Same-origin admission for one configured browser origin and a loopback listener.
Canonical origin parsing, authenticated ingress, TLS and nonce secrecy remain host obligations. -/
namespace SQLollaM2.HostOrigin

/-- Direct localhost HTTP and IIS-terminated HTTPS use the same header admission rule. -/
inductive Scheme where
  | http | https

/-- The configured origin contains a canonical browser authority and no path or credentials. -/
structure Origin where
  scheme : Scheme
  authority : String

/-- Compare Origin against a configured scheme and authority. -/
def Origin.value (origin : Origin) : String :=
  (match origin.scheme with | .http => "http://" | .https => "https://") ++ origin.authority

/-- Public origin settings have no effect on the native bind address. -/
def localListener (port : UInt16) : String × UInt16 := ("127.0.0.1", port)

/-- The configured host listens on IPv4 loopback at the selected port. -/
theorem listener_loopback (port : UInt16) : (localListener port).1 = "127.0.0.1" := rfl

/-- Only the browser methods used by the host enter the application. -/
inductive Method where
  | get | head | post | other

/-- Field multiplicity remains visible so duplicate authorities or capabilities fail closed. -/
structure Headers where
  host : List String
  origin : List String
  csrf : List String
  json : Bool
  forwarded : List String

/-- GET and HEAD accept an omitted Origin; POST binds both Origin and the memory-only capability. -/
def admits (origin : Origin) (csrf : String) (method : Method) (headers : Headers) : Bool :=
  decide (headers.host = [origin.authority]) &&
    match method with
    | .get | .head => headers.origin.isEmpty || decide (headers.origin = [origin.value])
    | .post => decide (headers.origin = [origin.value]) && decide (headers.csrf = [csrf]) && headers.json
    | .other => false

/-- A browser POST must keep both configured identities, exactly one capability and its JSON type. -/
theorem post_admission_exact (origin : Origin) (csrf : String) (headers : Headers) :
    admits origin csrf .post headers = true ↔
      headers.host = [origin.authority] ∧ headers.origin = [origin.value] ∧
      headers.csrf = [csrf] ∧ headers.json = true := by
  simp [admits, and_assoc]

/-- A configured HTTPS origin rejects a loopback or public HTTP Origin report. -/
theorem mismatched_origin_refused (origin : Origin) (csrf : String) (headers : Headers)
    (different : headers.origin ≠ [origin.value]) : admits origin csrf .post headers = false := by
  simp [admits, different]

/-- Forwarding metadata cannot establish authority, origin or capability. -/
theorem forwarded_independent (origin : Origin) (csrf : String) (method : Method) (headers : Headers)
    (forwarded : List String) :
    admits origin csrf method { headers with forwarded } = admits origin csrf method headers := rfl

end SQLollaM2.HostOrigin
