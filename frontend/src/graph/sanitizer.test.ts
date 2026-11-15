import { describe, expect, it } from "vitest";
import { sanitizeSvg } from "./sanitizer";

describe("SVG sanitizer boundary", () => {
  it("removes scripts, foreign content, images, links, events and data URLs", () => {
    const hostile = `<?xml version="1.0"?>
      <svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" onload="alert(1)">
        <script>alert(2)</script>
        <foreignObject><body xmlns="http://www.w3.org/1999/xhtml"><img src="x" onerror="alert(3)"/></body></foreignObject>
        <image href="https://evil.example/pixel.png" width="10" height="10"/>
        <a xlink:href="javascript:alert(4)"><text>linked</text></a>
        <g onclick="alert(5)"><rect width="10" height="10" fill="url(https://evil.example/fill)"/><text>safe label</text></g>
      </svg>`;
    const sanitized = sanitizeSvg(hostile);
    const parsed = new DOMParser().parseFromString(sanitized.markup, "image/svg+xml");
    expect(parsed.querySelector("script")).toBeNull();
    expect(parsed.querySelector("foreignObject")).toBeNull();
    expect(parsed.querySelector("image")).toBeNull();
    expect(parsed.querySelector("a")).toBeNull();
    expect(parsed.querySelector("[onload]")).toBeNull();
    expect(parsed.querySelector("[onclick]")).toBeNull();
    expect(parsed.querySelector("[href]")).toBeNull();
    expect(parsed.querySelector("[src]")).toBeNull();
    expect(sanitized.markup.toLowerCase()).not.toContain("javascript:");
    expect(sanitized.markup.toLowerCase()).not.toContain("evil.example");
    expect(parsed.documentElement.textContent).toContain("safe label");
    expect(sanitized.removed.length).toBeGreaterThan(0);
  });

  it("rejects malformed SVG instead of returning partial markup", () => {
    expect(() => sanitizeSvg("<svg><text>not closed</svg>")).toThrow();
    expect(() => sanitizeSvg("<html><script>alert(1)</script></html>")).toThrow();
  });
});
