/** Real browser/host checks; optional provider credentials enter only through an in-memory argument. */
import assert from "node:assert/strict";
import { createRequire } from "node:module";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";

const { chromium } = createRequire(import.meta.url)("playwright-core");

/** Match a canonical action without inspecting or retaining any authentication headers. */
function commandResponse(action) {
  return (response) => {
    if (!response.url().endsWith("/api/command")) return false;
    try {
      return JSON.parse(response.request().postData() ?? "{}").action === action;
    } catch {
      return false;
    }
  };
}

/** Start actual SAT work and require the subsequent canonical bundle reload before review. */
async function analyze(page) {
  const opened = page.waitForResponse(commandResponse("instances.open"), { timeout: 120_000 });
  await page.getByRole("button", { name: "Analyze query", exact: true }).click();
  const payload = await (await opened).json();
  assert.equal(payload.status, "ok");
  assert.equal(payload.record.instance.validation, "RUST_REPLAY_CONFIRMED");
  assert.ok(payload.bundle.ir && payload.bundle.model.includes("pred"));
  await page.getByTestId("review-card").waitFor();
  await page.locator(".graph-svg svg").waitFor({ timeout: 20_000 });
  assert.equal(await page.getByTestId("review-card").getAttribute("data-instance-id"), payload.record.instance.instance_id);
  return payload;
}

/** Explicit Next must make one canonical request and replace one visible card. */
async function nextCase(page) {
  const before = await page.getByTestId("review-card").getAttribute("data-instance-id");
  const response = page.waitForResponse(commandResponse("instances.next"));
  await page.getByRole("button", { name: "Next counterexample", exact: true }).click();
  const payload = await (await response).json();
  assert.equal(payload.status, "ok");
  assert.equal(payload.enumeration_status, "instance");
  await page.waitForFunction((id) => document.querySelector('[data-testid="review-card"]')?.getAttribute("data-instance-id") !== id, before);
  assert.equal(await page.getByTestId("review-card").count(), 1);
  return payload;
}

/** Configure explicit unredacted synthetic context; saving settings must not send the key. */
async function configureAi(page, apiKey) {
  await page.getByRole("button", { name: "Open explanation provider settings" }).click();
  await page.getByRole("radio", { name: /OpenAI-compatible/ }).check();
  assert.equal(await page.getByLabel("Model identifier").inputValue(), "gpt-6.1-sol");
  await page.getByLabel("API key (memory only)", { exact: true }).fill(apiKey);
  await page.getByLabel("Redact SQL and values before sending explanation context").uncheck();
  await page.getByRole("button", { name: "Save in this tab · no send", exact: true }).click();
  await page.getByRole("dialog").waitFor({ state: "detached" });
}

/** Exercise the actual UI→host→provider request and validate its evidence identity. */
async function assistance(page, instance) {
  const response = page.waitForResponse((item) => item.url().endsWith("/api/ai/explain"), { timeout: 180_000 });
  await page.getByRole("button", { name: "Explain this case", exact: true }).click();
  const received = await response;
  const payload = await received.json();
  assert.equal(received.status(), 200, `Assistance failed: ${payload.code ?? "unknown"}`);
  assert.equal(payload.status, "ok");
  assert.equal(payload.model, "gpt-6.1-sol");
  assert.equal(payload.instance_id, instance.instance_id);
  assert.equal(payload.model_revision_sha256, instance.model_revision_sha256);
  assert.match(payload.context_sha256, /^[0-9a-f]{64}$/);
  const references = new Set(instance.facts.map((fact) => fact.reference));
  assert.ok(payload.document.claims.every((claim) => claim.fact_references.every((ref) => references.has(ref))));
  await page.getByRole("heading", { name: payload.document.title, exact: true }).waitFor();
  return payload;
}

/** Optional live explanation stays separate from deterministic refinement checks. */
async function checkAi(page, apiKey, instance) {
  await configureAi(page, apiKey);
  await page
    .getByLabel("Question about this case")
    .fill("Explain why this tenant-isolation counterexample violates the retained rule. Cite supplied row facts and mention finite bounds.");
  const summary = await assistance(page, instance);
  return { model: summary.model, explanationClaims: summary.document.claims.length };
}

/** Three persisted bins support reconsideration without changing the live enumeration cursor. */
async function reviewBins(page, instance) {
  assert.equal(await page.getByRole("button", { name: "Check candidate SQL locally", exact: true }).isEnabled(), false);
  await page.getByRole("button", { name: "Mark unsure", exact: true }).click();
  const unsure = page.getByRole("region", { name: "Unsure", exact: true });
  await unsure.getByRole("button", { name: "Open case", exact: true }).waitFor();
  assert.equal(await page.getByRole("button", { name: "Regenerate SQL", exact: true }).isEnabled(), false);
  await unsure.getByRole("button", { name: "Move to No", exact: true }).click();
  const no = page.getByRole("region", { name: "No", exact: true });
  const opened = page.waitForResponse(commandResponse("instances.open"));
  await no.getByRole("button", { name: "Open case", exact: true }).click();
  assert.equal((await (await opened).json()).status, "ok");
  await page.locator("#return-to-live-review").waitFor();
  assert.equal(await page.getByTestId("review-card").getAttribute("data-instance-id"), instance.instance_id);
  await page.locator("#return-to-live-review").click();
  assert.equal(await page.getByTestId("review-card").getAttribute("data-instance-id"), instance.instance_id);
}

