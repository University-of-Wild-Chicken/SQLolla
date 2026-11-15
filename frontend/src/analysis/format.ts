/** Presentation-only formatting and JSON Pointer helpers. */
import type { CellModel, ColumnModel, PropertyModel } from "./types";
import { parseSqlType, sqlTypeText } from "./sqlTypes";
import { losslessNumber, stringifyJson } from "./json";

/** Render a typed cell as plain, unambiguous text without interpreting SQL. */
export function cellText(cell: CellModel): string {
  if (losslessNumber(cell.value)) return cell.value.toString();
  if (cell.type === "null") return `NULL · ${sqlTypeText(parseSqlType(cell.value, "NULL type"))}`;
  if (cell.type === "text" && typeof cell.value === "string") return cell.value;
  if (cell.type === "bool" && typeof cell.value === "boolean") {
    return cell.value ? "TRUE" : "FALSE";
  }
  if (typeof cell.value === "string") return cell.value;
  if (typeof cell.value === "number" || typeof cell.value === "boolean") {
    return String(cell.value);
  }
  if (cell.type === "scalar" && typeof cell.value === "object" && cell.value !== null) {
    const scalar = cell.value as Record<string, unknown>;
    if (typeof scalar["value"] === "string") return scalar["value"];
  }
  return stringifyJson(cell.value);
}

/** Render an exact typed value, distinguishing NULL and string identity. */
export function typedValueText(cell: CellModel): string {
  if (cell.type === "null") return `NULL::${sqlTypeText(parseSqlType(cell.value, "NULL type"))}`;
  if (cell.type === "text") return stringifyJson(cell.value);
  return cellText(cell);
}

/** Format a short SHA-256 identity for a chip. */
export function shortHash(hash: string | null | undefined): string {
  if (!hash) return "not supplied";
  return `${hash.slice(0, 8)}…${hash.slice(-4)}`;
}

/** State a supported property in plain language. */
export function propertySentence(property: PropertyModel | null, columns: readonly ColumnModel[] = []): string {
  if (!property) return "No failure rule is attached to this stored case.";
  if (property.kind === "no_sql_error") return "The statement should complete without a modeled SQL error.";
  if (property.kind === "max_rows") {
    return `The result should contain at most ${property.max} row${property.max === 1 ? "" : "s"}.`;
  }
  const column = columns[property.column];
  const label = column ? column.name : `column index ${property.column}`;
  const comparator = property.kind === "only_value" ? "only" : "never";
  return `Column “${label}” should ${comparator} contain ${typedValueText(property.value)}.`;
}

/** Escape one JSON Pointer segment. */
export function escapePointerSegment(segment: string): string {
  return segment.replaceAll("~", "~0").replaceAll("/", "~1");
}

/** Decode one RFC 6901 JSON Pointer segment. */
export function unescapePointerSegment(segment: string): string {
  return segment.replaceAll("~1", "/").replaceAll("~0", "~");
}

/** Resolve a JSON Pointer against an untrusted JSON document. */
export function resolveJsonPointer(document: unknown, pointer: string): unknown {
  if (pointer === "") return document;
  if (!pointer.startsWith("/")) return undefined;
  return pointer
    .slice(1)
    .split("/")
    .map(unescapePointerSegment)
    .reduce<unknown>((current, segment) => {
      if (Array.isArray(current)) return current[Number(segment)];
      if (typeof current === "object" && current !== null) {
        return (current as Record<string, unknown>)[segment];
      }
      return undefined;
    }, document);
}

/** Canonical JSON used only for embedded fact consistency checks. */
export function canonicalJson(value: unknown): string {
  if (losslessNumber(value)) return stringifyJson(value);
  if (Array.isArray(value)) return `[${value.map(canonicalJson).join(",")}]`;
  if (typeof value === "object" && value !== null) {
    const entries = Object.entries(value as Record<string, unknown>)
      .sort(([left], [right]) => left.localeCompare(right))
      .map(([key, child]) => `${JSON.stringify(key)}:${canonicalJson(child)}`);
    return `{${entries.join(",")}}`;
  }
  return stringifyJson(value);
}

/** Build a table's stable display name. */
export function tableLabel(tableId: string): { schema: string; name: string } {
  const dot = tableId.lastIndexOf(".");
  if (dot <= 0) return { schema: "unqualified", name: tableId };
  return { schema: tableId.slice(0, dot), name: tableId.slice(dot + 1) };
}
