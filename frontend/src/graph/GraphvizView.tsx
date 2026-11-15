import { stringifyJson } from "../analysis/json";
/**
 * Accessible Graphviz view: a pinned WASM renderer in a worker, sanitized SVG
 * insertion, zoom controls, a partial-view warning and an equivalent node
 * evidence inspector that does not depend on pointer gestures.
 */
import { useEffect, useId, useMemo, useRef, useState, type JSX } from "react";
import { buildPresentationGraph, type PresentationGraph } from "./presentation";
import { sanitizeSvg } from "./sanitizer";
import type { EvidenceView, FactModel } from "../analysis/types";

interface RenderReply {
  readonly type: "rendered" | "failed";
  readonly id: string;
  readonly svg?: string;
  readonly graphvizVersion?: string;
  readonly warnings?: readonly string[];
  readonly message?: string;
}

type RenderState =
  | { readonly kind: "loading" }
  | {
      readonly kind: "ready";
      readonly svg: string;
      readonly version: string;
      readonly removed: number;
    }
  | { readonly kind: "error"; readonly message: string }
  | { readonly kind: "idle" };

/** Props for the graph panel. */
export interface GraphvizViewProps {
  readonly evidence: EvidenceView;
  readonly onOpenJson: () => void;
  readonly onInspectFact: (reference: string) => void;
  readonly compact?: boolean;
}

const WORKER_UNAVAILABLE = "Web Workers are unavailable. Use Table view; deterministic evidence is unaffected.";
const RENDER_TIMEOUT = "Graphviz did not finish within 10 seconds. Use Table view; deterministic evidence is unaffected.";

/** Insert sanitized SVG nodes into a reserved container without HTML sinks. */
function insertSanitizedSvg(container: HTMLDivElement | null, markup: string): void {
  if (!container || !markup) return;
  const parsed = new DOMParser().parseFromString(markup, "image/svg+xml");
  container.replaceChildren(document.importNode(parsed.documentElement, true));
}

/** Create the bundled worker once, with a fail-closed offline fallback. */
function useGraphvizWorker(): {
  readonly worker: Worker | null;
  readonly unavailable: string | null;
} {
  const [unavailable, setUnavailable] = useState<string | null>(null);
  const worker = useMemo(() => {
    if (typeof Worker === "undefined") return null;
    try {
      return new Worker(new URL("./graphviz.worker.ts", import.meta.url), {
        type: "module",
        name: "sqlolla-graphviz",
      });
    } catch {
      return null;
    }
  }, []);
  useEffect(() => {
    if (!worker) setUnavailable(WORKER_UNAVAILABLE);
    return () => worker?.terminate();
  }, [worker]);
  return { worker, unavailable };
}

/** Apply one sanitized worker reply to local render state. */
function applyReply(reply: RenderReply, setState: (state: RenderState) => void): void {
  if (reply.type === "failed" || !reply.svg) {
    setState({
      kind: "error",
      message: reply.message ?? "Graphviz could not render this presentation graph.",
    });
    return;
  }
  try {
    const sanitized = sanitizeSvg(reply.svg);
    setState({
      kind: "ready",
      svg: sanitized.markup,
      version: reply.graphvizVersion ?? "unknown",
      removed: sanitized.removed.length,
    });
  } catch (error) {
    setState({
      kind: "error",
      message: error instanceof Error ? error.message : "SVG sanitizer rejected the render.",
    });
  }
}

/** Render incoming DOT through the worker with one request and a bounded wait. */
function useRenderedSvg(worker: Worker | null, dot: string): RenderState {
  const [state, setState] = useState<RenderState>({ kind: "idle" });
  useEffect(() => {
    if (!worker) return undefined;
    const id = `render-${Date.now()}-${Math.random().toString(16).slice(2)}`;
    setState({ kind: "loading" });
    const timeout = window.setTimeout(() => setState({ kind: "error", message: RENDER_TIMEOUT }), 10_000);
    const listener = (event: MessageEvent<RenderReply>) => {
      if (event.data.id !== id) return;
      window.clearTimeout(timeout);
      applyReply(event.data, setState);
    };
    worker.addEventListener("message", listener);
    worker.postMessage({ type: "render", id, dot });
    return () => {
      window.clearTimeout(timeout);
      worker.removeEventListener("message", listener);
    };
  }, [worker, dot]);
  return state;
}

/** Render the importable SVG markup into the graph viewport. */
function SvgCanvas({ markup, zoom, description }: { readonly markup: string; readonly zoom: number; readonly description: string }): JSX.Element {
  const container = useRef<HTMLDivElement>(null);
  useEffect(() => insertSanitizedSvg(container.current, markup), [markup]);
  return (
    <div className="graph-canvas" role="img" aria-label={description}>
      <div className="graph-zoom-layer" style={{ width: `${zoom}%` }}>
        <div ref={container} className="graph-svg" />
      </div>
    </div>
  );
}

