/**
 * Demonstration explanation and context preview. This is fixture content, not
 * an AI provider call, and every generated claim cites an existing supplied fact.
 */
import { canonicalJson, cellText, propertySentence } from "../analysis/format";
import { AI_ENDPOINT, AI_MODEL } from "./config";
import { stringifyJson } from "../analysis/json";
import type { DemoClaim, DemoSummary, DemoSummarySeed, EvidenceView, FactModel, OccurrenceView, RelationView } from "../analysis/types";

/** A provider's display identity for demo-only explanation runs. */
export type AiProviderMode = "offline_demo" | "openai_compatible" | "anthropic_compatible" | "gemini_compatible";

/** In-memory provider selection; credentials never enter persistence or exports. */
export interface AiProviderSettings {
  readonly mode: AiProviderMode;
  readonly model: string;
  readonly endpoint: string;
  readonly apiKey: string;
  readonly redactValues: boolean;
  readonly simulateFailure: boolean;
  readonly preparedInMemory: boolean;
}

/** Safe default: no provider account, no secret and no egress. */
export const defaultAiSettings: AiProviderSettings = {
  mode: "offline_demo",
  model: AI_MODEL,
  endpoint: AI_ENDPOINT,
  apiKey: "",
  redactValues: true,
  simulateFailure: false,
  preparedInMemory: false,
};

/** Lookup for a supplied fact by exact JSON Pointer. */
function factByReference(evidence: EvidenceView, reference: string): FactModel | undefined {
  return evidence.facts.find((fact) => fact.reference === reference);
}

/** Return a fact only when the generated claim can actually cite it. */
function citedOccurrence(occurrence: OccurrenceView, evidence: EvidenceView): FactModel | null {
  return occurrence.factReference ? (factByReference(evidence, occurrence.factReference) ?? null) : null;
}

/** Pick the first before occurrence that has a supplied fact. */
function firstInputOccurrence(evidence: EvidenceView): { table: RelationView; occurrence: OccurrenceView; fact: FactModel } | null {
  for (const table of evidence.inputTables) {
    for (const occurrence of table.rows) {
      const fact = citedOccurrence(occurrence, evidence);
      if (fact) return { table, occurrence, fact };
    }
  }
  return null;
}

/** Find the first exact duplicate pair and both separate fact identities. */
function firstDuplicate(evidence: EvidenceView): { occurrence: OccurrenceView; partner: OccurrenceView; fact: FactModel } | null {
  for (const table of evidence.inputTables) {
    const groups = new Map<string, OccurrenceView[]>();
    for (const occurrence of table.rows) {
      const key = canonicalJson(occurrence.cells.map((cell) => cell.raw));
      groups.set(key, [...(groups.get(key) ?? []), occurrence]);
    }
    for (const group of groups.values()) {
      if (group.length > 1) {
        const fact = citedOccurrence(group[0]!, evidence);
        if (fact) return { occurrence: group[0]!, partner: group[1]!, fact };
      }
    }
  }
  return null;
}

/** Find the first explicit typed NULL with a supplied fact. */
function firstTypedNull(evidence: EvidenceView): { occurrence: OccurrenceView; cellIndex: number; fact: FactModel } | null {
  for (const table of evidence.inputTables) {
    for (const occurrence of table.rows) {
      const cellIndex = occurrence.cells.findIndex((cell) => cell.type === "null");
      const fact = citedOccurrence(occurrence, evidence);
      if (cellIndex >= 0 && fact) return { occurrence, cellIndex, fact };
    }
  }
  return null;
}

/** Format one occurrence as a compact fact-grounded label. */
function occurrenceLabel(table: RelationView | null, occurrence: OccurrenceView): string {
  const location = table ? ` in ${table.tableId}` : "";
  const values = occurrence.cells.map(cellText).join(" · ");
  return `${location} occurrence #${occurrence.position}: ${values}`;
}

/** Build a grounded claim for the first before occurrence. */
function inputClaim(evidence: EvidenceView): DemoClaim | null {
  const input = firstInputOccurrence(evidence);
  return input
    ? {
        text: `Before the statement, ${occurrenceLabel(input.table, input.occurrence)}.`,
        factReferences: [input.fact.reference],
      }
    : null;
}

