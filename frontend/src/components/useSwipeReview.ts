/**
 * Horizontal swipe enhancement over the review card. Graph, table, controls and
 * any `[data-no-swipe]` surface are excluded so panning the graph can never
 * submit feedback. Buttons and keyboard actions remain the primary interface.
 */
import { useEffect, useRef, useState, type PointerEvent as ReactPointerEvent } from "react";

/** Inputs for the non-essential swipe enhancement. */
export interface SwipeReviewOptions {
  readonly disabled: boolean;
  readonly onAccept: () => void;
  readonly onReject: () => void;
}

/** Visible drag state plus the article pointer handlers. */
export interface SwipeReview {
  readonly dragX: number;
  readonly dragging: boolean;
  readonly handlers: {
    readonly onPointerDown: (event: ReactPointerEvent<HTMLElement>) => void;
    readonly onPointerMove: (event: ReactPointerEvent<HTMLElement>) => void;
    readonly onPointerUp: (event: ReactPointerEvent<HTMLElement>) => void;
    readonly onPointerCancel: () => void;
    readonly onLostPointerCapture: () => void;
  };
}

type Start = { readonly x: number; readonly y: number; readonly pointerId: number };
type SetNumber = (value: number) => void;
type SetBoolean = (value: boolean) => void;

const SWIPE_THRESHOLD = 72;

/** Check whether a target belongs to an interactive or excluded surface. */
function excludedTarget(target: EventTarget | null): boolean {
  return target instanceof Element ? Boolean(target.closest("[data-no-swipe], button, a, input, select, textarea, summary, label, [role='button']")) : true;
}

/** Begin tracking a potentially horizontal gesture. */
function startDrag(event: ReactPointerEvent<HTMLElement>, disabled: boolean, start: { current: Start | null }): void {
  if (disabled || start.current || event.button !== 0 || excludedTarget(event.target)) return;
  if (event.pointerType === "mouse") event.preventDefault();
  start.current = { x: event.clientX, y: event.clientY, pointerId: event.pointerId };
  event.currentTarget.setPointerCapture?.(event.pointerId);
}

/** Update the visual drag translation only after horizontal intent is clear. */
function moveDrag(event: ReactPointerEvent<HTMLElement>, start: { current: Start | null }, setDragging: SetBoolean, setDragX: SetNumber): void {
  if (!start.current || start.current.pointerId !== event.pointerId) return;
  const dx = event.clientX - start.current.x;
  const dy = event.clientY - start.current.y;
  if (Math.abs(dx) < 8 || Math.abs(dx) <= Math.abs(dy)) return;
  setDragging(true);
  setDragX(Math.max(-140, Math.min(140, dx)));
}

/** Finish a gesture and dispatch only a deliberate horizontal swipe. */
function endDrag(event: ReactPointerEvent<HTMLElement>, start: { current: Start | null }, reset: () => void, options: SwipeReviewOptions): void {
  const origin = start.current;
  if (!origin || origin.pointerId !== event.pointerId) return;
  reset();
  const dx = event.clientX - origin.x;
  const dy = event.clientY - origin.y;
  if (Math.abs(dx) < SWIPE_THRESHOLD || Math.abs(dx) <= Math.abs(dy)) return;
  if (dx > 0) options.onAccept();
  else options.onReject();
}

/** Supply horizontal drag detection while leaving vertical scrolling alone. */
export function useSwipeReview(options: SwipeReviewOptions): SwipeReview {
  const start = useRef<Start | null>(null);
  const [dragX, setDragX] = useState(0);
  const [dragging, setDragging] = useState(false);
  const reset = (): void => {
    start.current = null;
    setDragging(false);
    setDragX(0);
  };
  useEffect(() => {
    if (!options.disabled) return;
    reset();
  }, [options.disabled]);
  return {
    dragX,
    dragging,
    handlers: {
      onPointerDown: (event) => startDrag(event, options.disabled, start),
      onPointerMove: (event) => moveDrag(event, start, setDragging, setDragX),
      onPointerUp: (event) => endDrag(event, start, reset, options),
      onPointerCancel: reset,
      onLostPointerCapture: reset,
    },
  };
}
