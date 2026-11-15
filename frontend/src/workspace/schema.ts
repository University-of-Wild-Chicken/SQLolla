/** Build an explicitly declared table without replacing the rest of the local request. */
import { parseCatalog } from "../analysis/adapter";
import { asRecord } from "../analysis/guards";
import { parseJson } from "../analysis/json";
import type { CatalogModel } from "../analysis/types";
import { editableRequest } from "./native";

const identifier = "[a-z_][a-z0-9_]*";
const relation = new RegExp(`^(?:${identifier}\\.)?${identifier}$`);
const declaration = new RegExp("^(" + identifier + "):(int32|int64|text|bool)(\\?)?$");
export const simpleTypes = ["int32", "int64", "text", "bool"] as const;
export type SimpleType = (typeof simpleTypes)[number];

/** The editor, manual builder and AI drafting all read the same catalog. */
export function currentCatalog(text: string): CatalogModel | null {
  try {
    return parseCatalog(editableRequest(text).catalog, "request catalog");
  } catch {
    return null;
  }
}

/** Parse the form's core types and explicit nullable marker. Other types use the full request editor. */
function columns(source: string) {
  const result = source.split(",").map((item) => {
    const match = declaration.exec(item.trim());
    if (!match) throw new Error("Enter columns as name:type. Add ? after the type for a nullable column.");
    return { name: match[1], ty: match[2], nullable: !!match[3] };
  });
  if (new Set(result.map((column) => column.name)).size !== result.length) throw new Error("Each column needs a unique name.");
  return result;
}

/** Add one table and its explicit modeled row limit, preserving input mode and all other fields. */
export function addTable(text: string, tableName: string, source: string, limit: number) {
  const request = editableRequest(text);
  const catalog = parseCatalog(request.catalog, "request catalog");
  const input = tableName.trim();
  if (!relation.test(input)) throw new Error("Enter a lower-case table name, optionally prefixed with its schema.");
  const [schema, name] = input.includes(".") ? input.split(".") : [catalog.searchPath[0] ?? "public", input];
  if (Object.values(catalog.tables).some((table) => table.schema === schema && table.name === name)) throw new Error("This table is already declared.");
  const key = `${schema}.${name}`;
  const table = { schema, name, columns: columns(source) };
  const raw = asRecord(catalog.raw, "catalog");
  const updated = { ...raw, tables: { ...asRecord(raw.tables, "tables"), [key]: table } };
  return { ...request, catalog: updated, ...tableInput(request, key, limit) };
}

/** Keep symbolic limits and concrete bags in their respective modes. */
function tableInput(request: Record<string, unknown>, key: string, limit: number) {
  if (request.mode === "analyze") {
    if (!Number.isSafeInteger(limit) || limit < 0) throw new Error("Enter a nonnegative whole number of modeled rows.");
    return { bounds: { ...asRecord(request.bounds ?? {}, "row bounds"), [key]: limit } };
  }
  return { database: { ...asRecord(request.database ?? {}, "database"), [key]: [] } };
}

/** Preserve exact 64-bit values and accept only explicit boolean literals. */
export function typedValue(type: SimpleType, value: string): unknown {
  if (type === "text") return { type, value };
  if (type === "bool") {
    if (value !== "true" && value !== "false") throw new Error("Enter true or false.");
    return { type, value: value === "true" };
  }
  if (!/^-?\d+$/.test(value)) throw new Error("Enter a whole number.");
  const number = BigInt(value);
  const edge = 1n << (type === "int32" ? 31n : 63n);
  if (number < -edge || number >= edge) throw new Error(`Value exceeds ${type}.`);
  return { type, value: parseJson(number.toString()) };
}
