/** Editable finite domain and property for a user-owned schema. */
import { useState, type JSX } from "react";
import { stringifyJson } from "../analysis/json";
import { editableRequest } from "../workspace/native";
import { simpleTypes, typedValue, type SimpleType } from "../workspace/schema";

type InputProps = { readonly text: string; readonly change: (value: Record<string, unknown>) => void };

/** Manage simple typed values with input that never rounds 64-bit numbers. */
function ValueFields({
  type,
  setType,
  value,
  setValue,
}: {
  readonly type: SimpleType;
  readonly value: string;
  readonly setType: (type: SimpleType) => void;
  readonly setValue: (value: string) => void;
}): JSX.Element {
  return (
    <>
      <label>
        Value type
        <select aria-label="Value type" value={type} onChange={(event) => setType(event.target.value as SimpleType)}>
          {simpleTypes.map((item) => (
            <option key={item}>{item}</option>
          ))}
        </select>
      </label>
      <label>
        Value
        <input value={value} onChange={(event) => setValue(event.target.value)} />
      </label>
    </>
  );
}

/** Return one field group and the exact typed value when the user commits it. */
function useTypedEntry() {
  const [type, setType] = useState<SimpleType>("int32");
  const [value, setValue] = useState("");
  return { fields: { type, setType, value, setValue }, commit: () => typedValue(type, value) };
}

/** Make the populated domain visible and append only explicitly entered values. */
function ModeledValues({ text, change }: InputProps): JSX.Element {
  const request = editableRequest(text);
  const domain = Array.isArray(request.domain) ? request.domain : [];
  const entry = useTypedEntry();
  const [error, setError] = useState<string | null>(null);
  const add = (): void => {
    try {
      change({ ...request, domain: [...domain, entry.commit()] });
      entry.fields.setValue("");
      setError(null);
    } catch (failure) {
      setError(failure instanceof Error ? failure.message : "Invalid modeled value.");
    }
  };
  return (
    <details className="schema-fields">
      <summary>Modeled values ({domain.length})</summary>
      <p>These values form the finite input for analysis. Enter at least one for each column type.</p>
      {domain.length ? <pre className="code-block">{stringifyJson(domain, 2)}</pre> : null}
      <ValueFields {...entry.fields} />
      {error ? <p role="alert">{error}</p> : null}
      <button type="button" className="button" onClick={add}>
        Add modeled value
      </button>
    </details>
  );
}

type RuleKind = "no_sql_error" | "max_rows" | "only_value" | "no_value";

/** Compile the UI rule without guessing a column position or value. */
function rule(kind: RuleKind, amount: string, cell: () => unknown): unknown {
  if (kind === "no_sql_error") return { kind };
  if (!/^\d+$/.test(amount) || !Number.isSafeInteger(Number(amount))) throw new Error("Enter a whole number.");
  const position = Number(amount);
  if (kind === "max_rows") return { kind, max: position };
  if (position < 1) throw new Error("Output positions start at 1.");
  return { kind, column: position - 1, value: cell() };
}

/** Show the current property and expose the ordinary bounded rule kinds. */
function RuleForm({ text, change }: InputProps): JSX.Element {
  const request = editableRequest(text);
  const [kind, setKind] = useState<RuleKind>("no_sql_error");
  const [amount, setAmount] = useState("");
  const [error, setError] = useState<string | null>(null);
  const cell = useTypedEntry();
  const save = (): void => {
    try {
      change({ ...request, property: rule(kind, amount, cell.commit) });
      setError(null);
    } catch (failure) {
      setError(failure instanceof Error ? failure.message : "Invalid rule.");
    }
  };
  return (
    <RuleFields current={request.property} kind={kind} setKind={setKind} amount={amount} setAmount={setAmount} fields={cell.fields} error={error} save={save} />
  );
}

type RuleFieldsProps = {
  readonly current: unknown;
  readonly kind: RuleKind;
  readonly setKind: (kind: RuleKind) => void;
  readonly amount: string;
  readonly setAmount: (amount: string) => void;
  readonly fields: ReturnType<typeof useTypedEntry>["fields"];
  readonly error: string | null;
  readonly save: () => void;
};

/** Expose only the fields used by the selected rule kind. */
function RuleFields({ current, kind, setKind, amount, setAmount, fields, error, save }: RuleFieldsProps): JSX.Element {
  return (
    <details className="schema-fields">
      <summary>Output rule</summary>
      <pre className="code-block">{stringifyJson(current ?? null, 2)}</pre>
      <label>
        Rule type
        <select aria-label="Rule type" value={kind} onChange={(event) => setKind(event.target.value as RuleKind)}>
          <option value="no_sql_error">No SQL error</option>
          <option value="max_rows">Limit returned rows</option>
          <option value="only_value">Require a value in an output column</option>
          <option value="no_value">Forbid a value in an output column</option>
        </select>
      </label>
      {kind !== "no_sql_error" ? (
        <label>
          {kind === "max_rows" ? "Maximum returned rows" : "Output column position"}
          <input type="number" min={kind === "max_rows" ? 0 : 1} value={amount} onChange={(event) => setAmount(event.target.value)} />
        </label>
      ) : null}
      {kind === "only_value" || kind === "no_value" ? <ValueFields {...fields} /> : null}
      {error ? <p role="alert">{error}</p> : null}
      <button type="button" className="button" onClick={save}>
        Set output rule
      </button>
    </details>
  );
}

/** Leave complex cases available through the advanced exact JSON editor. */
export function BoundedInputs(props: InputProps): JSX.Element {
  return (
    <>
      <ModeledValues {...props} />
      <RuleForm {...props} />
    </>
  );
}
