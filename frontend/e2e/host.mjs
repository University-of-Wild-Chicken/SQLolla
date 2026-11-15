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
async function assistance(page, kind, instance) {
  const response = page.waitForResponse((item) => item.url().endsWith(`/api/ai/${kind}`), { timeout: 180_000 });
  await page.getByRole("button", { name: kind === "explain" ? "Explain this case" : "Propose SQL", exact: true }).click();
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

/** A proposed repair is reanalyzed, then explicitly adopted as an unvalidated editor draft. */
async function checkAi(page, apiKey, instance) {
  await configureAi(page, apiKey);
  await page
    .getByLabel("Question or proposed change")
    .fill("Explain why this tenant-isolation counterexample violates the retained rule. Cite supplied row facts and mention finite bounds.");
  const summary = await assistance(page, "explain", instance);
  await page
    .getByLabel("Question or proposed change")
    .fill(
      "Preserve SELECT id, tenant_id FROM orders and add WHERE tenant_id = 1. Keep the original output columns and all context, parameters, property, bounds and domain unchanged.",
    );
  const proposal = await assistance(page, "propose", instance);
  assert.equal(proposal.validation.status, "ok");
  assert.equal(proposal.validation.analysis, "NO_COUNTEREXAMPLE_WITHIN_BOUNDS");
  await page.getByRole("button", { name: "Use as SQL draft", exact: true }).click();
  await page.getByRole("heading", { name: "Ready to check a new question" }).waitFor();
  assert.equal(await page.getByTestId("review-card").count(), 0);
  assert.equal(await page.getByLabel("SQL query", { exact: true }).inputValue(), proposal.document.sql);
  return { model: summary.model, explanationClaims: summary.document.claims.length, proposalAnalysis: proposal.validation.analysis };
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
  const errors = [];
  const external = [];
  page.on("pageerror", () => errors.push(true));
  page.on("request", (request) => {
    if (!request.url().startsWith(url) && /^https?:/.test(request.url())) external.push(true);
  });
  try {
    await page.goto(url, { waitUntil: "networkidle" });
    await page.getByRole("button", { name: "03 / Analyze", exact: true }).click();
    const first = await analyze(page);
    const next = await nextCase(page);
    const ai = apiKey ? await checkAi(page, apiKey, next.record.instance) : { skipped: "No in-memory credential supplied" };
    const precision = await exactNumbers(page);
    if (apiKey) await checkDisconnect(page, apiKey);
    assert.equal(errors.length, 0, "Uncaught browser error");
    assert.equal(external.length, 0, "Browser requested a nonlocal destination");
    return report(first.assurance_kind, precision, ai);
  } finally {
    await browser.close();
  }
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
