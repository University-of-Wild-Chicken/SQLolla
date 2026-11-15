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
  readonly onUnsure: () => void;
  readonly onUndo: () => void;
  readonly onNext: () => void;
}

/** Check whether focus is in an editable control or an open dialog. */
function keyboardBlocked(target: EventTarget | null): boolean {
  if (typeof document !== "undefined" && document.querySelector("dialog[open]")) return true;
  if (target instanceof HTMLElement) {
    if (target.isContentEditable) return true;
    if (["INPUT", "TEXTAREA", "SELECT"].includes(target.tagName)) return true;
    if (target.getAttribute("role") === "textbox" || target.closest(".review-bins")) return true;
  }
  return false;
}

/** Translate shortcuts to one of the current card's actions. */
function keyboardAction(event: KeyboardEvent, options: ReviewKeyboardOptions): (() => void) | undefined {
  if (keyboardBlocked(event.target) || event.altKey) return undefined;
  const key = event.key.toLowerCase();
  if (event.ctrlKey || event.metaKey) return key === "z" ? options.onUndo : undefined;
  return { a: options.onAccept, r: options.onReject, u: options.onUnsure, n: options.onNext }[key];
}

/** Install document-level review shortcuts while the card is active. */
export function useReviewKeyboard(options: ReviewKeyboardOptions): void {
  const latest = useRef(options);
  latest.current = options;
  useEffect(() => {
    if (!options.enabled) return undefined;
    const listener = (event: KeyboardEvent): void => {
      const action = keyboardAction(event, latest.current);
      if (!action) return;
      event.preventDefault();
      action();
    };
    document.addEventListener("keydown", listener);
    return () => document.removeEventListener("keydown", listener);
  }, [options.enabled]);
}
