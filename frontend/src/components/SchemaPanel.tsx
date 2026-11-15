/** Visible schema management for both manual SQL and low-code drafts. */
import { useState, type JSX } from "react";
import type { CatalogModel } from "../analysis/types";
import { editableRequest, type NativeAnalysis } from "../workspace/native";
import { addTable } from "../workspace/schema";
import { BoundedInputs } from "./BoundedInputs";
import { AdvancedRequest } from "./NativeAnalysisPanel";

interface Props {
  readonly analysis: NativeAnalysis;
  readonly catalog: CatalogModel | null;
  readonly change: (request: Record<string, unknown>) => void;
  readonly edited: () => void;
}

/** Expose the active tables before a user tries to bind a query. */
export function SchemaPanel(props: Props): JSX.Element {
  const tables = Object.values(props.catalog?.tables ?? {}).map(({ schema, name }) => `${schema}.${name}`);
  return (
    <details className="query-schema" open={!tables.length || undefined}>
      <summary>Schema and rule</summary>
      <p>{props.catalog ? tables.join(", ") || "No tables declared" : "Fix the catalog in the advanced request."}</p>
      {props.catalog ? <AddTableForm analysis={props.analysis} change={props.change} /> : null}
      {symbolic(props.analysis.text) ? <BoundedInputs text={props.analysis.text} change={props.change} /> : null}
      <AdvancedRequest
        analysis={{
          ...props.analysis,
          editText: (value) => {
            props.analysis.editText(value);
            props.edited();
          },
        }}
      />
    </details>
  );
}

/** Mode only selects which explicit input extension the table form will produce. */
function symbolic(text: string): boolean {
  try {
    return editableRequest(text).mode === "analyze";
  } catch {
    return false;
  }
}

/** A new declaration becomes available to the query builder immediately. */
function AddTableForm({ analysis, change }: Pick<Props, "analysis" | "change">): JSX.Element {
  const fields = useTableFields(analysis.text, change);
  return <TableFields {...fields} bounded={symbolic(analysis.text)} />;
}

/** Validate an explicit declaration before touching the request. */
function useTableFields(text: string, change: Props["change"]) {
  const [name, setName] = useState("");
  const [columns, setColumns] = useState("");
  const [limit, setLimit] = useState("1");
  const [error, setError] = useState<string | null>(null);
  const add = (): void => {
    try {
      change(addTable(text, name, columns, limit === "" ? NaN : Number(limit)));
      setName("");
      setColumns("");
      setError(null);
    } catch (failure) {
      setError(failure instanceof Error ? failure.message : "Could not declare this table.");
    }
  };
  return { name, setName, columns, setColumns, limit, setLimit, error, add };
}

/** Show the supported declaration syntax close to the fields. */
function TableFields({ bounded, ...fields }: ReturnType<typeof useTableFields> & { readonly bounded: boolean }): JSX.Element {
  const { name, setName, columns, setColumns, limit, setLimit, error, add } = fields;
  return (
    <div className="schema-fields">
      <label>
        New table name
        <input value={name} placeholder="public.customers" onChange={(event) => setName(event.target.value)} />
      </label>
      <label>
        Column definitions
        <input value={columns} placeholder="id:int32, name:text?" onChange={(event) => setColumns(event.target.value)} />
      </label>
      <p>Use int32, int64, text or bool. Add ? for a nullable column.</p>
      {bounded ? (
        <label>
          Maximum modeled rows
          <input type="number" min="0" value={limit} onChange={(event) => setLimit(event.target.value)} />
        </label>
      ) : null}
      {error ? <p role="alert">{error}</p> : null}
      <button type="button" className="button" onClick={add} disabled={!name.trim() || !columns.trim()}>
        Declare table
      </button>
    </div>
  );
}
