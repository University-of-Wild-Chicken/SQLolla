/**
 * Browser verification for the local SQLolla frontend. Uses an installed Chrome
 * through playwright-core; it never calls a hosted analysis or provider API.
 */
import fs from "node:fs";
import path from "node:path";
import { chromium } from "playwright-core";
import { preview as vitePreview } from "vite";

const localServer = process.env.SQLLOLLA_URL ? null : await vitePreview({ logLevel: "error", preview: { host: "127.0.0.1", port: 4173, strictPort: true } });
const BASE = process.env.SQLLOLLA_URL ?? "http://127.0.0.1:4173";
const CHROME = process.env.CHROME_PATH ?? "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe";
const OUT = path.resolve("e2e/screenshots");
fs.mkdirSync(OUT, { recursive: true });

let failures = 0;
const check = (condition, message) => {
  if (condition) console.log(`PASS ${message}`);
  else {
    failures += 1;
    console.error(`FAIL ${message}`);
  }
};

const browser = await chromium.launch({ executablePath: CHROME, headless: true });
const context = await browser.newContext({ viewport: { width: 1440, height: 1000 } });
const page = await context.newPage();
const resetView = async () => {
  await page.evaluate(() => {
    document.documentElement.style.scrollBehavior = "auto";
    window.scrollTo({ top: 0, left: 0, behavior: "instant" });
    document.querySelector(".rail--left .rail-scroll")?.scrollTo({ top: 0, behavior: "instant" });
    document.querySelector(".rail--right .rail-scroll")?.scrollTo({ top: 0, behavior: "instant" });
  });
  await page.waitForTimeout(250);
};
const requests = [];
const pageErrors = [];
page.on("request", (request) => requests.push(request.url()));
page.on("pageerror", (error) => pageErrors.push(error.message));

await page.goto(BASE, { waitUntil: "networkidle" });
await page.getByRole("heading", { name: "Orders without a tenant filter" }).waitFor();
await page.locator(".graph-svg svg").waitFor({ timeout: 20000 });
await resetView();
await page.screenshot({ path: path.join(OUT, "desktop-initial.png") });
await page.screenshot({ path: path.join(OUT, "desktop-review.png") });

const external = requests.filter((url) => !url.startsWith(BASE) && !url.startsWith("data:") && !url.startsWith("blob:"));
check(external.length === 0, `no external network requests (${external.join(", ") || "none"})`);
check((await page.locator(".graph-svg svg").count()) === 1, "Graphviz rendered one sanitized SVG canvas");
check((await page.getByTestId("review-card").count()) === 1, "one review card is mounted");
check((await page.locator("script").count()) < 10, "page has no injected script payloads from evidence");

const inspect = page.getByRole("button", { name: "Inspect triple" });
check(await inspect.isEnabled(), "inspector is available once evidence is visible");
await inspect.click();
await page.getByRole("dialog").waitFor();
check(await page.getByRole("heading", { name: "Evidence inspector" }).isVisible(), "inspector opens the SQL/IR/Alloy drawer");
await page.getByRole("button", { name: "Close Evidence inspector" }).click();
await page.getByRole("dialog").waitFor({ state: "detached" });

const overflow = await page.evaluate(() => document.documentElement.scrollWidth - window.innerWidth);
check(overflow <= 1, `desktop layout has no horizontal overflow (${overflow}px)`);

const undo = page.getByRole("button", { name: /Undo label/i });
check(await undo.isDisabled(), "undo starts disabled without feedback");
await page.getByTestId("review-card").click({ position: { x: 40, y: 120 } });
await page.keyboard.press("a");
await page.waitForFunction(() => {
  const button = [...document.querySelectorAll("button")].find((item) => /Undo label/i.test(item.textContent ?? ""));
  return button && !button.disabled;
});
check(await undo.isEnabled(), "keyboard A appends an accepted label");
await page.keyboard.press("Control+z");
await page.waitForFunction(() => {
  const button = [...document.querySelectorAll("button")].find((item) => /Undo label/i.test(item.textContent ?? ""));
  return button && button.disabled;
});
check(await undo.isDisabled(), "keyboard undo appends a withdrawal and clears the active label");
await page.keyboard.press("r");
check(await undo.isEnabled(), "keyboard R appends an exclusion label");
await undo.click();
check(await undo.isDisabled(), "labelled Undo is equivalent to keyboard undo");

const beforeDrag = await page.getByTestId("review-card").getAttribute("data-instance-id");
const box = await page.locator(".graph-canvas").boundingBox();
if (box) {
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  await page.mouse.down();
  await page.mouse.move(box.x + box.width / 2 + 120, box.y + box.height / 2 + 10, { steps: 6 });
  await page.mouse.up();
}
check((await page.getByTestId("review-card").getAttribute("data-instance-id")) === beforeDrag, "graph drag does not advance the case");
check(await undo.isDisabled(), "graph drag does not create a review decision");

