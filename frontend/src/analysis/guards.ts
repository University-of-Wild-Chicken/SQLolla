/**
 * Small runtime shape guards for untrusted evidence JSON. They never execute
 * SQL or derive solver semantics; they only establish fields the UI can rely on.
 */

/** Internal shape violation with a stable adapter code. */
export class GuardError extends Error {
  /**
   * Create a contract-level shape failure.
   *
   * @param code - stable adapter error code.
   * @param message - human-readable path-aware explanation.
   */
  constructor(
    public readonly code: string,
    message: string,
  ) {
    super(message);
    this.name = "GuardError";
  }
}

/** Check whether a value is a non-array JSON object. */
export function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

/** Require a JSON object or fail with a path-aware adapter error. */
export function asRecord(value: unknown, path: string): Record<string, unknown> {
  if (!isRecord(value)) throw new GuardError("contract.shape", `${path} must be an object`);
  return value;
}

/** Require a string field. */
export function stringField(object: Record<string, unknown>, key: string, path: string): string {
  const value = object[key];
  if (typeof value !== "string") throw new GuardError("contract.shape", `${path}.${key} must be a string`);
  return value;
}

/** Read a string field when present, otherwise return a default. */
export function optionalStringField(object: Record<string, unknown>, key: string, path: string, fallback: string | null): string | null {
  if (object[key] === undefined || object[key] === null) return fallback;
  return stringField(object, key, path);
}

/** Require a finite number field. */
export function numberField(object: Record<string, unknown>, key: string, path: string): number {
  const value = object[key];
  if (typeof value !== "number" || !Number.isFinite(value)) {
    throw new GuardError("contract.shape", `${path}.${key} must be a finite number`);
  }
  return value;
}

/** Require a non-negative integer field. */
export function integerField(object: Record<string, unknown>, key: string, path: string): number {
  const value = numberField(object, key, path);
  if (!Number.isInteger(value) || value < 0) {
    throw new GuardError("contract.shape", `${path}.${key} must be a non-negative integer`);
  }
  return value;
}

/** Require a boolean field. */
export function booleanField(object: Record<string, unknown>, key: string, path: string): boolean {
  const value = object[key];
  if (typeof value !== "boolean") throw new GuardError("contract.shape", `${path}.${key} must be a boolean`);
  return value;
}

/** Require an array value. */
export function asArray(value: unknown, path: string): readonly unknown[] {
  if (!Array.isArray(value)) throw new GuardError("contract.shape", `${path} must be an array`);
  return value;
}

/** Require a string array value. */
export function stringArray(value: unknown, path: string): readonly string[] {
  return asArray(value, path).map((item, index) => {
    if (typeof item !== "string") {
      throw new GuardError("contract.shape", `${path}[${index}] must be a string`);
    }
    return item;
  });
}

/** Reject any schema identity other than the one declared by this adapter. */
export function requireSchema(object: Record<string, unknown>, expected: string, path: string): void {
  const actual = object["schema"];
  if (actual !== expected) {
    const suffix = typeof actual === "string" ? actual : "missing";
    throw new GuardError("contract.unsupported_schema", `${path} uses unsupported schema "${suffix}" (expected "${expected}")`);
  }
}

/** Require a lowercase SHA-256-shaped identity. */
export function hashField(object: Record<string, unknown>, key: string, path: string): string {
  const value = stringField(object, key, path);
  if (!/^[0-9a-f]{64}$/.test(value)) {
    throw new GuardError("contract.shape", `${path}.${key} must be a lowercase SHA-256 identity`);
  }
  return value;
}

/** Reject status values outside a fixed literal union. */
export function literalField<T extends string>(object: Record<string, unknown>, key: string, path: string, allowed: readonly T[]): T {
  const value = stringField(object, key, path);
  if (!allowed.includes(value as T)) {
    throw new GuardError("contract.shape", `${path}.${key} has unsupported value "${value}"`);
  }
  return value as T;
}
