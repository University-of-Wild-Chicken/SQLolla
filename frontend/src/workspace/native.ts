/** Editable native requests stay separate from immutable evidence and explicit start operations. */
import { useRef, useState } from "react";
import { asRecord } from "../analysis/guards";
import { parseJson, stringifyJson } from "../analysis/json";
import { EMPTY_REQUEST } from "../analysis/starter";
import type { AnalysisSession } from "../analysis/session";
import type { FixtureScenario } from "../analysis/types";

/** Native editor state and explicit operations, without credentials or automatic provider calls. */
export interface NativeAnalysis {
  readonly text: string;
  readonly sql: string;
  readonly dirty: boolean;
  readonly pending: boolean;
  readonly error: string | null;
  readonly kind: "example" | "counterexample";
  readonly maxInstances: number;
  readonly editText: (text: string) => void;
  readonly editSql: (sql: string, context?: unknown) => void;
  readonly setKind: (kind: "example" | "counterexample") => void;
  readonly setMaxInstances: (limit: number) => void;
  readonly start: () => Promise<boolean>;
  readonly stage: (request: unknown) => void;
}

/** Read the exact context even before a user supplies SQL. */
export function editableRequest(text: string): Record<string, unknown> {
  return asRecord(parseJson(text), "analysis request");
}

/** Reject malformed requests and missing SQL before analysis. */
export function requestObject(text: string): Record<string, unknown> {
  const request = editableRequest(text);
  if (typeof request["sql"] !== "string" || !request["sql"].trim()) throw new Error("Enter a SQL query.");
  return request;
}

/** Display the SQL editor even when the advanced JSON currently has an incomplete edit. */
function requestSql(text: string): string {
  try {
    const sql = editableRequest(text)["sql"];
    return typeof sql === "string" ? sql : "";
  } catch {
    return "";
  }
}

/** Manage request text revisions so a late result cannot validate a newer draft. */
function useEditor() {
  const [text, setText] = useState(() => stringifyJson(EMPTY_REQUEST, 2));
  const [dirty, setDirty] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const revision = useRef(0);
  const touch = (): void => {
    ++revision.current;
    setDirty(true);
    setError(null);
  };
  const editText = (value: string): void => {
    touch();
    setText(value);
  };
  const stage = (request: unknown): void => editText(stringifyJson(request, 2));
  const editSql = (sql: string, context?: unknown): void => {
    try {
      stage({ ...asRecord(context ?? parseJson(text), "request context"), sql });
    } catch (failure) {
      setError(failure instanceof Error ? failure.message : "Invalid request context");
    }
  };
  return { text, dirty, error, revision, editText, editSql, stage, setDirty, setError, touch };
}

/** Explicitly start one bounded analysis; request edits during the operation remain unvalidated drafts. */
function useStarter(session: AnalysisSession, editor: ReturnType<typeof useEditor>, kind: NativeAnalysis["kind"], maxInstances: number) {
  const [pending, setPending] = useState(false);
  const inFlight = useRef(false);
  const start = async (): Promise<boolean> => {
    if (inFlight.current) return false;
    inFlight.current = true;
    setPending(true);
    editor.setError(null);
    const revision = editor.revision.current;
    try {
      if (!Number.isSafeInteger(maxInstances) || maxInstances < 1 || maxInstances > 256) throw new Error("Choose 1–256 review cases.");
      const success = await session.startRequest({ action: "instances.start", request: requestObject(editor.text), kind, max_instances: maxInstances });
      if (success && revision === editor.revision.current) editor.setDirty(false);
      return success;
    } catch (failure) {
      editor.setError(failure instanceof Error ? failure.message : "Analysis could not start");
      return false;
    } finally {
      inFlight.current = false;
      setPending(false);
    }
  };
  return { pending, start };
}

/** Compose editable input and explicit operations; control edits invalidate the prior revision too. */
export function useNativeAnalysis(session: AnalysisSession): NativeAnalysis {
  const editor = useEditor();
  const [kind, setKind] = useState<NativeAnalysis["kind"]>("counterexample");
  const [maxInstances, setMaxInstances] = useState(8);
  const starter = useStarter(session, editor, kind, maxInstances);
  return {
    ...editor,
    ...starter,
    sql: requestSql(editor.text),
    kind,
    maxInstances,
    setKind: (value) => {
      editor.touch();
      setKind(value);
    },
    setMaxInstances: (value) => {
      editor.touch();
      setMaxInstances(value);
    },
  };
}

/** Label real persisted evidence without inheriting any illustrative fixture summary or provenance. */
export function nativeScenario(session: AnalysisSession, fallback: FixtureScenario): FixtureScenario {
  if (!session.state.native || session.state.scenarioId !== fallback.id) return fallback;
  return {
    ...fallback,
    title: fallback.id === "native" ? fallback.title : "Your local SQL analysis",
    shortTitle: "Local analysis",
    description: "Generated by the local Rust and Alloy pipeline.",
    statusLabel: "LOCAL ANALYSIS",
    bundleId: session.state.bundleId,
    summarySeed: null,
    provenance: { label: "Local solver evidence", source: "Local SQLolla host", note: "Persisted backend evidence reopened and replayed before review." },
    builderCatalog: session.state.bundle?.request.catalog ?? fallback.builderCatalog,
    openPayload: session.state.openRaw,
    nextByCursor: {},
    transportFailure: null,
  };
}
