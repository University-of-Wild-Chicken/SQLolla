/**
 * Honest status vocabulary shared by banners, cards and details. It never maps
 * solver or error outcomes to a global safety claim.
 */
import type { AdapterFailure, EnumerationError, EnumerationStatus, EvidenceView, TransportFailure } from "./types";

/** A user-facing state label with tone and bounded explanation. */
export interface StatusDescriptor {
  readonly kind: string;
  readonly tone: "neutral" | "info" | "positive" | "caution" | "danger" | "muted";
  readonly label: string;
  readonly headline: string;
  readonly explanation: string;
}

/** A plain-language description of an assurance label. */
export interface AssuranceDescription {
  readonly label: string;
  readonly plain: string;
  readonly caveat: string;
  readonly known: boolean;
}

/** Describe a native or evaluator-backed assurance without merging the labels. */
export function describeAssurance(kind: string): AssuranceDescription {
  if (kind === "native_ra_alloy_with_rust_replay") {
    return {
      label: "Native RA + Alloy with Rust replay",
      plain: "An independent relational encoding was checked and the witness was replayed in Rust.",
      caveat: "This is bounded evidence for the supported profile, not a proof of all PostgreSQL behavior.",
      known: true,
    };
  }
  if (kind === "evaluator_backed_table_with_rust_replay") {
    return {
      label: "Evaluator-backed finite table + Rust replay",
      plain: "Alloy selected from a finite table produced by the Rust evaluator, then the witness was replayed.",
      caveat: "This checks encoding and replay, not independent SQL semantics; it is not a semantics proof.",
      known: true,
    };
  }
  return {
    label: "Unknown assurance label",
    plain: "The supplied assurance label is not recognised by this frontend.",
    caveat: "Treat the observation as untrusted and use the table/original JSON for inspection.",
    known: false,
  };
}

/** Describe a modeled SQL rejection while keeping it distinct from a host failure. */
function describeSqlError(evidence: EvidenceView): StatusDescriptor {
  return {
    kind: "sql_error",
    tone: "caution",
    label: "SQL error observed",
    headline: `The modeled statement returned SQLSTATE ${evidence.statement.error?.sqlstate ?? "unknown"}.`,
    explanation: "This is a modeled SQL rejection inside the stored evidence. It is not an analysis-host failure, a successful mutation, or a safety result.",
  };
}

/** Describe one bounded counterexample without claiming global impact. */
function describeCounterexample(): StatusDescriptor {
  return {
    kind: "counterexample",
    tone: "danger",
    label: "Counterexample found",
    headline: "A supplied case violates the selected rule inside this bounded model.",
    explanation:
      "Counterexamples identify concrete unintended behavior. They do not prove a production database is compromised or that every deployment is affected.",
  };
}

/** Describe a supplied mutation/effect observation as one finite transition. */
function describeMutation(): StatusDescriptor {
  return {
    kind: "mutation",
    tone: "info",
    label: "Mutation observed",
    headline: "The stored statement changed rows and produced the supplied observations.",
    explanation: "This is one finite transition with explicit effects and affected counts. It does not establish transaction or concurrency correctness.",
  };
}

/** Describe a satisfying stored example without equating it with safety. */
function describeExample(): StatusDescriptor {
  return {
    kind: "example",
    tone: "positive",
    label: "Example found",
    headline: "One stored finite execution is available for review.",
    explanation: "An example is a use case to preserve. It is not evidence that the property holds for all inputs or bounds.",
  };
}

/** Describe the stored instance status, with SQL error kept visible. */
export function describeEvidence(evidence: EvidenceView): StatusDescriptor {
  if (evidence.statement.outcome === "sql_error") return describeSqlError(evidence);
  if (evidence.instance.kind === "counterexample") return describeCounterexample();
  if (evidence.statement.outcome === "succeeded") return describeMutation();
  return describeExample();
}

/** Describe exhaustion and configured limits without inheriting a false safety flag. */
export function describeEnumerationStatus(status: EnumerationStatus): StatusDescriptor {
  if (status === "exhausted") {
    return {
      kind: "exhausted",
      tone: "neutral",
      label: "Exhausted within this exact model",
      headline: "The solver reported no further cases for this exact finite model.",
      explanation:
        "Exhaustion applies only to the supplied model, context, bounds and property. It is not a global safety or absence-of-counterexamples claim.",
    };
  }
  if (status === "limit") {
    return {
      kind: "limit",
      tone: "caution",
      label: "Configured limit reached",
      headline: "Enumeration stopped at its configured instance limit.",
      explanation: "The configured limit is not exhaustion. More cases may exist; no safety claim follows from stopping here.",
    };
  }
  return {
    kind: "instance",
    tone: "info",
    label: "Stored instance",
    headline: "One stored solver case is available.",
    explanation: "The interface does not infer safety from SAT, UNSAT, or an accepted use case.",
  };
}

interface ErrorRule {
  readonly matches: (error: EnumerationError) => boolean;
  readonly kind: string;
  readonly label: string;
  readonly explanation: string;
  readonly tone: StatusDescriptor["tone"];
}

