/** Exercise new model workflows without sending a paid provider call. */
import assert from "node:assert/strict";

const model = {
  id: "browser-new-shipping-model",
  mode: "analyze",
  sql: "SELECT elapsed FROM shipments",
  catalog: {
    search_path: ["public"],
    tables: {
      shipments: { name: "shipments", schema: "public", columns: [{ name: "elapsed", ty: "int32", nullable: false }] },
    },
  },
  domain: [
    { type: "int32", value: 4 },
    { type: "int32", value: 13 },
  ],
  bounds: { shipments: 1 },
  property: { kind: "max_rows", max: 0 },
  parameters: {},
  include_model: true,
};

/** Open the named disclosure without toggling an already open editor. */
export async function openDetails(page, summary) {
  const node = page.getByText(summary, { exact: true });
  if (!(await node.evaluate((element) => element.closest("details")?.open))) await node.click();
}

/** Enter only a table declaration through the production form. */
export async function declareTable(page, name, columns) {
  await openDetails(page, "Schema and rule");
  await page.getByLabel("New table name", { exact: true }).fill(name);
  await page.getByLabel("Column definitions", { exact: true }).fill(columns);
  await page.getByRole("button", { name: "Declare table", exact: true }).click();
}

/** Set up first-draft fixtures and ensure the browser sends only the selected disclosure. */
async function installDraftRoutes(page) {
  const base = {
    provider: "openai",
    status: "ok",
    model: "gpt-6.1-sol",
    authority: "untrusted_assistance",
    assumptions: [],
    validation: { status: "compiled" },
  };
  await page.route("**/api/ai/byot/model-draft", (route) => {
    assert.deepEqual(route.request().postDataJSON(), { instruction: "Check how shipments return elapsed time.", share_prompt: true });
    return route.fulfill({ json: { ...base, schema: "sqlolla.ai.model_draft.v1", sql: model.sql, model_request: model } });
  });
  await page.route("**/api/ai/byot/draft", (route) => {
    const body = route.request().postDataJSON();
    assert.equal(body.request.sql, "");
    assert.equal(body.share_schema, true);
    assert.deepEqual(body.request.catalog, model.catalog);
    return route.fulfill({ json: { ...base, schema: "sqlolla.ai.initial_draft.v1", sql: "SELECT elapsed FROM shipments WHERE elapsed = 4" } });
  });
}

/** Accept a suggested model, then use its own schema for a separate SQL draft and real analysis. */
export async function checkModelDraft(page, configureAi, analyze) {
  await installDraftRoutes(page);
  await configureAi(page, "synthetic-first-draft-key");
  await page.getByRole("button", { name: "Build with AI", exact: true }).click();
  await page.getByLabel("Describe the database and what you want to check").fill("Check how shipments return elapsed time.");
  const generate = page.getByRole("button", { name: "Generate database model", exact: true });
  assert.equal(await generate.isEnabled(), false);
  await page.getByText("Share my description with my AI provider.", { exact: true }).click();
  await generate.click();
  await page.getByRole("button", { name: "Use this model and query", exact: true }).click();
  assert.equal(await page.getByLabel("SQL query", { exact: true }).inputValue(), model.sql);
  assert.equal(await page.getByTestId("review-card").count(), 0);
  await page.getByRole("button", { name: "Build with AI", exact: true }).click();
  await page.getByLabel("What should the query return?").fill("Return shipments that took four.");
  await page.getByText("Share my prompt, schema and parameter types with my AI provider.", { exact: true }).click();
  await page.getByRole("button", { name: "Generate SQL draft", exact: true }).click();
  await page.getByRole("button", { name: "Use this SQL draft", exact: true }).click();
  const solved = await analyze(page);
  assert.deepEqual(Object.keys(solved.bundle.request.catalog.tables), ["shipments"]);
  assert.equal(solved.bundle.request.catalog.tables.shipments.columns[0].name, "elapsed");
  assert.equal(solved.bundle.request.sql, "SELECT elapsed FROM shipments WHERE elapsed = 4");
  await page.unroute("**/api/ai/byot/model-draft");
  await page.unroute("**/api/ai/byot/draft");
}

/** Verify the live model identity and actual host compilation before staging. */
async function generateGemini(page, action, button) {
  const response = page.waitForResponse((item) => item.url().endsWith("/api/ai/byot/" + action), { timeout: 120_000 });
  await page.getByRole("button", { name: button, exact: true }).click();
  const received = await response;
  const draft = await received.json();
  assert.equal(received.status(), 200, `Gemini request returned ${draft.code ?? received.status()}`);
  assert.equal(draft.provider, "gemini");
  assert.equal(draft.model, "gemini-4-hydrogen");
  assert.equal(draft.validation.status, "compiled", draft.validation.message);
  return draft;
}

/** Start blank, stage a live Gemini model and query, then edit them before the real solver check. */
export async function checkGeminiDraft(page, configureAi, analyze) {
  await configureAi();
  const build = page.getByRole("button", { name: "Build with AI", exact: true });
  await build.click();
  await page
    .getByLabel("Describe the database and what you want to check")
    .fill(
      "Model inventory(item_id int32, units int32) with at most one row and values 0, 1, and 5. Draft SELECT item_id, units FROM inventory and check that at most zero rows are returned.",
    );
  await page.getByText("Share my description with my AI provider.", { exact: true }).click();
  const draft = await generateGemini(page, "model-draft", "Generate database model");
  assert.equal(draft.model_request.catalog.tables.inventory?.name, "inventory");
  await page.getByRole("button", { name: "Use this model and query", exact: true }).click();
  const editor = page.getByLabel("SQL query", { exact: true });
  assert.equal(await editor.inputValue(), draft.sql);
  await build.click();
  await page.getByLabel("What should the query return?").fill("Return item_id and units for inventory rows where units equals 5.");
  await page.getByText("Share my prompt, schema and parameter types with my AI provider.", { exact: true }).click();
  await generateGemini(page, "draft", "Generate SQL draft");
  await page.getByRole("button", { name: "Use this SQL draft", exact: true }).click();
  const sql = "SELECT item_id, units FROM inventory WHERE units = 5";
  await editor.fill(sql);
  const solved = await analyze(page);
  assert.equal(solved.bundle.request.sql, sql);
  assert.equal(solved.record.instance.kind, "counterexample");
  return { blankWorkbench: true, modelAndSqlCompiled: true, editedSqlCounterexample: true };
}
