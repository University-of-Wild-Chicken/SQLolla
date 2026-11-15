import { describe, expect, it } from "vitest";
import { BUILDER_CATALOG } from "../analysis/fixtures";
import { createDraft, draftReducer, type DraftState } from "./model";
import { draftErrors, parameterSummary, readablePlan, renderDraftSql } from "./render";

function baseDraft(): DraftState {
  return createDraft(BUILDER_CATALOG);
}

describe("local draft builder", () => {
  it("renders a parameterized draft without claiming validation", () => {
    const state = baseDraft();
    const sql = renderDraftSql(state);
    expect(sql).toContain("SELECT");
    expect(sql).toContain("FROM public.orders");
    expect(sql).toContain("WHERE");
    expect(sql).toContain("tenant_id = $1");
    expect(sql).toContain("LIMIT 50");
    expect(sql).not.toContain("DISTINCT");
    expect(sql).not.toContain("= 7");
    expect(parameterSummary(state.parameters)).toContain("$1 : int32 = 7");
    expect(readablePlan(state).some((step) => step.title === "Filter rows")).toBe(true);
    expect(draftErrors(state)).toEqual([]);
  });

  it("preserves parameter identity and explicit typed NULL semantics", () => {
    const withNull = draftReducer(baseDraft(), {
      type: "update-parameter",
      number: 1,
      patch: { type: "null", nullType: "int32" },
    })!;
    expect(renderDraftSql(withNull)).toContain("tenant_id = $1");
    expect(parameterSummary(withNull.parameters)).toContain("$1 : typed NULL = NULL::int32");
    const filterId = withNull.filters[0]!.id;
    const literalNull = draftReducer(withNull, {
      type: "update-filter",
      id: filterId,
      patch: { source: "literal", literalType: "null", literalNullType: "int32" },
    })!;
    expect(renderDraftSql(literalNull)).toContain("tenant_id = CAST(NULL AS integer)");
  });

  it("adds a join card and only renders it once both sides are complete", () => {
    const withTable = draftReducer(baseDraft(), {
      type: "add-table",
      tableId: "public.customers",
    })!;
    expect(draftErrors(withTable).join(" ")).toContain("Complete the join");
    expect(renderDraftSql(withTable)).toBeNull();
    const joined = draftReducer(withTable, {
      type: "update-join",
      tableId: "public.customers",
      patch: { kind: "left", leftColumn: "customer_id", rightColumn: "customer_id" },
    })!;
    const sql = renderDraftSql(joined);
    expect(sql).toContain("LEFT JOIN public.customers");
    expect(sql).toContain("ON o.customer_id = c.customer_id");
    expect(draftErrors(joined)).toEqual([]);
  });

  it("validates structural limits and integer values without semantic compilation", () => {
    const tooLarge = draftReducer(baseDraft(), { type: "limit", limit: "501" })!;
    expect(draftErrors(tooLarge).join(" ")).toContain("0 to 500");
    const invalidInt = draftReducer(baseDraft(), {
      type: "update-parameter",
      number: 1,
      patch: { value: "seven" },
    })!;
    expect(draftErrors(invalidInt).join(" ")).toContain("integer");
    const reset = draftReducer(tooLarge, { type: "reset" });
    expect(reset).toBeNull();
    const noTable = createDraft({ searchPath: [], tables: {}, raw: null });
    expect(draftErrors(noTable).join(" ")).toContain("at least one table");
  });
});