const ERROR_RULES: readonly ErrorRule[] = [
  {
    matches: (e) => e.code.startsWith("unsupported.") || e.code.startsWith("syntax."),
    kind: "unsupported",
    label: "Unsupported feature",
    explanation: "This request needs a feature the current profile does not admit.",
    tone: "caution",
  },
  {
    matches: (e) => e.code.startsWith("sql.") || /^[0-9A-Z]{5}$/.test(e.code),
    kind: "sql_error",
    label: "SQL error returned by analysis",
    explanation: "The analysis stopped on a modeled SQL error. This does not imply success or safety.",
    tone: "danger",
  },
  {
    matches: (e) => e.code.startsWith("store."),
    kind: "archive_store",
    label: "Archive or store error",
    explanation: "The local evidence archive could not be trusted. No evidence was substituted.",
    tone: "danger",
  },
  {
    matches: (e) => e.code.startsWith("resource.") || /timeout|capacity|resource/i.test(e.message),
    kind: "timeout_resource",
    label: "Timeout or resource limit",
    explanation: "Work stopped at a declared capacity. This is inconclusive, not exhaustion.",
    tone: "danger",
  },
  {
    matches: (e) => e.code.includes("revision_changed") || e.code === "contract.stale_revision",
    kind: "stale_revision",
    label: "Stale model revision",
    explanation: "The response belongs to a different immutable revision and was discarded.",
    tone: "danger",
  },
  {
    matches: (e) => e.code.startsWith("evidence.replay") || e.code === "resume.prefix_changed" || e.code === "evidence.command",
    kind: "replay_mismatch",
    label: "Replay mismatch",
    explanation: "Deterministic replay did not match the solver observation. The evidence was refused.",
    tone: "danger",
  },
  {
    matches: (e) => e.code.startsWith("invalid.") || e.code === "evidence.property" || e.code === "evidence.assumptions" || e.code.includes("context"),
    kind: "missing_context",
    label: "Missing or invalid context",
    explanation: "The request needs complete, explicit context before analysis can be treated as evidence.",
    tone: "caution",
  },
];

/** Construct a failure descriptor with the stable backend code preserved for detail. */
function failureDescriptor(rule: Omit<ErrorRule, "matches">, error: EnumerationError): StatusDescriptor {
  return { ...rule, headline: error.message };
}

/** Fail-closed classification for structured backend analysis errors. */
function classifyEnumerationError(error: EnumerationError): StatusDescriptor {
  const rule = ERROR_RULES.find((candidate) => candidate.matches(error));
  return failureDescriptor(
    rule ?? {
      kind: "analysis_error",
      label: "Analysis stopped",
      explanation: "The backend returned an error. This state is not a safety result.",
      tone: "danger",
    },
    error,
  );
}

/** Describe a transport failure without conflating it with solver evidence. */
function describeTransport(failure: TransportFailure): StatusDescriptor {
  return {
    kind: "transport_error",
    tone: "caution",
    label: "Analysis host unavailable",
    headline: failure.message,
    explanation: "Transport failure is separate from SQL errors and solver evidence. No local evidence was replaced or reinterpreted.",
  };
}

/** Describe a stale contract response without applying it. */
function describeStaleContract(failure: AdapterFailure): StatusDescriptor {
  return {
    kind: "stale_revision",
    tone: "caution",
    label: "Stale revision discarded",
    headline: failure.message,
    explanation: "Responses from another model revision are not mixed into the current review. The previous cursor and evidence remain untouched.",
  };
}

/** Describe a rejected schema version without rewriting the payload. */
function describeUnsupportedEvidence(failure: AdapterFailure): StatusDescriptor {
  return {
    kind: "unsupported_evidence",
    tone: "danger",
    label: "Unsupported evidence format",
    headline: failure.message,
    explanation: "The schema version was rejected before fields were trusted. Evidence was not rewritten to fit this UI.",
  };
}

/** Normalize a frontend contract or transport failure into the same honest vocabulary. */
export function classifyFailure(failure: AdapterFailure | TransportFailure): StatusDescriptor {
  if (failure.kind === "transport") return describeTransport(failure);
  if (failure.code === "contract.stale_revision") return describeStaleContract(failure);
  if (failure.code === "contract.unsupported_schema") return describeUnsupportedEvidence(failure);
  return {
    kind: "contract_error",
    tone: "danger",
    label: "Evidence rejected",
    headline: failure.message,
    explanation: "Runtime shape validation rejected this payload. No result is inferred from a malformed response.",
  };
}

/** Classify either kind of analysis error into one display descriptor. */
export function classifyAnalysisError(error: AdapterFailure | EnumerationError | TransportFailure): StatusDescriptor {
  if (error.kind === "enumeration") return classifyEnumerationError(error);
  return classifyFailure(error);
}

/** Identify the supporting evidence state for a record with no instance. */
export function describeTerminalRecord(status: "exhausted" | "limit" | "instance"): StatusDescriptor {
  return describeEnumerationStatus(status);
}
