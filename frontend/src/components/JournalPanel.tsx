/**
 * Browser-local feedback ledger with append-only history, conflict resolution,
 * stale-revision disclosure and plain JSON export/import.
 */
import { useRef, useState, type JSX, type RefObject } from "react";
import { shortHash } from "../analysis/format";
import type { FeedbackJournal } from "../feedback/useFeedbackJournal";
import { activeEvents, detectConflicts, staleDecisions, type FeedbackConflict, type FeedbackEvent } from "../feedback/journal";

interface JournalPanelProps {
  readonly journal: FeedbackJournal;
  readonly currentRevision: string | null;
  readonly onWithdraw: (eventIds: readonly string[]) => void;
}

const DISPOSITION_LABEL: Readonly<Record<FeedbackEvent["disposition"], string>> = {
  accepted_use_case: "Use case preserved",
  unintended_behavior: "Unintended behavior",
  unsure: "Unsure",
  withdrawn: "Withdrawn",
};

/** Trigger a local JSON download without a remote URL. */
function downloadText(filename: string, text: string): void {
  const blob = new Blob([text], { type: "application/json" });
  const url = URL.createObjectURL(blob);
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = filename;
  anchor.click();
  URL.revokeObjectURL(url);
}

interface JournalEventProps {
  readonly event: FeedbackEvent;
  readonly active: boolean;
}

/** Render one append-only event with active/superseded state. */
function JournalEvent({ event, active }: JournalEventProps): JSX.Element {
  return (
    <li className={active ? "journal-event is-active" : "journal-event"}>
      <div className="journal-event__top">
        <span className={`requirement-tag requirement-tag--${event.disposition}`}>{DISPOSITION_LABEL[event.disposition]}</span>
        <time dateTime={event.created_at}>{new Date(event.created_at).toLocaleString()}</time>
      </div>
      <p>
        Instance {shortHash(event.instance_id)} · revision {shortHash(event.model_revision_sha256)}
        {event.bundle_id ? ` · bundle ${shortHash(event.bundle_id)}` : " · bundle unknown"}
      </p>
      {event.rationale ? <p className="journal-rationale">“{event.rationale}”</p> : null}
      <code>{event.event_id}</code>
      {event.supersedes_event_id ? <span className="supersedes">supersedes {shortHash(event.supersedes_event_id)}</span> : null}
      {!active ? <span className="superseded-label">Superseded (history retained)</span> : null}
    </li>
  );
}

interface JournalEventListProps {
  readonly events: readonly FeedbackEvent[];
  readonly activeIds: ReadonlySet<string>;
}

/** Render the append-only event list newest first. */
function JournalEventList({ events, activeIds }: JournalEventListProps): JSX.Element {
  if (events.length === 0) return <p className="empty-note">No intent labels yet. Accept or reject a case to create the first event.</p>;
  return (
    <ol className="journal-list">
      {[...events].reverse().map((event) => (
        <JournalEvent key={event.event_id} event={event} active={activeIds.has(event.event_id)} />
      ))}
    </ol>
  );
}

interface ConflictActionsProps {
  readonly conflict: FeedbackConflict;
  readonly onWithdraw: (eventIds: readonly string[]) => void;
}

/** Render conflict resolution actions. */
function ConflictActions({ conflict, onWithdraw }: ConflictActionsProps): JSX.Element {
  return (
    <div className="conflict-actions">
      <button type="button" className="text-button" onClick={() => onWithdraw(conflict.rejected.map((event) => event.event_id))}>
        Keep accepted use case
      </button>
      <button type="button" className="text-button" onClick={() => onWithdraw(conflict.accepted.map((event) => event.event_id))}>
        Keep unintended behavior
      </button>
    </div>
  );
}

interface ConflictResolutionProps {
  readonly conflict: FeedbackConflict;
  readonly onWithdraw: (eventIds: readonly string[]) => void;
}

/** Render one contradictory-label resolution choice. */
function ConflictResolution({ conflict, onWithdraw }: ConflictResolutionProps): JSX.Element {
  return (
    <div className="inline-alert" role="alert">
      <strong>Contradictory labels for {shortHash(conflict.semantic)}</strong>
      <span>Both accepted and rejected intent are active. Choose which intent to keep; the other is withdrawn by a new event.</span>
      <ConflictActions conflict={conflict} onWithdraw={onWithdraw} />
    </div>
  );
}

interface JournalTransferControlsProps {
  readonly journal: FeedbackJournal;
  readonly fileInput: RefObject<HTMLInputElement | null>;
  readonly onFile: (file: File | undefined) => Promise<void>;
}

