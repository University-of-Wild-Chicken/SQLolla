/** Validate and label structured SQL types without rewriting canonical evidence. */
import { asRecord, GuardError, stringArray, stringField } from "./guards";
import type { SqlTypeModel } from "./types";

const PRIMITIVES = new Set([
  "int32",
  "int64",
  "bool",
  "text",
  "int16",
  "float64",
  "bpchar",
  "name",
  "oid",
  "xid",
  "regclass",
  "internal_char",
  "bytea",
  "date",
  "timestamp",
  "timestamptz",
  "interval",
  "jsonb",
  "json",
  "void",
]);

/** Require known primitive or structured types, with a bounded recursive array depth. */
export function parseSqlType(value: unknown, path: string, depth = 0): SqlTypeModel {
  if (depth > 32) throw new GuardError("contract.type", `${path} exceeds the type depth limit`);
  if (typeof value === "string" && PRIMITIVES.has(value)) return value;
  const object = asRecord(value, path);
  const entries = Object.entries(object);
  if (entries.length !== 1) throw new GuardError("contract.type", `${path} must have one type constructor`);
  const [kind, detail] = entries[0]!;
  if (kind === "array") parseSqlType(detail, `${path}.array`, depth + 1);
  else validateConstructor(kind, detail, path);
  return object;
}

/** Check numeric modifiers as exact bounded UI metadata, never as scalar cell values. */
function modifier(value: unknown, path: string, nullable: boolean, minimum: number): void {
  if (value === null && nullable) return;
  if (typeof value !== "number" || !Number.isSafeInteger(value) || value < minimum) {
    throw new GuardError("contract.type", `${path} has an invalid type modifier`);
  }
}

/** Validate constructor fields while retaining the exact original descriptor. */
function validateConstructor(kind: string, value: unknown, path: string): void {
  const object = asRecord(value, path);
  if (kind === "numeric") {
    modifier(object["precision"], path, true, 1);
    modifier(object["scale"], path, true, -32768);
  } else if (["varchar", "char", "bit"].includes(kind)) {
    modifier(object["length"], path, kind === "varchar", 0);
  } else if (kind === "enum") {
    stringField(object, "name", path);
    stringArray(object["labels"], path);
  } else throw new GuardError("contract.type", `${path} has unsupported type ${kind}`);
  const allowed = kind === "numeric" ? ["precision", "scale"] : kind === "enum" ? ["name", "labels"] : ["length"];
  if (Object.keys(object).some((key) => !allowed.includes(key))) throw new GuardError("contract.type", `${path} has unknown type fields`);
}

/** Render nested descriptors legibly while leaving nominal identity and storage modifiers intact. */
export function sqlTypeText(type: SqlTypeModel): string {
  if (typeof type === "string") return type;
  const [kind, value] = Object.entries(type)[0]!;
  if (kind === "array") return `${sqlTypeText(value as SqlTypeModel)}[]`;
  const detail = value as Record<string, unknown>;
  if (kind === "enum") return String(detail["name"]);
  if (kind === "numeric") return detail["precision"] === null ? "numeric" : `numeric(${detail["precision"]},${detail["scale"] ?? 0})`;
  return detail["length"] === null ? kind : `${kind}(${detail["length"]})`;
}
