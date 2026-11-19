/** An explicit first-draft request fits before the workbench's evidence and feedback loop. */
import { useEffect, useMemo, useRef, useState, type JSX } from "react";
import { requestDraft, type DraftMode, type SqlDraft } from "../ai/draft";
import type { AiProviderSettings } from "../ai/summary";
import { parseCatalog } from "../analysis/adapter";
import { stringifyJson } from "../analysis/json";
import { AiTargetNote } from "./AiAssistant";
import { CopySqlButton } from "./CopySqlButton";

interface Props {
  readonly requestText: string;
  readonly validCatalog: boolean;
  readonly settings: AiProviderSettings;
  readonly initialPrompt: string;
  readonly used: ReadonlySet<string>;
  readonly markProposed: (prompt: string) => void;
  readonly openSettings: () => void;
  readonly stage: (draft: SqlDraft, prompt: string) => void;
}

type Response = { identity: object; draft?: SqlDraft; error?: string; busy?: boolean };

/** Discard late results after editing the prompt, context, credential or disclosure. */
function useDraftRequest(props: Props, prompt: string, share: boolean, mode: DraftMode) {
  const identity = useMemo(() => ({}), [props.requestText, props.settings, prompt, share, mode]);
  const latest = useRef(identity);
  latest.current = identity;
  const pending = useRef<AbortController | null>(null);
  const [response, setResponse] = useState<Response | null>(null);
  useEffect(() => () => pending.current?.abort(), [identity]);
  const generate = async (): Promise<void> => {
    if (pending.current || !share || (mode === "query" && !props.validCatalog) || !prompt.trim() || props.used.has(prompt.trim())) return;
    const controller = new AbortController();
    pending.current = controller;
    setResponse({ identity, busy: true });
    try {
      const draft = await requestDraft(mode, props.requestText, prompt, props.settings, controller.signal);
      if (latest.current === identity && !controller.signal.aborted) {
        if (!draft.refusal) props.markProposed(prompt.trim());
        setResponse({ identity, draft });
      }
    } catch (error) {
      if (latest.current === identity && !controller.signal.aborted)
        setResponse({ identity, error: error instanceof Error ? error.message : "The draft request failed." });
    } finally {
      if (pending.current === controller) pending.current = null;
    }
  };
  return { current: response?.identity === identity ? response : null, generate };
}

interface GenerateProps {
  readonly props: Props;
  readonly fields: ReturnType<typeof useDraftFields>;
  readonly request: ReturnType<typeof useDraftRequest>;
  readonly used: boolean;
}

/** Offer generation only with a key, a prompt and consent; otherwise point to provider setup. */
function GenerateAction({ props, fields: { prompt, share, mode }, request: { current, generate }, used }: GenerateProps): JSX.Element {
  if (!props.settings.apiKey.trim()) {
    return (
      <button type="button" className="ghost-button" onClick={props.openSettings}>
        Connect AI provider…
      </button>
    );
  }
  return (
    <button
      type="button"
      className="primary-button"
      disabled={!prompt.trim() || !share || current?.busy || used || (mode === "query" && !props.validCatalog)}
      onClick={() => void generate()}
    >
      {current?.busy ? "Building draft…" : mode === "query" ? "Generate SQL draft" : "Generate database model"}
    </button>
  );
}

/** Require schema-sharing consent and an explicit draft request. */
export function AiDraftPanel(props: Props): JSX.Element {
  const fields = useDraftFields(props);
  const { prompt, share, mode } = fields;
  const request = useDraftRequest(props, prompt, share, mode);
  const { current } = request;
  const used = props.used.has(prompt.trim());
  return (
    <div className="builder-panel ai-draft">
      <p className="ai-purpose">
        Describe what you need in plain words. AI drafts it, SQLolla compiles the draft locally, and nothing replaces your query until you choose it.
      </p>
      <DraftFields fields={fields} validCatalog={props.validCatalog} />
      <AiTargetNote settings={props.settings} />
      <GenerateAction props={props} fields={fields} request={request} used={used} />
      {used && !current?.draft ? <p>Use the review bins to refine this prompt after analysis.</p> : null}
      {current?.error ? <p role="alert">{current.error}</p> : null}
      {current?.draft ? <DraftResult draft={current.draft} stage={() => props.stage(current.draft!, prompt)} /> : null}
    </div>
  );
}

