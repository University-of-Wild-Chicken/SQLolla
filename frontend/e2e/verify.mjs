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

/** Verify startup and the fixed-model BYOT form without transmitting even synthetic credentials. */
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
  if (output) await page.screenshot({ path: path.join(output, size + ".png") });
  await page.getByRole("button", { name: "AI settings" }).click();
  assert.equal(await page.getByLabel("Model identifier").inputValue(), "gpt-6.1-sol");
  await page.getByLabel("API key (memory only)").fill("synthetic-browser-input");
  await page.getByRole("button", { name: "Save in this tab · no send" }).click();
  await page.getByRole("dialog").waitFor({ state: "detached" });
  assert.equal(requests.filter((request) => request.startsWith("/api/ai/") || request === "/api/command").length, 0);
  assert.equal(await page.evaluate(() => JSON.stringify(localStorage).includes("synthetic-browser-input")), false);
}

/** Check accessible names, colors and landmarks on the desktop and mobile starting screen. */
async function checkAccessibility(page) {
  const axe = fs.readFileSync(path.resolve("node_modules/axe-core/axe.min.js"), "utf8");
  await page.addScriptTag({ content: axe });
  const failures = await page.evaluate(async () => {
    const result = await window.axe.run({ runOnly: ["wcag2a", "wcag2aa", "wcag21aa"] });
    return result.violations.map(({ id, nodes }) => ({ id, targets: nodes.map(({ target }) => target) }));
  });
  assert.deepEqual(failures, []);
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
    assert.deepEqual(errors, []);
    await page.close();
    console.log("PASS", size, "workbench, BYOT and accessibility");
  }
} finally {
  await browser.close();
  await local?.close();
}
