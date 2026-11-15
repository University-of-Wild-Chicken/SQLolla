/** Deterministic request construction; Rust remains the sole binder and Alloy compiler. */
import { parseJson } from "../analysis/json";
import type { EvidenceView } from "../analysis/types";
import type { DraftParameter, DraftState } from "./model";
import { draftErrors, renderDraftSql } from "./render";

/** Convert declared card parameters without rounding integer text or renumbering tokens. */
function parameterCell(parameter: DraftParameter): unknown {
  if (parameter.type === "null") return { type: "null", value: parameter.nullType };
  if (parameter.type === "text") return { type: "text", value: parameter.value };
  if (parameter.type === "bool") return { type: "bool", value: parameter.value.toLowerCase() === "true" };
  const value = BigInt(parameter.value);
  const bits = parameter.type === "int32" ? 32n : 64n;
  if (value < -(1n << (bits - 1n)) || value >= 1n << (bits - 1n)) throw new Error(`$${parameter.number} exceeds ${parameter.type}`);
  return { type: parameter.type, value: parseJson(value.toString()) };
}

/** Compile against the displayed input instance, or an explicitly empty bag for each declared table. */
export function draftRequest(draft: DraftState, evidence: EvidenceView | null): Record<string, unknown> {
  const failures = draftErrors(draft);
  if (failures.length) throw new Error(failures.join(" "));
  const database = Object.fromEntries(
    Object.keys(draft.catalog.tables).map((table) => [table, evidence?.instance.semantic.database[table]?.map((row) => row.map((cell) => cell.raw)) ?? []]),
  );
  return {
    id: "browser-card-draft",
    mode: "compile",
    sql: renderDraftSql(draft),
    catalog: draft.catalog.raw,
    parameters: Object.fromEntries(draft.parameters.map((parameter) => [String(parameter.number), parameterCell(parameter)])),
    database,
    include_model: true,
    ...(evidence?.bundle.request.execution ? { execution: evidence.bundle.request.execution } : {}),
  };
}