/** Changing workflow invalidates an earlier sharing decision. */
function useDraftFields(props: Props) {
  const [prompt, setPrompt] = useState(props.initialPrompt);
  const [sharedWith, setSharedWith] = useState<AiProviderSettings["mode"] | null>(null);
  const share = sharedWith === props.settings.mode;
  const setShare = (consent: boolean): void => setSharedWith(consent ? props.settings.mode : null);
  const [mode, changeMode] = useState<DraftMode>(props.validCatalog ? "query" : "model");
  const setMode = (next: DraftMode): void => {
    changeMode(next);
    setShare(false);
  };
  return { prompt, setPrompt, share, setShare, mode, setMode };
}

/** Explain what will be shared for the selected request. */
function DraftFields({ fields, validCatalog }: { readonly fields: ReturnType<typeof useDraftFields>; readonly validCatalog: boolean }): JSX.Element {
  const { mode, setMode, prompt, setPrompt, share, setShare } = fields;
  return (
    <>
      <label>
        AI task
        <select aria-label="AI task" value={mode} onChange={(event) => setMode(event.target.value as DraftMode)}>
          <option value="model">Build a database model from a description</option>
          <option value="query" disabled={!validCatalog}>
            Draft SQL for my current schema
          </option>
        </select>
      </label>
      <label className="field-label">
        {mode === "model" ? "Describe the database and what you want to check" : "What should the query return?"}
        <textarea rows={4} maxLength={4096} value={prompt} onChange={(event) => setPrompt(event.target.value)} />
      </label>
      <label className="check-row">
        <input type="checkbox" checked={share} onChange={(event) => setShare(event.target.checked)} />
        {mode === "query" ? "Share my prompt, schema and parameter types with my AI provider." : "Share my description with my AI provider."}
      </label>
    </>
  );
}

/** Stage exactly the compiled statement on explicit selection; Analyze remains a separate action. */
function DraftResult({ draft, stage }: { readonly draft: SqlDraft; readonly stage: () => void }): JSX.Element {
  return (
    <div className="builder-section">
      <h3>Proposed SQL</h3>
      <pre className="code-block">{draft.sql}</pre>
      <CopySqlButton sql={draft.sql} />
      {draft.modelRequest ? <ModelPreview request={draft.modelRequest} /> : null}
      {draft.assumptions.length ? (
        <ul>
          {draft.assumptions.map((text, index) => (
            <li key={index}>{text}</li>
          ))}
        </ul>
      ) : null}
      <DraftAction draft={draft} stage={stage} />
    </div>
  );
}

/** Summarize the independently admitted catalog before the user selects it. */
function ModelPreview({ request }: { readonly request: Record<string, unknown> }): JSX.Element {
  const tables = Object.values(parseCatalog(request.catalog, "proposed catalog").tables);
  return (
    <>
      <h3>Proposed tables</h3>
      <ul>
        {tables.map((table) => (
          <li key={table.key}>
            {table.name} ({table.columns.map((column) => column.name).join(", ")})
          </li>
        ))}
      </ul>
      <details>
        <summary>Proposed schema, values and rule</summary>
        <pre className="code-block">{stringifyJson(request, 2)}</pre>
      </details>
    </>
  );
}

/** Only a compiled proposal gets a stage action. */
function DraftAction({ draft, stage }: { readonly draft: SqlDraft; readonly stage: () => void }): JSX.Element {
  if (draft.refusal) return <p role="alert">{draft.refusal}</p>;
  return (
    <>
      <p>Compiled locally. Review the values and rule before analysis.</p>
      <button type="button" className="primary-button" onClick={stage}>
        {draft.modelRequest ? "Use this model and query" : "Use this SQL draft"}
      </button>
    </>
  );
}
