/**
 * Keyboard equivalent for review actions: A accepts, R rejects, Ctrl/Cmd+Z
 * undoes, and N requests the next stored case. Typing and open dialogs are
 * always excluded.
 */
import { useEffect, useRef } from "react";

/** Keyboard hooks mirroring the labelled review controls. */
export interface ReviewKeyboardOptions {
  readonly enabled: boolean;
  readonly onAccept: () => void;
  readonly onReject: () => void;
  readonly onUndo: () => void;
  readonly onNext: () => void;
}

/** Check whether focus is in an editable control or an open dialog. */
function keyboardBlocked(target: EventTarget | null): boolean {
  if (typeof document !== "undefined" && document.querySelector("dialog[open]")) return true;
  if (target instanceof HTMLElement) {
    if (target.isContentEditable) return true;
    if (["INPUT", "TEXTAREA", "SELECT"].includes(target.tagName)) return true;
    if (target.getAttribute("role") === "textbox") return true;
  }
  return false;
}

/** Install document-level review shortcuts while the card is active. */
export function useReviewKeyboard(options: ReviewKeyboardOptions): void {
  const latest = useRef(options);
  latest.current = options;
  useEffect(() => {
    if (!options.enabled) return undefined;
    const listener = (event: KeyboardEvent): void => {
      if (keyboardBlocked(event.target) || event.altKey) return;
      const key = event.key.toLowerCase();
      if ((event.ctrlKey || event.metaKey) && key === "z") {
        event.preventDefault();
        latest.current.onUndo();
        return;
      }
      if (event.ctrlKey || event.metaKey) return;
      if (key === "a") {
        event.preventDefault();
        latest.current.onAccept();
      } else if (key === "r") {
        event.preventDefault();
        latest.current.onReject();
      } else if (key === "n") {
        event.preventDefault();
        latest.current.onNext();
      }
    };
    document.addEventListener("keydown", listener);
    return () => document.removeEventListener("keydown", listener);
  }, [options.enabled]);
}
