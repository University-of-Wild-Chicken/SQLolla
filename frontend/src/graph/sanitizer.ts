/**
 * Defense-in-depth boundary for Graphviz SVG. The DOT is generated locally from
 * validated evidence, then the returned SVG is parsed with an allowlist so no
 * active, external, or embedded content crosses into React.
 */

/** Tags emitted for the plain presentation graph; anything else is removed. */
const ALLOWED_TAGS = new Set(["svg", "g", "title", "desc", "polygon", "polyline", "path", "ellipse", "circle", "rect", "line", "text", "tspan"]);

/** Attribute prefixes/names that can trigger navigation or active content. */
const FORBIDDEN_ATTRIBUTES = new Set(["href", "xlink:href", "src", "action", "formaction", "poster", "data", "srcdoc"]);

/** Sanitized SVG plus a record of removed elements/attributes for diagnosis. */
export interface SanitizedSvg {
  readonly markup: string;
  readonly removed: readonly string[];
}

/** Check an attribute value for external or scriptable URL references. */
function dangerousValue(value: string): boolean {
  const normalized = value.replace(/\s+/g, "").toLowerCase();
  return normalized.includes("javascript:") || normalized.includes("url(") || normalized.includes("data:text/html");
}

/** Scrub attributes on one element, returning removed attribute names. */
function cleanAttributes(element: Element): string[] {
  const removed: string[] = [];
  for (const attribute of [...element.attributes]) {
    const name = attribute.name.toLowerCase();
    const forbidden = name.startsWith("on") || FORBIDDEN_ATTRIBUTES.has(name) || name.includes("href");
    if (forbidden || dangerousValue(attribute.value)) {
      element.removeAttributeNode(attribute);
      removed.push(`${element.tagName}.${attribute.name}`);
    }
  }
  return removed;
}

/** Remove a disallowed element and report its tag. */
function removeElement(element: Element): string {
  const tag = element.tagName.toLowerCase();
  element.remove();
  return `element:${tag}`;
}

/** Parse and sanitize a Graphviz SVG string without executing or navigating. */
export function sanitizeSvg(raw: string): SanitizedSvg {
  const parsed = new DOMParser().parseFromString(raw, "image/svg+xml");
  if (parsed.getElementsByTagName("parsererror").length > 0) {
    throw new Error("Graphviz returned malformed SVG");
  }
  const root = parsed.documentElement;
  if (!root || root.tagName.toLowerCase() !== "svg") throw new Error("Graphviz output is not an SVG root");
  const removed: string[] = [];
  for (const element of [...parsed.querySelectorAll("*")]) {
    if (!ALLOWED_TAGS.has(element.tagName.toLowerCase())) {
      removed.push(removeElement(element));
      continue;
    }
    removed.push(...cleanAttributes(element));
  }
  removed.push(...cleanAttributes(root));
  root.setAttribute("role", "img");
  root.setAttribute("aria-hidden", "true");
  root.setAttribute("focusable", "false");
  const markup = new XMLSerializer().serializeToString(root);
  return { markup, removed };
}
