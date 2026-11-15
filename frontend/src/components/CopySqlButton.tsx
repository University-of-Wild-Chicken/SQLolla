import { useRef, useState, type JSX } from "react";

interface CopyState {
  readonly sql: string;
  readonly copied: boolean;
}

/** Copy only the displayed SQL on explicit request, with a selectable fallback on failure. */
export function CopySqlButton({ sql }: { readonly sql: string }): JSX.Element {
  const [state, setState] = useState<CopyState | null>(null);
  const request = useRef(0);
  const result = state?.sql === sql ? state : null;
  const copy = async (): Promise<void> => {
    const id = ++request.current;
    let copied = false;
    try {
      await navigator.clipboard.writeText(sql);
      copied = true;
    } catch {
      // Clipboard support and permission vary by browser and origin.
    }
    if (request.current === id) setState({ sql, copied });
  };
  return (
    <div className="copy-sql">
      <button type="button" className="text-button" disabled={!sql} onClick={() => void copy()}>
        Copy SQL
      </button>
      <span role="status">{result ? (result.copied ? "SQL copied." : "Clipboard unavailable. Select and copy the SQL below.") : ""}</span>
      {result && !result.copied ? (
        <textarea aria-label="SQL to copy" className="code-block" readOnly rows={2} value={sql} onFocus={(event) => event.target.select()} />
      ) : null}
    </div>
  );
}