/** Render local import/export controls. */
function JournalTransferControls({ journal, fileInput, onFile }: JournalTransferControlsProps): JSX.Element {
  return (
    <div className="journal-actions">
      <button
        type="button"
        className="text-button"
        onClick={() => downloadText("sqlolla-feedback-journal.json", journal.exportText())}
        disabled={journal.events.length === 0}
      >
        Export JSON
      </button>
      <button type="button" className="text-button" onClick={() => fileInput.current?.click()}>
        Import JSON
      </button>
      <input
        ref={fileInput}
        className="sr-only"
        type="file"
        accept=".json,application/json"
        aria-label="Import feedback journal JSON"
        onChange={(event) => void onFile(event.target.files?.[0])}
      />
    </div>
  );
}

interface JournalTransferProps {
  readonly journal: FeedbackJournal;
  readonly fileInput: RefObject<HTMLInputElement | null>;
  readonly onFile: (file: File | undefined) => Promise<void>;
  readonly importError: string | null;
}

/** Render local import/export controls and their latest error. */
function JournalTransfer({ journal, fileInput, onFile, importError }: JournalTransferProps): JSX.Element {
  return (
    <>
      <JournalTransferControls journal={journal} fileInput={fileInput} onFile={onFile} />
      {importError ? (
        <div className="inline-alert inline-alert--danger" role="alert">
          <strong>Import rejected</strong>
          <span>{importError}</span>
        </div>
      ) : null}
    </>
  );
}

/** Manage the local import interaction and its visible error. */
function useJournalImport(journal: FeedbackJournal): {
  readonly fileInput: RefObject<HTMLInputElement | null>;
  readonly error: string | null;
  readonly onFile: (file: File | undefined) => Promise<void>;
} {
  const [error, setError] = useState<string | null>(null);
  const fileInput = useRef<HTMLInputElement>(null);
  const onFile = async (file: File | undefined): Promise<void> => {
    if (!file) return;
    setError(null);
    const result = journal.importText(await file.text());
    if (!result.ok) setError(result.message);
    if (fileInput.current) fileInput.current.value = "";
  };
  return { fileInput, error, onFile };
}

/** Render the journal storage error banner. */
function StorageError({ journal }: { readonly journal: FeedbackJournal }): JSX.Element | null {
  if (!journal.storageError) return null;
  return (
    <div className="inline-alert inline-alert--danger" role="alert">
      <strong>Journal store error</strong>
      <span>{journal.storageError}</span>
      <span>Export existing events; corrupted storage is not overwritten.</span>
      <button type="button" className="text-button" onClick={() => downloadText("sqlolla-feedback-journal.json", journal.exportText())}>
        Export journal
      </button>
    </div>
  );
}

/** Render the journal status notice. */
function JournalNotice({ journal }: { readonly journal: FeedbackJournal }): JSX.Element | null {
  if (!journal.notice) return null;
  return (
    <div className="inline-alert" role="status">
      <span>{journal.notice}</span>
      <button type="button" className="text-button" onClick={journal.clearNotice}>
        Dismiss
      </button>
    </div>
  );
}

interface JournalNoticesProps {
  readonly journal: FeedbackJournal;
  readonly onWithdraw: (eventIds: readonly string[]) => void;
}

/** Render the ledger storage/status notices. */
function JournalNotices({ journal, onWithdraw }: JournalNoticesProps): JSX.Element {
  const conflicts = detectConflicts(journal.events);
  return (
    <>
      <StorageError journal={journal} />
      <JournalNotice journal={journal} />
      {conflicts.map((conflict) => (
        <ConflictResolution key={`${conflict.revision}-${conflict.semantic}`} conflict={conflict} onWithdraw={onWithdraw} />
      ))}
    </>
  );
}

/** Render the ledger heading. */
function LedgerHeader(): JSX.Element {
  return (
    <div className="rail-heading">
      <span className="mono-label">REVIEW LEDGER</span>
      <h2>Intent, kept separate</h2>
      <p>Append-only events live in this browser. Evidence files are never edited to match feedback.</p>
    </div>
  );
}

/** Render a stale feedback warning. */
function StaleFeedback({ count }: { readonly count: number }): JSX.Element | null {
  if (count === 0) return null;
  return (
    <div className="inline-alert">
      <strong>Earlier revision feedback</strong>
      <span>{count} active label(s) come from another revision. Refinement checks whether their stored context still applies.</span>
    </div>
  );
}

/** Render the ledger, conflict controls, stale warnings and import/export. */
export function JournalPanel({ journal, currentRevision, onWithdraw }: JournalPanelProps): JSX.Element {
  const transfer = useJournalImport(journal);
  const activeIds = new Set(activeEvents(journal.events).map((event) => event.event_id));
  const stale = staleDecisions(journal.events, currentRevision);
  return (
    <section className="journal-panel" aria-label="Review ledger">
      <LedgerHeader />
      <JournalNotices journal={journal} onWithdraw={onWithdraw} />
      <StaleFeedback count={stale.length} />
      <JournalTransfer journal={journal} fileInput={transfer.fileInput} onFile={transfer.onFile} importError={transfer.error} />
      <p className="journal-note">Export contains journal events only. Provider keys are never included and are never written to browser storage.</p>
      <JournalEventList events={journal.events} activeIds={activeIds} />
    </section>
  );
}
