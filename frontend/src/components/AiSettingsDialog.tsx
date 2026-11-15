/** Explicit BYOT consent with tab-memory credentials and a fixed, reviewed provider destination. */
import { useState, type JSX } from "react";
import { providers, type LiveProviderMode } from "../ai/config";
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
  readonly showDemo?: boolean;
}
interface FieldsProps {
  readonly draft: AiProviderSettings;
  readonly patch: (patch: Partial<AiProviderSettings>) => void;
}

/** Choosing a different provider clears the previous provider's key. */
function ProviderFields({ draft, patch, showDemo }: FieldsProps & Pick<Props, "showDemo">): JSX.Element {
  return (
    <fieldset className="provider-grid">
      <legend className="mono-label">PROVIDER</legend>
      {(Object.keys(providers) as LiveProviderMode[]).map((mode) => (
        <label className="provider-card" key={mode}>
          <input type="radio" name="provider" checked={draft.mode === mode} onChange={() => patch({ mode, apiKey: "" })} />
          <strong>{providers[mode].label}</strong>
        </label>
      ))}
      {showDemo ? (
        <label className="provider-card">
          <input type="radio" name="provider" checked={draft.mode === "offline_demo"} onChange={() => patch({ mode: "offline_demo", apiKey: "" })} />
          <strong>Offline demo</strong>
        </label>
      ) : null}
    </fieldset>
  );
}

/** Keep the credential in the form until saving or closing it. */
function CredentialFields({ draft, patch }: FieldsProps): JSX.Element {
  return (
    <div className="credential-fields">
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
        Your key stays in this tab until you disconnect or refresh. SQLolla sends it to the selected provider when you request assistance.
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
        Redacted explanations share schema shape, fact identifiers, counts, scope and verified rule outcomes. Your question is sent as written.
      </p>
      <p className="field-note">Saving settings sends nothing. Your provider’s account policies apply when you request assistance.</p>
    </div>
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
      <ProviderFields draft={draft} patch={patch} showDemo={props.showDemo} />
      {draft.mode !== "offline_demo" ? <CredentialFields draft={draft} patch={patch} /> : null}
      <SharingFields draft={draft} patch={patch} />
      <details>
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
    <Modal open={props.open} onClose={props.onClose} title="AI provider" eyebrow="BRING YOUR OWN TOKEN" className="settings-dialog">
      {props.open ? <Body {...props} /> : null}
    </Modal>
  );
}