const firstId = beforeDrag;
await page.getByRole("button", { name: "Next counterexample" }).click();
await page.waitForFunction((id) => document.querySelector('[data-testid="review-card"]')?.getAttribute("data-instance-id") !== id, firstId);
check((await page.getByTestId("review-card").count()) === 1, "explicit Next replaces exactly one card");

await page.getByRole("button", { name: "Start a local draft" }).click();
await page.getByRole("heading", { name: "Draft awaiting deterministic analysis" }).waitFor();
check((await page.getByTestId("review-card").count()) === 0, "draft sets stored evidence aside");
const limit = page.getByLabel(/Result limit/i);
await limit.fill("42");
await page.getByRole("button", { name: /Check draft with deterministic compiler/i }).click();
await page.getByText("Analysis bridge unavailable").waitFor();
check((await page.getByText(/LIMIT 42/).count()) > 0, "draft SQL preview reflects the local limit");
check((await page.getByText("Analysis bridge unavailable").count()) === 1, "compiler gap is explicit and does not fake validation");
await resetView();
await page.screenshot({ path: path.join(OUT, "desktop-draft.png") });
await page.getByRole("button", { name: /Discard draft & return to evidence/i }).click();
await page.getByTestId("review-card").waitFor();

const openStates = async () => {
  if (!(await page.locator('[data-scenario-id="state-large"]').isVisible())) {
    await page.locator(".state-library summary").click();
  }
};
await openStates();
await page.locator('[data-scenario-id="state-large"]').click();
await page.locator(".partial-banner").waitFor();
check((await page.getByText(/Graph-view completeness/i).count()) === 1, "large graph exposes an honest partial-view banner");
await resetView();
await page.screenshot({ path: path.join(OUT, "desktop-partial.png") });

await openStates();
await page.locator('[data-scenario-id="state-sql-error"]').click();
await page.getByRole("heading", { name: "A modeled SQL rejection" }).waitFor();
check((await page.getByText(/23514/).count()) > 0, "SQL error fixture retains supplied diagnostics");
check((await page.getByTestId("review-card").count()) === 1, "SQL error evidence remains reviewable");

await page.emulateMedia({ reducedMotion: "reduce" });
const durations = await page.evaluate(() => {
  const element = document.querySelector(".site-header__scanline");
  return element ? [getComputedStyle(element).animationDuration, getComputedStyle(document.body).transitionDuration] : [];
});
check(Number.parseFloat(durations[0]) <= 0.001, `reduced motion disables animation (${durations[0]})`);

const axePath = path.resolve("node_modules/axe-core/axe.min.js");
if (fs.existsSync(axePath)) {
  await page.addScriptTag({ content: fs.readFileSync(axePath, "utf8") });
  const axe = await page.evaluate(async () => {
    const results = await window.axe.run(document, { runOnly: ["wcag2a", "wcag2aa"] });
    return results.violations.map((violation) => ({
      id: violation.id,
      impact: violation.impact,
      nodes: violation.nodes.map((node) => ({ target: node.target, html: node.html, summary: node.failureSummary })),
    }));
  });
  const severe = axe.filter((violation) => violation.impact === "serious" || violation.impact === "critical");
  console.log("AXE", JSON.stringify(axe, null, 2));
  check(severe.length === 0, `no serious/critical automated accessibility violations (${severe.map((item) => item.id).join(", ") || "none"})`);
}

const mobile = await browser.newContext({ viewport: { width: 390, height: 844 }, reducedMotion: "reduce" });
const narrow = await mobile.newPage();
await narrow.goto(BASE, { waitUntil: "networkidle" });
await narrow.getByTestId("review-card").waitFor();
const narrowOverflow = await narrow.evaluate(() => document.documentElement.scrollWidth - window.innerWidth);
check(narrowOverflow <= 1, `narrow layout has no horizontal overflow (${narrowOverflow}px)`);
await narrow.screenshot({ path: path.join(OUT, "narrow-main.png") });
await narrow.getByRole("button", { name: /Cases & draft/i }).click();
await narrow.getByRole("button", { name: /Close cases and builder panel/i }).waitFor();
await narrow.screenshot({ path: path.join(OUT, "narrow-library.png") });
await narrow.getByRole("button", { name: /Close cases and builder panel/i }).click();
await narrow.getByRole("button", { name: /^Explain/i }).click();
await narrow.getByRole("button", { name: /Close explanation and ledger panel/i }).waitFor();
await narrow.screenshot({ path: path.join(OUT, "narrow-explain.png") });
await mobile.close();

console.log("PAGE_ERRORS", JSON.stringify(pageErrors));
check(pageErrors.length === 0, "no uncaught page errors");
await browser.close();
await localServer?.close();
if (failures > 0) {
  console.error(`${failures} browser check(s) failed`);
  process.exit(1);
}
console.log("All browser checks passed");
