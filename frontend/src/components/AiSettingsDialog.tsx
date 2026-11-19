/**
 * Explicit BYOT setup: provider, memory-only key, model choice and disclosure. Saving sends
 * nothing; checking models sends only the key; AI actions remain separate deliberate requests.
 */
import { useId, useState, type JSX } from "react";
import { providers, selectedModel, type LiveProviderMode } from "../ai/config";
import type { ModelCatalog } from "../ai/models";
import { buildContextPreview, type AiProviderSettings } from "../ai/summary";
import { useModelCatalog } from "../ai/useModelCatalog";
import type { EvidenceView } from "../analysis/types";
import { Modal } from "./Modal";

interface Props {
  readonly open: boolean;
  readonly onClose: () => void;
  readonly settings: AiProviderSettings;
  readonly evidence: EvidenceView | null;
  readonly onSave: (settings: AiProviderSettings) => void;
  readonly onDisconnect: () => void;
  readonly showDemo?: boolean;
}
interface FieldsProps {
  readonly draft: AiProviderSettings;
  readonly patch: (patch: Partial<AiProviderSettings>) => void;
}

/** Choosing a different provider clears the previous key and returns to its tested model. */
function ProviderFields({ draft, patch, showDemo }: FieldsProps & Pick<Props, "showDemo">): JSX.Element {
  const choose = (mode: AiProviderSettings["mode"]) => patch({ mode, apiKey: "", model: "" });
  return (
    <fieldset className="settings-group provider-grid">
      <legend>Provider</legend>
      {(Object.keys(providers) as LiveProviderMode[]).map((mode) => (
        <label className="provider-card" key={mode}>
          <input type="radio" name="provider" checked={draft.mode === mode} onChange={() => choose(mode)} />
          <strong>{providers[mode].label}</strong>
        </label>
      ))}
      {showDemo ? (
        <label className="provider-card">
          <input type="radio" name="provider" checked={draft.mode === "offline_demo"} onChange={() => choose("offline_demo")} />
          <strong>Offline demo</strong>
        </label>
      ) : null}
    </fieldset>
  );
}

/** Keep the credential in the form until saving or closing it. */
function CredentialFields({ draft, patch }: FieldsProps): JSX.Element {
  const label = draft.mode === "offline_demo" ? "your provider" : providers[draft.mode].label;
  return (
    <fieldset className="settings-group credential-fields">
      <legend>API key</legend>
      <label>
        API key (memory only)
        <input
          type="password"
          value={draft.apiKey}
          autoComplete="new-password"
          spellCheck={false}
          onChange={(event) => patch({ apiKey: event.target.value })}
          placeholder="Held until disconnect or refresh"
        />
      </label>
      <p className="field-note">
        Stays in this tab until you disconnect or refresh. The local SQLolla host forwards it only to {label}, only when you check models or press an AI button.
      </p>
    </fieldset>
  );
}

/** Offer the tested default first, the current choice, then whatever the key's live listing reports. */
function modelOptions(recommended: string, selected: string, catalog: ModelCatalog | undefined): readonly string[] {
  return [...new Set([recommended, selected, ...(catalog?.models ?? [])])];
}

/** Summarize the latest explicit model check without implying the models were tested. */
function catalogStatus(current: ReturnType<typeof useModelCatalog>["current"], recommended: string): string {
  if (!current) return "Not checked yet. The recommended model is always available to choose.";
  if (current.busy) return "Checking models for this key…";
  if (current.error) return current.error;
  const { models } = current.catalog!;
  const missing = models.includes(recommended) ? "" : " The recommended model is not listed for this key.";
  return `${models.length} text model${models.length === 1 ? "" : "s"} available to this key.${missing}`;
}

/** Choose a model explicitly; untested choices are allowed but labelled, and never silently replaced. */
function ModelFields({ draft, patch, mode }: FieldsProps & { readonly mode: LiveProviderMode }): JSX.Element {
  const { label, model: recommended } = providers[mode];
  const { current, check } = useModelCatalog(draft);
  const selected = selectedModel(draft);
  const selectId = useId();
  return (
    <fieldset className="settings-group model-fields">
      <legend>Model</legend>
      <label htmlFor={selectId}>Model</label>
      <select id={selectId} value={selected} onChange={(event) => patch({ model: event.target.value })}>
        {modelOptions(recommended, selected, current?.catalog).map((model) => (
          <option key={model} value={model}>
            {model === recommended ? `${model} — recommended` : model}
          </option>
        ))}
      </select>
      <div className="settings-row">
        <button type="button" className="ghost-button" disabled={!draft.apiKey.trim() || !!current?.busy} onClick={() => void check()}>
          Check models for this key
        </button>
        <span role="status">{catalogStatus(current, recommended)}</span>
      </div>
      <p className="field-note">
        SQLolla is tested with the recommended model. Other models must return structured JSON; if {label} rejects one, you see an error and your evidence is
        unchanged. Checking models sends only your key to {label}.
      </p>
    </fieldset>
  );
}

