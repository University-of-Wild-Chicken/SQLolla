import { afterEach, describe, expect, it, vi } from "vitest";
import { hostJson, resetHostSession } from "./host";
import { originalJson, parseJson, stringifyJson } from "./json";

const nonce = "a".repeat(64);
const status = { schema: "sqlolla.host.v1", model: "gpt-6.1-sol", csrf_token: nonce };

/** Build a mock HTTP response with the same text boundary as fetch. */
function response(value: unknown, code = 200): Response {
  return new Response(stringifyJson(value), { status: code, headers: { "Content-Type": "application/json" } });
}

afterEach(() => {
  resetHostSession();
  vi.unstubAllGlobals();
});

describe("same-origin host boundary", () => {
  it("retains large integers and sends one memory-only nonce/authorization header", async () => {
    const body = parseJson('{"value":9223372036854775807}');
    const request = vi
      .fn()
      .mockResolvedValueOnce(response(status))
      .mockResolvedValueOnce(response(body))
      .mockResolvedValueOnce(response({ ok: true }));
    vi.stubGlobal("fetch", request);
    const result = await hostJson("/api/ai/explain", body, { headers: { Authorization: "Bearer test-transient" } });
    expect(stringifyJson(result)).toBe('{"value":9223372036854775807}');
    expect(originalJson(result)).toBe('{"value":9223372036854775807}');
    const sent = request.mock.calls[1]![1] as RequestInit;
    expect(sent.body).toBe('{"value":9223372036854775807}');
    expect(new Headers(sent.headers).get("X-SQLolla-CSRF")).toBe(nonce);
    expect(sent.redirect).toBe("manual");
    await hostJson("/api/command", { action: "instances.open" });
    expect(new Headers(request.mock.calls[2]![1].headers).has("Authorization")).toBe(false);
    expect(request.mock.calls.filter(([url]) => url === "/api/status")).toHaveLength(1);
  });

  it("reports an Access login redirect without resending the provider key", async () => {
    const request = vi
      .fn()
      .mockResolvedValueOnce(response(status))
      .mockResolvedValueOnce(new Response(null, { status: 302 }));
    vi.stubGlobal("fetch", request);
    await expect(hostJson("/api/ai/explain", {}, { headers: { Authorization: "Bearer transient-test" } })).rejects.toMatchObject({ code: "host.sign_in" });
    expect(request).toHaveBeenCalledTimes(2);
  });

  it("reports static HTML clearly instead of treating it as an API response", async () => {
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue(new Response("<!doctype html>", { headers: { "Content-Type": "text/html" } })));
    await expect(hostJson("/api/command", {})).rejects.toMatchObject({ code: "host.invalid_response" });
  });

  it("never retries a rejected POST and reacquires the nonce only on a later explicit command", async () => {
    const request = vi
      .fn()
      .mockResolvedValueOnce(response(status))
      .mockResolvedValueOnce(response({ code: "host.csrf", message: "Rejected" }, 403));
    vi.stubGlobal("fetch", request);
    await expect(hostJson("/api/command", {})).rejects.toMatchObject({ code: "host.csrf" });
    expect(request).toHaveBeenCalledTimes(2);
    request.mockResolvedValueOnce(response(status)).mockResolvedValueOnce(response({ ok: true }));
    await hostJson("/api/command", {});
    expect(request.mock.calls[2]![0]).toBe("/api/status");
  });

  it("refuses another protocol and nonlocal paths before sending a command", async () => {
    const request = vi.fn().mockResolvedValue(response({ ...status, schema: "future-protocol" }));
    vi.stubGlobal("fetch", request);
    await expect(hostJson("https://example.com/api/command", {})).rejects.toMatchObject({ code: "host.path" });
    expect(request).not.toHaveBeenCalled();
    await expect(hostJson("/api/command", {})).rejects.toMatchObject({ code: "host.protocol" });
    expect(request).toHaveBeenCalledTimes(1);
  });
});
