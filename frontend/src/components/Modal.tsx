/**
 * Native modal dialog boundary with a labelled heading and cancel handling.
 * Focus containment and Escape semantics are provided by the platform dialog.
 */
import { useEffect, useId, useRef, type JSX, type ReactNode, type RefObject } from "react";

/** Props for a focus-managed native dialog. */
export interface ModalProps {
  readonly open: boolean;
  readonly onClose: () => void;
  readonly title: string;
  readonly eyebrow?: string;
  readonly children: ReactNode;
  readonly className?: string;
}

/** Mount an open native `<dialog>` and close it through the supplied callback. */
export function Modal({ open, ...props }: ModalProps): JSX.Element | null {
  return open ? <ModalContent {...props} /> : null;
}

/** Open the native modal once after mount, with a test-friendly fallback. */
function useShowModal(ref: RefObject<HTMLDialogElement | null>): void {
  useEffect(() => {
    const dialog = ref.current;
    if (!dialog || dialog.open) return;
    try {
      dialog.showModal();
    } catch {
      dialog.setAttribute("open", "");
    }
  }, [ref]);
}

interface DialogHeaderProps {
  readonly title: string;
  readonly eyebrow?: string;
  readonly titleId: string;
  readonly onClose: () => void;
}

/** Render the labelled dialog header with a close control. */
function DialogHeader({ title, eyebrow, titleId, onClose }: DialogHeaderProps): JSX.Element {
  return (
    <header className="dialog-header">
      <div>
        {eyebrow ? <span className="mono-label">{eyebrow}</span> : null}
        <h2 id={titleId}>{title}</h2>
      </div>
      <button type="button" className="icon-button" onClick={onClose} aria-label={`Close ${title}`}>
        ×
      </button>
    </header>
  );
}

/** Internal mounted dialog; separated so effects run only while visible. */
function ModalContent({ onClose, title, eyebrow, children, className = "" }: Omit<ModalProps, "open">): JSX.Element {
  const ref = useRef<HTMLDialogElement>(null);
  const titleId = useId();
  useShowModal(ref);
  return (
    <dialog
      ref={ref}
      className={`app-dialog ${className}`}
      aria-labelledby={titleId}
      onCancel={(event) => {
        event.preventDefault();
        onClose();
      }}
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
    >
      <div className="dialog-surface">
        <DialogHeader title={title} eyebrow={eyebrow} titleId={titleId} onClose={onClose} />
        {children}
      </div>
    </dialog>
  );
}