/** List what each AI action shares so disclosure is understood before any request. */
function DisclosureTable({ redacted }: { readonly redacted: boolean }): JSX.Element {
  const rows = [
    ["Draft a database model", "Your description"],
    ["Draft SQL", "Your request, table schema and parameter types"],
    [
      "Explain a result",
      redacted ? "Your question, schema shape, fact IDs, counts and the rule outcome" : "Your question and the case’s SQL, IR, Alloy and values",
    ],
    ["Regenerate SQL", "Your prompt, the canonical SQL and your labeled cases with notes (never redacted)"],
  ];
  return (
    <table className="disclosure-table">
      <caption>What each AI action sends to your provider</caption>
      <tbody>
        {rows.map(([action, sent]) => (
          <tr key={action}>
            <th scope="row">{action}</th>
            <td>{sent}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

/** Redacted explanation mode does not expose raw source or scalar payloads. */
function SharingFields({ draft, patch }: FieldsProps): JSX.Element {
  return (
    <fieldset className="settings-group settings-checks">
      <legend>Privacy</legend>
      <label>
        <input type="checkbox" checked={draft.redactValues} onChange={(event) => patch({ redactValues: event.target.checked })} />
        Redact SQL and values before sending explanation context
      </label>
      <DisclosureTable redacted={draft.redactValues} />
      <p className="field-note">Saving settings sends nothing. Your provider’s account policies apply when you use an AI action.</p>
    </fieldset>
  );
}

type ActionProps = Pick<Props, "onSave" | "onClose" | "onDisconnect" | "showDemo"> & { readonly draft: AiProviderSettings };

/** Disconnect removes the in-memory credential; saving cannot trigger a provider call. */
function Actions({ draft, onSave, onClose, onDisconnect, showDemo }: ActionProps): JSX.Element {
  const valid = draft.mode === "offline_demo" ? showDemo : draft.apiKey.trim().length > 0;
  const save = () => {
    onSave({ ...draft, apiKey: draft.apiKey.trim() });
    onClose();
  };
  const disconnect = () => {
    onDisconnect();
    onClose();
  };
  return (
    <div className="dialog-actions">
      <button type="button" className="ghost-button" onClick={disconnect}>
        Disconnect & clear
      </button>
      <span className="spacer" />
      <button type="button" className="ghost-button" onClick={onClose}>
        Cancel
      </button>
      <button type="button" className="primary-button" disabled={!valid} onClick={save}>
        Save in this tab · no send
      </button>
    </div>
  );
}

/** The draft credential lives only in this mounted form and the tab's settings state. */
function Body(props: Omit<Props, "open">): JSX.Element {
  const [draft, setDraft] = useState<AiProviderSettings>(props.settings);
  const patch = (next: Partial<AiProviderSettings>) => setDraft((current) => ({ ...current, ...next }));
  return (
    <div className="settings-body">
      <p className="settings-intro">
        AI drafts queries, explains results and proposes fixes. <strong>AI proposes; SQLolla decides:</strong> the local compiler and solver check every
        proposal, and AI text stays labelled as untrusted.
      </p>
      <ProviderFields draft={draft} patch={patch} showDemo={props.showDemo} />
      {draft.mode !== "offline_demo" ? (
        <>
          <CredentialFields draft={draft} patch={patch} />
          <ModelFields draft={draft} patch={patch} mode={draft.mode} />
        </>
      ) : (
        <p className="field-note">Choose a provider above to enter its key and pick a model.</p>
      )}
      <SharingFields draft={draft} patch={patch} />
      <details className="settings-preview">
        <summary>Preview explanation context</summary>
        <pre className="code-block preview-block">
          {props.evidence ? buildContextPreview(props.evidence, draft) : "Open a stored case to preview its context."}
        </pre>
      </details>
      <Actions draft={draft} onClose={props.onClose} onSave={props.onSave} onDisconnect={props.onDisconnect} showDemo={props.showDemo} />
    </div>
  );
}

/** Render a memory-only BYOT form; live requests remain separate deliberate actions. */
export function AiSettingsDialog(props: Props): JSX.Element {
  return (
    <Modal open={props.open} onClose={props.onClose} title="Connect an AI provider" eyebrow="BRING YOUR OWN KEY" className="settings-dialog">
      {props.open ? <Body {...props} /> : null}
    </Modal>
  );
}
