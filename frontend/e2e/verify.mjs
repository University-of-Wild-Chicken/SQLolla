/** Browser checks for the shipped workbench, deliberately without a solver or provider. */
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { chromium } from "playwright-core";
import { preview } from "vite";

const local = process.env.SQLLOLLA_URL ? null : await preview({ logLevel: "error", preview: { host: "127.0.0.1", port: 4173, strictPort: true } });
const origin = process.env.SQLLOLLA_URL ?? "http://127.0.0.1:4173";
const chrome = process.env.CHROME_PATH ?? "C:/Program Files/Google/Chrome/Application/chrome.exe";
const output = process.env.SQLLOLLA_SCREENSHOTS;
if (output) fs.mkdirSync(output, { recursive: true });

/** Check the first-visit choices, private key entry and reload without any assistance request. */
async function checkOnboarding(page, size) {
  const prompt = page.getByRole("region", { name: "Set up AI assistance", exact: true });
  await prompt.waitFor();
  if (size === "narrow") {
    await prompt.getByRole("button", { name: "Skip for now", exact: true }).click();
  } else {
    await prompt.getByRole("button", { name: "Set up AI", exact: true }).click();
    assert.equal(await page.getByLabel("Model identifier").count(), 0);
    assert.equal(await page.getByRole("dialog").getByRole("radio").count(), 3);
    await page.getByRole("radio", { name: "OpenAI", exact: true }).check();
    await page.getByLabel("API key (memory only)").fill("synthetic-browser-input");
    await page.getByRole("radio", { name: "Anthropic", exact: true }).check();
    assert.equal(await page.getByLabel("API key (memory only)").inputValue(), "");
    await page.getByRole("radio", { name: "Gemini", exact: true }).check();
    await page.getByLabel("API key (memory only)").fill("synthetic-browser-input");
    await page.getByRole("button", { name: "Save in this tab · no send" }).click();
    await page.getByRole("dialog").waitFor({ state: "detached" });
  }
  await page.reload({ waitUntil: "networkidle" });
  await page.getByText("Analysis server reachable", { exact: true }).waitFor();
  assert.equal(await prompt.count(), 0);
  assert.equal(await page.evaluate(() => JSON.stringify(localStorage).includes("synthetic-browser-input")), false);
  await page.getByRole("button", { name: "AI settings" }).click();
  await page.getByRole("radio", { name: "Gemini", exact: true }).check();
  assert.equal(await page.getByLabel("API key (memory only)").inputValue(), "");
  await page.getByRole("button", { name: "Cancel", exact: true }).click();
}

/** Verify startup without sending a solver command or provider request. */
async function checkWorkbench(page, size) {
  const requests = [];
  page.on("request", (request) => requests.push(new URL(request.url()).pathname));
  await page.route("**/api/status", (route) => route.fulfill({ json: { schema: "sqlolla.host.v1", model: "gpt-6.1-sol", csrf_token: "a".repeat(64) } }));
  await page.goto(origin, { waitUntil: "networkidle" });
  await page.getByRole("heading", { name: "Query workbench" }).waitFor();
  await page.getByText("Analysis server reachable", { exact: true }).waitFor();
  assert.equal(await page.getByTestId("review-card").count(), 0);
  assert.equal(await page.getByRole("button", { name: /scenario|demo terminal/i }).count(), 0);
  assert.equal(await page.getByRole("button", { name: "Analyze query", exact: true }).isEnabled(), true);
  assert.equal(await page.evaluate(() => document.documentElement.scrollWidth - innerWidth), 0, size + " overflow");
  assert.doesNotMatch(await page.locator("body").innerText(), /gpt-6\.1-sol/i);
  if (output) await page.screenshot({ path: path.join(output, size + ".png") });
  await checkAccessibility(page);
  await checkOnboarding(page, size);
  assert.equal(requests.filter((request) => request.startsWith("/api/ai/") || request === "/api/command").length, 0);
}

/** Check accessible names, colors and landmarks on the desktop and mobile starting screen. */
async function checkAccessibility(page) {
  const axe = fs.readFileSync(path.resolve("node_modules/axe-core/axe.min.js"), "utf8");
  await page.evaluate(axe);
  const failures = await page.evaluate(async () => {
    const result = await window.axe.run({ runOnly: ["wcag2a", "wcag2aa", "wcag21aa"] });
    return result.violations.map(({ id, nodes }) => ({ id, targets: nodes.map(({ target }) => target) }));
  });
  assert.deepEqual(failures, []);
}

/** The workbench opens static docs in a separate tab to preserve the user's draft. */
async function checkDocs(page, size) {
  const opened = page.waitForEvent("popup");
  await page.getByRole("link", { name: /^Docs / }).click();
  const docs = await opened;
  await docs.waitForLoadState("networkidle");
  assert.equal(new URL(docs.url()).pathname, "/docs/");
  await docs.getByRole("heading", { name: "Quick Start", exact: true }).waitFor();
  assert.equal(await docs.evaluate(() => document.documentElement.scrollWidth - innerWidth), 0, size + " docs overflow");
  await checkAccessibility(docs);
  if (output) await docs.screenshot({ path: path.join(output, "docs-" + size + ".png") });
  if (size === "desktop") await checkDocGuide(docs);
  await docs.close();
}

/** Verify the docs' examples, deep links, and expandable troubleshooting. */
async function checkDocGuide(docs) {
  await docs.context().grantPermissions(["clipboard-read", "clipboard-write"], { origin });
  const prompt = docs.locator("#inventory-prompt");
  await docs.getByRole("button", { name: "Copy inventory prompt", exact: true }).click();
  assert.equal(await docs.evaluate(() => navigator.clipboard.readText()), await prompt.textContent());
  await docs.getByRole("button", { name: "Copy inventory SQL", exact: true }).click();
  const query = await docs.evaluate(() => navigator.clipboard.readText());
  assert.equal(query.replaceAll("\r\n", "\n"), "SELECT item_id, units\nFROM inventory\nWHERE units = 5");
  const witness = docs.getByRole("table", { name: "Counterexample from the inventory demo" });
  assert.deepEqual(await witness.getByRole("row").filter({ hasText: "Query result" }).getByRole("cell").allTextContents(), ["5", "5"]);
  const troubleshooting = docs.getByRole("navigation", { name: "Documentation" }).getByRole("link", { name: "Troubleshooting" });
  await troubleshooting.click();
  await troubleshooting.and(docs.locator('[aria-current="location"]')).waitFor();
  await docs.getByText("Why is Add table disabled?", { exact: true }).click();
  await docs.getByText("The builder lists declared tables", { exact: false }).waitFor();
}

const browser = await chromium.launch({ executablePath: chrome, headless: true });
try {
  for (const [size, width, height] of [
    ["desktop", 1440, 1000],
    ["mobile", 390, 844],
    ["narrow", 320, 725],
  ]) {
    const page = await browser.newPage({ viewport: { width, height } });
    const errors = [];
    page.on("pageerror", (error) => errors.push(error.message));
    await checkWorkbench(page, size);
    await checkAccessibility(page);
    await checkDocs(page, size);
    assert.deepEqual(errors, []);
    await page.close();
    console.log("PASS", size, "workbench, docs, BYOT and accessibility");
  }
} finally {
  await browser.close();
  await local?.close();
}
