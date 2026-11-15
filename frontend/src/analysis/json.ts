/** One lossless JSON boundary for backend evidence, editable requests and local downloads. */
import { isLosslessNumber, LosslessNumber, parse, stringify } from "lossless-json";

const sources = new WeakMap<object, string>();

/** Keep small integer metadata ergonomic while preserving every other numeric source lexeme. */
function parseNumber(source: string): number | LosslessNumber {
  const number = Number(source);
  return /^-?(0|[1-9]\d*)$/.test(source) && Number.isSafeInteger(number) ? number : new LosslessNumber(source);
}

/** Parse exact UTF-8 response/request text and retain its untouched source for inspection. */
export function parseJson(source: string): unknown {
  const value = parse(source, undefined, { parseNumber });
  if (typeof value === "object" && value !== null) sources.set(value, source);
  return value;
}

/** Serialize lossless numeric cells as JSON numbers, never quoted numbers or rounded doubles. */
export function stringifyJson(value: unknown, space?: number): string {
  return stringify(value, undefined, space) ?? "null";
}

/** Recover original response bytes when available; no canonical object is mutated to store them. */
export function originalJson(value: unknown): string | undefined {
  return typeof value === "object" && value !== null ? sources.get(value) : undefined;
}

/** Identify preserved scalar numbers before generic object traversal or presentation. */
export function losslessNumber(value: unknown): value is LosslessNumber {
  return isLosslessNumber(value);
}

/** Return an exact integer spelling or reject numbers already rounded before this boundary. */
export function integerText(value: unknown): string | null {
  if (typeof value === "number") return Number.isSafeInteger(value) ? String(value) : null;
  if (typeof value === "bigint") return value.toString();
  return isLosslessNumber(value) && /^-?\d+$/.test(value.value) ? value.value : null;
}