/** A real candidate must remove the No outcome and pass the original Alloy assertion before adoption. */
async function checkRefinement(page) {
  const sql = "SELECT id, tenant_id FROM orders WHERE tenant_id = 1";
  const prompt = "Return orders for tenant 1 and preserve the original columns.";
  await page.getByLabel("Original prompt", { exact: true }).fill(prompt);
  await page.getByLabel("Candidate SQL", { exact: true }).fill(sql);
  const checked = page.waitForResponse(commandResponse("refinement.check"), { timeout: 180_000 });
  await page.getByRole("button", { name: "Check candidate SQL locally", exact: true }).click();
  const result = await (await checked).json();
  assert.equal(result.status, "ok", result.message);
  assert.equal(result.ready, true);
  assert.equal(result.property_check.status, "NO_COUNTEREXAMPLE_WITHIN_BOUNDS");
  assert.equal(result.cases[0].status, "satisfied");
  await page.locator(".refinement-result").getByRole("button", { name: "Copy SQL", exact: true }).click();
  assert.equal(await page.evaluate(() => navigator.clipboard.readText()), sql);
  await page.getByRole("button", { name: "Use checked SQL as new draft", exact: true }).click();
  await page.getByRole("heading", { name: "Ready to check a new question" }).waitFor();
  assert.equal(await page.getByLabel("SQL query", { exact: true }).inputValue(), sql);
  assert.equal(await page.getByLabel("Original prompt", { exact: true }).inputValue(), prompt);
  assert.equal(await page.getByRole("region", { name: "No", exact: true }).locator("li").count(), 1);
  return { threeBins: true, unsureOnlyBlocked: true, archiveReopen: true, noRemoved: true, realAlloyCheck: true, promptRetained: true, clipboard: true };
}

/** Check real browser precision for i64::MAX and a structured SQL NULL through stored examples. */
async function exactNumbers(page) {
  await page.getByRole("button", { name: "03 / Analyze", exact: true }).click();
  const advanced = page.locator("details").filter({ has: page.getByLabel("Advanced analysis request") });
  if ((await advanced.getAttribute("open")) === null) await advanced.locator("summary").click();
  await page
    .getByLabel("Advanced analysis request")
    .fill(
      '{"sql":"SELECT $1 AS biggest, NULL::bigint AS absent","mode":"compare","catalog":{"tables":{},"search_path":["public"]},"execution":{"max_outcomes":256,"max_rows":256,"settings":{}},"parameters":{"1":{"type":"int64","value":9223372036854775807}}}',
    );
  await page.getByLabel("Analysis goal").selectOption("example");
  const payload = await analyze(page);
  const text = await page.getByTestId("review-card").textContent();
  assert.ok(text.includes("9223372036854775807"));
  assert.ok(text.includes("NULL"));
  return { schema: payload.record.instance.schema, exactInt64: true, typedNull: true };
}

/** Secrets must stay out of browser persistence; Disconnect clears the visible tab setting. */
async function checkDisconnect(page, apiKey) {
  const absent = await page.evaluate((key) => ![...Object.values(localStorage), ...Object.values(sessionStorage)].some((value) => value.includes(key)), apiKey);
  assert.equal(absent, true);
  await page.getByRole("button", { name: "Open explanation provider settings" }).click();
  await page.getByRole("button", { name: "Disconnect & clear", exact: true }).click();
  await page.getByRole("button", { name: "Open explanation provider settings" }).click();
  await page.getByRole("radio", { name: /OpenAI-compatible/ }).check();
  assert.equal(await page.getByLabel("API key (memory only)", { exact: true }).inputValue(), "");
  await page.getByRole("button", { name: "Cancel", exact: true }).click();
}

/** Return only sanitized verification metadata; no trace, request headers or credential file is produced. */
export async function verifyHost({ url, chromePath, apiKey }) {
  const browser = await chromium.launch({ executablePath: chromePath, headless: true });
  const page = await browser.newPage({ viewport: { width: 1440, height: 1000 } });
  await page.context().grantPermissions(["clipboard-read", "clipboard-write"], { origin: url });
  const errors = [];
  const external = [];
  page.on("pageerror", () => errors.push(true));
  page.on("request", (request) => {
    if (!request.url().startsWith(url) && /^https?:/.test(request.url())) external.push(true);
  });
  try {
    await page.goto(url, { waitUntil: "networkidle" });
    const result = await verifyFlows(page, apiKey);
    assert.equal(errors.length, 0, "Uncaught browser error");
    assert.equal(external.length, 0, "Browser requested a nonlocal destination");
    return result;
  } finally {
    await browser.close();
  }
}

/** Exercise the full review and adoption flow against a host with a fresh test-only store. */
async function verifyFlows(page, apiKey) {
  await page.getByRole("button", { name: "03 / Analyze", exact: true }).click();
  const first = await analyze(page);
  const next = await nextCase(page);
  const ai = apiKey ? await checkAi(page, apiKey, next.record.instance) : { skipped: "No in-memory credential supplied" };
  await reviewBins(page, next.record.instance);
  const refinement = await checkRefinement(page);
  const precision = await exactNumbers(page);
  if (apiKey) await checkDisconnect(page, apiKey);
  return { ...report(first.assurance_kind, precision, ai), refinement };
}

/** Retain reproducible facts without source contents, provider prose or credentials. */
function report(assurance, precision, ai) {
  return {
    schema: "sqlolla.host_browser_check.v1",
    status: "passed",
    startOpen: true,
    graphviz: true,
    explicitNext: true,
    assurance,
    precision,
    ai,
    browserExternalRequests: 0,
    uncaughtErrors: 0,
  };
}

// The command-line gate is offline with respect to AI; authorized live callers pass a key in memory.
if (typeof process !== "undefined" && process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const result = await verifyHost({
    url: process.env.SQLLOLLA_URL ?? "http://127.0.0.1:8787",
    chromePath: process.env.CHROME_PATH ?? "C:/Program Files/Google/Chrome/Application/chrome.exe",
  });
  console.log(JSON.stringify(result, null, 2));
}