/** Build a grounded claim showing that equal values remain separate occurrences. */
function duplicateClaim(evidence: EvidenceView): DemoClaim | null {
  const duplicate = firstDuplicate(evidence);
  if (!duplicate) return null;
  return {
    text: `The same row value appears as separate occurrences #${duplicate.occurrence.position} and #${duplicate.partner.position}; duplicates are not collapsed.`,
    factReferences: [duplicate.fact.reference, duplicate.partner.factReference ?? duplicate.fact.reference],
  };
}

/** Build a grounded claim for explicit typed NULL. */
function nullClaim(evidence: EvidenceView): DemoClaim | null {
  const typedNull = firstTypedNull(evidence);
  if (!typedNull) return null;
  const cell = typedNull.occurrence.cells[typedNull.cellIndex];
  return {
    text: `Occurrence #${typedNull.occurrence.position} contains a typed NULL, displayed as NULL · ${String(cell?.value)}.`,
    factReferences: [typedNull.fact.reference],
  };
}

/** Build a grounded claim for the first supplied output occurrence. */
function outputClaim(evidence: EvidenceView): DemoClaim | null {
  const output = evidence.output.rows.find((row) => citedOccurrence(row, evidence));
  if (!output) return null;
  const fact = citedOccurrence(output, evidence)!;
  return {
    text: `The supplied output includes ${occurrenceLabel(null, output)}.`,
    factReferences: [fact.reference],
  };
}

/** Build claims that can always resolve to facts in the supplied instance. */
function buildClaims(evidence: EvidenceView): readonly DemoClaim[] {
  return [inputClaim, duplicateClaim, nullClaim, outputClaim]
    .flatMap((build) => {
      const claim = build(evidence);
      return claim ? [claim] : [];
    })
    .slice(0, 4);
}

/** Build the visible fixture summary from canonical counts and supplied facts. */
export function buildDemoSummary(evidence: EvidenceView, seed: DemoSummarySeed): DemoSummary {
  return {
    title: seed.title,
    claims: buildClaims(evidence),
    assumptions: [...seed.assumptions, propertySentence(evidence.bundle.request.property, evidence.output.columns)],
    proposedEdit: seed.proposedEdit,
    proposedEditLabel: seed.proposedEditLabel,
    demonstrated: true,
  };
}

/** Render a provider label without asserting live availability. */
export function providerLabel(mode: AiProviderMode): string {
  const labels: Record<AiProviderMode, string> = {
    offline_demo: "Offline demo summary",
    openai_compatible: "OpenAI",
    anthropic_compatible: "Anthropic unavailable in this host",
    gemini_compatible: "Gemini unavailable in this host",
  };
  return labels[mode];
}

/** Build a future-send context preview. It never includes credentials or sends data. */
export function buildContextPreview(evidence: EvidenceView, settings: AiProviderSettings): string {
  const redact = settings.redactValues;
  const factLines = evidence.facts.slice(0, 4).map((fact) => {
    const value = redact ? "[redacted]" : stringifyJson(fact.value);
    return `- ${fact.reference} (${fact.kind}) => ${value}`;
  });
  return [
    `Provider: ${providerLabel(settings.mode)}`,
    "Network request: NONE — requests require an explicit action after closing settings.",
    `Redaction: ${redact ? "values redacted" : "values included"}`,
    `SQL: ${redact ? "[redacted]" : evidence.bundle.request.sql}`,
    `Rule: ${redact ? "Verified rule outcome and witness position when available; rule values omitted" : propertySentence(evidence.bundle.request.property, evidence.output.columns)}`,
    `Bounds: ${stringifyJson(evidence.bundle.request.bounds)}`,
    `Domain values: ${evidence.bundle.request.domain.length}`,
    `Tables: ${evidence.inputTables.map((table) => table.tableId).join(", ") || "none"}`,
    `Input occurrences: ${evidence.inputTables.reduce((sum, table) => sum + table.rows.length, 0)}`,
    `Output occurrences: ${evidence.output.rows.length}`,
    "Sample facts:",
    ...factLines,
    "The host reloads canonical stored evidence. Redaction omits SQL/IR/Alloy, parameters, domain values and fact payloads.",
  ].join("\n");
}
