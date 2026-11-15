import { describe, expect, it } from "vitest";
import { instance } from "@viz-js/viz";
import { parseOpenPayload } from "../analysis/adapter";
import { scenarioById } from "../analysis/fixtures";
import type { EvidenceView } from "../analysis/types";
import { buildPresentationGraph, DEFAULT_GRAPH_NODE_LIMIT } from "./presentation";
import { sanitizeSvg } from "./sanitizer";

function evidenceFor(id: string): EvidenceView {
  const result = parseOpenPayload(scenarioById(id)!.openPayload);
  if (!result.ok || !result.value.evidence) throw new Error("Fixture did not parse");
  return result.value.evidence;
}

describe("presentation graph", () => {
  it("keeps duplicate occurrences as separate nodes with stable fact links", () => {
    const evidence = evidenceFor("tenant-boundary");
    const graph = buildPresentationGraph(evidence);
    const before = graph.nodes.filter((node) => node.kind === "before");
    expect(before).toHaveLength(5);
    expect(new Set(before.map((node) => node.id)).size).toBe(5);
    expect(before[0]?.labelLines[0]).toContain("#1");
    expect(before[1]?.labelLines[0]).toContain("#2");
    expect(before[0]?.labelLines.slice(1)).toEqual(before[1]?.labelLines.slice(1));
    for (const node of graph.nodes) {
      if (node.factReference) expect(evidence.facts.some((fact) => fact.reference === node.factReference)).toBe(true);
    }
  });

  it("makes typed NULL explicit and labels long hostile-looking data as text", () => {
    const graph = buildPresentationGraph(evidenceFor("tenant-boundary"));
    const labels = graph.nodes.flatMap((node) => node.labelLines).join("\n");
    expect(labels).toContain("NULL · int32");
    expect(graph.dot).not.toContain("label=<");
    expect(graph.dot).not.toContain('image="');
    expect(graph.dot).not.toContain("URL=");
    expect(graph.limitations.join(" ")).toMatch(/shortened|backslash/);
  });

  it("does not inherit instance completeness when the graph cap hides nodes", () => {
    const evidence = evidenceFor("state-large");
    const graph = buildPresentationGraph(evidence, 6);
    expect(evidence.instance.completeness.view).toBe(true);
    expect(graph.totalNodeCount).toBeGreaterThan(6);
    expect(graph.visibleNodeCount).toBeLessThanOrEqual(6);
    expect(graph.omittedNodeCount).toBeGreaterThan(0);
    expect(graph.viewComplete).toBe(false);
    expect(graph.limitations.join(" ")).toContain("omitted");
  });

  it("links mutation outputs and effects to supplied facts", () => {
    const evidence = evidenceFor("inventory-update");
    const graph = buildPresentationGraph(evidence, DEFAULT_GRAPH_NODE_LIMIT);
    expect(graph.nodes.some((node) => node.kind === "after")).toBe(true);
    expect(graph.nodes.some((node) => node.kind === "effect")).toBe(true);
    const effect = graph.nodes.find((node) => node.kind === "effect");
    expect(effect?.factReference).toBe("/semantic/outcome/outcome/effects/0");
    expect(graph.viewComplete).toBe(true);
  });

  it("escapes hostile labels through Graphviz and rejects active SVG content", async () => {
    const evidence = evidenceFor("tenant-boundary");
    const graph = buildPresentationGraph(evidence, 40);
    const viz = await instance();
    const rendered = viz.render(graph.dot, { format: "svg" });
    expect(rendered.status).toBe("success");
    if (rendered.status !== "success") return;
    const sanitized = sanitizeSvg(rendered.output);
    const parsed = new DOMParser().parseFromString(sanitized.markup, "image/svg+xml");
    expect(parsed.querySelector("script")).toBeNull();
    expect(parsed.querySelector("foreignObject")).toBeNull();
    expect(parsed.querySelector("[onload]")).toBeNull();
    expect(parsed.querySelector("[href]")).toBeNull();
    expect(parsed.documentElement.textContent).toContain("<script>alert('x')</script>");
  });
});
