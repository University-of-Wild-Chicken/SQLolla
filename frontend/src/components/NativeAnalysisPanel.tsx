/** Accessible native entry point with a small example and advanced explicit request context. */
import type { JSX } from "react";
import type { NativeAnalysis } from "../workspace/native";

/** Keep SQL readable while advanced catalog, parameters, bounds and property remain explicitly editable. */
export function NativeAnalysisPanel({ analysis }: { readonly analysis: NativeAnalysis }): JSX.Element {
  return (
    <div className="rail-scroll builder-panel">
      <div className="rail-heading">
        <span className="mono-label">LOCAL ANALYSIS</span>
        <h2>Check your query</h2>
        <p>Try a tenant-isolation example. Look for a result belonging to tenant 0 when only tenant 1 is allowed.</p>
      </div>
      <label className="field-label">
        SQL query
        <textarea aria-label="SQL query" className="code-block" rows={7} value={analysis.sql} onChange={(event) => analysis.editSql(event.target.value)} />
      </label>
      <NativeControls analysis={analysis} />
      <AdvancedRequest analysis={analysis} />
      {analysis.error ? (
        <div role="alert" className="inline-alert inline-alert--danger">
          {analysis.error}
        </div>
      ) : null}
      <button type="button" className="primary-button" disabled={analysis.pending} onClick={() => void analysis.start()}>
        {analysis.pending ? "Analyzing…" : "Analyze query"}
      </button>
      <p className="rail-footnote">Results cover the declared finite model. Your database is not connected or modified.</p>
    </div>
  );
}

/** Advanced context is editable as exact JSON, hidden from the first-run query flow. */
function AdvancedRequest({ analysis }: { readonly analysis: NativeAnalysis }): JSX.Element {
  return (
    <details>
      <summary>Advanced request: schema, values and rule</summary>
      <p>This explicit JSON is sent to the local analyzer. Large integers retain their exact values.</p>
      <textarea
        aria-label="Advanced analysis request"
        className="code-block"
        rows={18}
        value={analysis.text}
        onChange={(event) => analysis.editText(event.target.value)}
      />
    </details>
  );
}

/** Explicit example/counterexample and enumeration limits never change after evidence is produced. */
function NativeControls({ analysis }: { readonly analysis: NativeAnalysis }): JSX.Element {
  return (
    <div className="builder-section">
      <label>
        Look for
        <select aria-label="Analysis goal" value={analysis.kind} onChange={(event) => analysis.setKind(event.target.value as NativeAnalysis["kind"])}>
          <option value="counterexample">Unintended behavior</option>
          <option value="example">Possible examples</option>
        </select>
      </label>
      <label>
        Maximum review cases
        <input
          aria-label="Maximum review cases"
          type="number"
          min={1}
          max={256}
          value={analysis.maxInstances}
          onChange={(event) => analysis.setMaxInstances(Number(event.target.value))}
        />
      </label>
    </div>
  );
}

/** Draft changes suppress old evidence until a new revision has been opened successfully. */
export function NativeDraftState(): JSX.Element {
  return (
    <section className="state-panel">
      <span className="mono-label">DRAFT AWAITING ANALYSIS</span>
      <h2>Ready to check a new question</h2>
      <p>Edit the SQL and explicit context, then choose Analyze query. The local backend will compile it, ask Alloy for a case, and replay the result.</p>
    </section>
  );
}