/** Label the graph as a summary, not a complete relation proof. */
function GraphStateMessage({ state }: { readonly state: RenderState }): JSX.Element {
  if (state.kind === "loading")
    return (
      <div className="graph-placeholder" role="status">
        Laying out evidence graph…
      </div>
    );
  if (state.kind === "idle") return <div className="graph-placeholder">Graph renderer is not ready.</div>;
  if (state.kind === "error")
    return (
      <div className="graph-placeholder graph-placeholder--error" role="alert">
        {state.message}
      </div>
    );
  return <></>;
}

/** Render graph zoom controls. */
function ZoomControls({ zoom, setZoom }: { readonly zoom: number; readonly setZoom: (value: number) => void }): JSX.Element {
  return (
    <div className="zoom-controls" aria-label="Graph zoom controls">
      <button type="button" onClick={() => setZoom(Math.max(75, zoom - 25))} aria-label="Zoom graph out">
        −
      </button>
      <output aria-live="polite">{zoom}%</output>
      <button type="button" onClick={() => setZoom(Math.min(175, zoom + 25))} aria-label="Zoom graph in">
        +
      </button>
      <button type="button" onClick={() => setZoom(100)}>
        Reset
      </button>
    </div>
  );
}

/** Render the graph toolbar. */
function GraphToolbar({ zoom, setZoom }: { readonly zoom: number; readonly setZoom: (value: number) => void }): JSX.Element {
  return (
    <div className="graph-toolbar">
      <div className="graph-toolbar__meta">
        <span className="mono-label">GRAPH</span>
        <span>Structural summary, not row lineage</span>
      </div>
      <ZoomControls zoom={zoom} setZoom={setZoom} />
    </div>
  );
}

/** Render a visible partial-view warning. */
function PartialGraphBanner({ graph, onOpenJson }: { readonly graph: PresentationGraph; readonly onOpenJson: () => void }): JSX.Element | null {
  if (graph.viewComplete) return null;
  return (
    <div className="partial-banner" role="status">
      <strong>Partial graph view</strong>
      <span>
        Graph-view completeness is <b>false</b>. Instance evidence may still be complete.
      </span>
      {graph.limitations.map((limitation) => (
        <span key={limitation}>{limitation}</span>
      ))}
      <button type="button" className="text-button" onClick={onOpenJson}>
        Open original JSON
      </button>
    </div>
  );
}

/** Render the ready canvas and renderer provenance. */
function ReadyGraph({
  state,
  evidence,
  zoom,
}: {
  readonly state: Extract<RenderState, { kind: "ready" }>;
  readonly evidence: EvidenceView;
  readonly zoom: number;
}): JSX.Element {
  return (
    <>
      <SvgCanvas markup={state.svg} zoom={zoom} description={evidence.textDescription} />
      <p className="graph-provenance">
        Graphviz {state.version} via pinned self-hosted WASM in a worker · sanitizer removed {state.removed} unsupported item(s)
      </p>
    </>
  );
}

/** Render the current renderer state and sanitized canvas. */
function GraphViewport({
  unavailable,
  state,
  evidence,
  zoom,
  onOpenJson,
}: {
  readonly unavailable: string | null;
  readonly state: RenderState;
  readonly evidence: EvidenceView;
  readonly zoom: number;
  readonly onOpenJson: () => void;
}): JSX.Element {
  if (unavailable)
    return (
      <div className="graph-placeholder graph-placeholder--error" role="alert">
        {unavailable}
      </div>
    );
  if (state.kind === "ready") return <ReadyGraph state={state} evidence={evidence} zoom={zoom} />;
  return (
    <>
      <GraphStateMessage state={state} />
      <button type="button" className="text-button" onClick={onOpenJson}>
        Open original JSON
      </button>
    </>
  );
}

/** Render visible node choices. */
function NodeChoices({
  graph,
  selectedId,
  onSelect,
}: {
  readonly graph: PresentationGraph;
  readonly selectedId: string;
  readonly onSelect: (id: string) => void;
}): JSX.Element {
  return (
    <ul className="node-list" aria-label="Inspect graph evidence nodes">
      {graph.nodes.map((node) => (
        <li key={node.id}>
          <button
            type="button"
            className={node.id === selectedId ? "node-choice is-selected" : "node-choice"}
            onClick={() => onSelect(node.id)}
            aria-pressed={node.id === selectedId}
          >
            <span className="node-choice__kind">{node.kind}</span>
            <span>{node.labelLines[0]}</span>
          </button>
        </li>
      ))}
    </ul>
  );
}

