import type { JSX, ReactNode } from "react";
import { shortHash } from "../analysis/format";
import { activeDecisions, REVIEW_LABELS, type FeedbackEvent, type ReviewDisposition } from "../feedback/journal";
import type { FeedbackJournal } from "../feedback/useFeedbackJournal";
import { archiveKey, type useArchivedReview } from "../workspace/archivedReview";

interface ReviewBinsProps {
  readonly journal: FeedbackJournal;
  readonly archive: ReturnType<typeof useArchivedReview>;
  readonly revision: string | null;
  readonly children: ReactNode;
}

const bins: readonly ReviewDisposition[] = ["accepted_use_case", "unintended_behavior", "unsure"];
const descriptions: Readonly<Record<ReviewDisposition, string>> = {
  accepted_use_case: "Preserve this observed behavior.",
  unintended_behavior: "Exclude this observed behavior.",
  unsure: "Keep these cases for review without requiring a result.",
};

interface BinEntryProps extends Omit<ReviewBinsProps, "children"> {
  readonly event: FeedbackEvent;
}

/** Offer explicit case opening, bin changes and withdrawal for this exact active event. */
function BinActions({ event, archive, journal }: BinEntryProps): JSX.Element {
  const pending = archive.visible?.caseKey === archiveKey(event) && archive.visible.pending;
  return (
    <div className="review-bin-actions">
      <button type="button" className="text-button" disabled={!event.bundle_id || pending} onClick={() => void archive.open(event)}>
        {pending ? "Opening case…" : "Open case"}
      </button>
      {bins
        .filter((bin) => bin !== event.disposition)
        .map((bin) => (
          <button type="button" key={bin} className="text-button" onClick={() => journal.relabel(event.event_id, bin)}>
            Move to {REVIEW_LABELS[bin]}
          </button>
        ))}
      <button type="button" className="text-button" onClick={() => journal.withdraw([event.event_id])}>
        Withdraw
      </button>
    </div>
  );
}

/** Show exact saved identity, optional rationale and any archive or legacy issue. */
function BinEntry(props: BinEntryProps): JSX.Element {
  const { event, archive, revision } = props;
  const error = archive.errors[archiveKey(event)];
  return (
    <li>
      <p>
        <strong>Case {shortHash(event.instance_id)}</strong> · {event.bundle_id ? `Bundle ${shortHash(event.bundle_id)}` : "Bundle unknown"}
      </p>
      <p>
        Revision {shortHash(event.model_revision_sha256)}
        {revision && revision !== event.model_revision_sha256 ? " · Earlier revision" : ""}
      </p>
      {event.rationale ? <p>{event.rationale}</p> : null}
      {!event.bundle_id ? <p>Find the original case and save its label again to link this entry to its bundle.</p> : null}
      {error ? <p role="alert">Unable to open case. {error}</p> : null}
      <BinActions {...props} />
    </li>
  );
}

/** Render the active labels in one persisted bin, newest decision first. */
function ReviewBin(props: Omit<ReviewBinsProps, "children"> & { readonly bin: ReviewDisposition }): JSX.Element {
  const { bin, journal } = props;
  const events = activeDecisions(journal.events)
    .filter((event) => event.disposition === bin)
    .reverse();
  return (
    <section className={`review-bin review-bin--${bin}`} aria-label={REVIEW_LABELS[bin]}>
      <h3>
        {REVIEW_LABELS[bin]} <span className="review-bin-count">{events.length}</span>
      </h3>
      <p>{descriptions[bin]}</p>
      {events.length ? (
        <ul>
          {events.map((event) => (
            <BinEntry key={event.event_id} {...props} event={event} />
          ))}
        </ul>
      ) : (
        <p className="empty-note">No cases yet.</p>
      )}
    </section>
  );
}

/** Keep all prior decisions visible at the bottom of the workspace. */
export function ReviewBins({ children, ...props }: ReviewBinsProps): JSX.Element {
  return (
    <section className="review-bins" id="review-bins" aria-labelledby="review-bins-title">
      <div>
        <h2 id="review-bins-title">Review bins</h2>
        <p>Reopen a saved case or move it between bins. Regeneration requires at least one Yes or No.</p>
      </div>
      <div className="review-bin-grid">
        {bins.map((bin) => (
          <ReviewBin key={bin} bin={bin} {...props} />
        ))}
      </div>
      {children}
    </section>
  );
}
