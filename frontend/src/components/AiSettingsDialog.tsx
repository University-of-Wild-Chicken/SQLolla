/** Explicit BYOT consent with tab-memory credentials and a fixed, reviewed provider destination. */
import { useState, type JSX } from "react";
import { AI_ENDPOINT, AI_MODEL } from "../ai/config";
import { buildContextPreview, type AiProviderSettings } from "../ai/summary";
import type { EvidenceView } from "../analysis/types";
import { Modal } from "./Modal";

interface Props {
  readonly open: boolean;
  readonly onClose: () => void;
  readonly settings: AiProviderSettings;
  readonly evidence: EvidenceView | null;
  readonly onSave: (settings: AiProviderSettings) => void;
  readonly onDisconnect: () => void;
  readonly fixedProvider?: boolean;
}
interface FieldsProps {
  readonly draft: AiProviderSettings;
  readonly patch: (patch: Partial<AiProviderSettings>) => void;
}

/** Only the implemented fixed OpenAI target can receive live requests. */
function ProviderFields({ draft, patch }: FieldsProps): JSX.Element {
  return (
    <fieldset className="provider-grid">
      <legend className="mono-label">PROVIDER</legend>
      <label className="provider-card">
        <input type="radio" name="provider" checked={draft.mode === "offline_demo"} onChange={() => patch({ mode: "offline_demo", apiKey: "" })} />
        <strong>Offline demo</strong>
        <span>No network request.</span>
      </label>
      <label className="provider-card">
        <input type="radio" name="provider" checked={draft.mode === "openai_compatible"} onChange={() => patch({ mode: "openai_compatible" })} />
        <strong>OpenAI-compatible</strong>
        <span>OpenAI Responses · {AI_MODEL}</span>
      </label>
      <p className="field-note">Anthropic and Gemini are not connected in this host. No provider or model fallback is used.</p>
    </fieldset>
  );
}

/** Show the immutable destination beside the credential; never offer an arbitrary proxy endpoint. */
function CredentialFields({ draft, patch }: FieldsProps): JSX.Element {
  return (
    <div className="credential-fields">
      <label>
        Model identifier
        <input readOnly value={AI_MODEL} />
      </label>
      <label>
        Provider destination
        <input readOnly value={AI_ENDPOINT} />
      </label>
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
        Your key goes to the local host for each requested action and then to OpenAI. It is not saved in evidence, browser storage, URLs or exports.
      </p>
    </div>
  );
}

/** Redacted explanation mode does not expose raw source or scalar payloads. */
function SharingFields({ draft, patch }: FieldsProps): JSX.Element {
  return (
    <div className="settings-checks">
      <label>
        <input type="checkbox" checked={draft.redactValues} onChange={(event) => patch({ redactValues: event.target.checked })} />
        Redact SQL and values before sending explanation context
      </label>
      <p className="field-note">
        Redacted explanations contain schema shape, fact identifiers, counts and analysis scope. Regeneration uses unredacted SQL and saved review cases.
      </p>
      <p className="field-note">OpenAI requests use store:false. Provider account policies still apply. Saving settings sends nothing.</p>
    </div>
  );
}

/** Disconnect removes the in-memory credential; saving cannot trigger a provider call. */
function Actions({
  draft,
  onSave,
  onClose,
  onDisconnect,
}: Omit<Props, "open" | "settings" | "evidence"> & { readonly draft: AiProviderSettings }): JSX.Element {
  const valid = draft.mode === "offline_demo" || draft.apiKey.trim().length > 0;
  const save = () => {
    onSave({ ...draft, model: AI_MODEL, endpoint: AI_ENDPOINT, apiKey: draft.apiKey.trim(), preparedInMemory: draft.mode === "openai_compatible" });
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
  const [draft, setDraft] = useState<AiProviderSettings>(() => (props.fixedProvider ? { ...props.settings, mode: "openai_compatible" } : props.settings));
  const patch = (next: Partial<AiProviderSettings>) => setDraft((current) => ({ ...current, ...next }));
  return (
    <div className="settings-body">
      {props.fixedProvider ? <p>OpenAI · {AI_MODEL}</p> : <ProviderFields draft={draft} patch={patch} />}
      {draft.mode === "openai_compatible" ? <CredentialFields draft={draft} patch={patch} /> : null}
      <SharingFields draft={draft} patch={patch} />
      <details>
        <summary>Preview explanation context</summary>
        <pre className="code-block preview-block">
          {props.evidence ? buildContextPreview(props.evidence, draft) : "Open a stored case to preview its context."}
        </pre>
      </details>
      <Actions draft={draft} onClose={props.onClose} onSave={props.onSave} onDisconnect={props.onDisconnect} />
    </div>
  );
}

/** Render a memory-only BYOT form; live requests remain separate deliberate actions. */
export function AiSettingsDialog(props: Props): JSX.Element {
  return (
    <Modal open={props.open} onClose={props.onClose} title="Explanation provider" eyebrow="BRING YOUR OWN TOKEN" className="settings-dialog">
      {props.open ? <Body {...props} /> : null}
    </Modal>
  );
}