/** Render the selected node's supplied fact. */
function FactPreview({
  fact,
  reference,
  onInspectFact,
}: {
  readonly fact: unknown;
  readonly reference: string | null;
  readonly onInspectFact: (reference: string) => void;
}): JSX.Element {
  return (
    <div className="fact-preview" aria-live="polite">
      <p className="mono-label">SELECTED NODE</p>
      {reference ? (
        <>
          <code>{reference}</code>
          <pre>{stringifyJson(fact, 2)}</pre>
          <button type="button" className="text-button" onClick={() => onInspectFact(reference)}>
            Open in evidence facts
          </button>
        </>
      ) : (
        <p>Structural summary only. This node has no supplied evidence fact reference.</p>
      )}
    </div>
  );
}

interface GraphInspectorProps {
  readonly graph: PresentationGraph;
  readonly selectedId: string;
  readonly selectedFact: FactModel | undefined;
  readonly onSelect: (id: string) => void;
  readonly onInspectFact: (reference: string) => void;
}

/** Render the accessible node inspector beneath the graph. */
function GraphInspector({ graph, selectedId, selectedFact, onSelect, onInspectFact }: GraphInspectorProps): JSX.Element {
  return (
    <div className="node-inspector">
      <div className="node-inspector__intro">
        <span className="mono-label">EVIDENCE NODES</span>
        <span>
          {graph.visibleNodeCount} of {graph.totalNodeCount} presentation nodes
        </span>
      </div>
      <div className="node-inspector__body">
        <NodeChoices graph={graph} selectedId={selectedId} onSelect={onSelect} />
        <FactPreview fact={selectedFact?.value} reference={selectedFact?.reference ?? null} onInspectFact={onInspectFact} />
      </div>
    </div>
  );
}

interface GraphPanelProps {
  readonly graph: PresentationGraph;
  readonly unavailable: string | null;
  readonly state: RenderState;
  readonly evidence: EvidenceView;
  readonly zoom: number;
  readonly setZoom: (value: number) => void;
  readonly descriptionId: string;
  readonly selectedId: string;
  readonly setSelectedId: (id: string) => void;
  readonly selectedFact: FactModel | undefined;
  readonly onOpenJson: () => void;
  readonly onInspectFact: (reference: string) => void;
  readonly compact?: boolean;
}

/** Render the graph panel. */
function GraphPanel(props: GraphPanelProps): JSX.Element {
  const { graph, unavailable, state, evidence, zoom, setZoom, descriptionId, selectedId, setSelectedId, selectedFact, onOpenJson, onInspectFact, compact } =
    props;
  const inspector = <GraphInspector graph={graph} selectedId={selectedId} selectedFact={selectedFact} onSelect={setSelectedId} onInspectFact={onInspectFact} />;
  return (
    <div className="graph-panel" data-no-swipe>
      <GraphToolbar zoom={zoom} setZoom={setZoom} />
      <p id={descriptionId} className="sr-only">
        {evidence.textDescription} Use the node list below for exact supplied fact references.
      </p>
      <PartialGraphBanner graph={graph} onOpenJson={onOpenJson} />
      <GraphViewport unavailable={unavailable} state={state} evidence={evidence} zoom={zoom} onOpenJson={onOpenJson} />
      {compact ? (
        <details className="graph-inspector-disclosure">
          <summary>Inspect {graph.visibleNodeCount} graph nodes</summary>
          {inspector}
        </details>
      ) : (
        inspector
      )}
    </div>
  );
}

/** Render the graph card controls, canvas, limitations and node inspector. */
export function GraphvizView(props: GraphvizViewProps): JSX.Element {
  const graph = useMemo(() => buildPresentationGraph(props.evidence), [props.evidence]);
  const { worker, unavailable } = useGraphvizWorker();
  const state = useRenderedSvg(worker, graph.dot);
  const [zoom, setZoom] = useState(props.compact ? 100 : 125);
  const [selectedId, setSelectedId] = useState("query");
  const descriptionId = useId();
  const selectedNode = graph.nodes.find((node) => node.id === selectedId) ?? graph.nodes[0];
  const selectedFact = selectedNode?.factReference ? props.evidence.facts.find((fact) => fact.reference === selectedNode.factReference) : undefined;
  return (
    <GraphPanel
      graph={graph}
      unavailable={unavailable}
      state={state}
      evidence={props.evidence}
      zoom={zoom}
      setZoom={setZoom}
      descriptionId={descriptionId}
      selectedId={selectedNode?.id ?? ""}
      setSelectedId={setSelectedId}
      selectedFact={selectedFact}
      onOpenJson={props.onOpenJson}
      onInspectFact={props.onInspectFact}
      compact={props.compact}
    />
  );
}
