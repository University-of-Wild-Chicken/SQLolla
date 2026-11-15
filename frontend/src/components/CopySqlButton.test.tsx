import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { CopySqlButton } from "./CopySqlButton";

afterEach(() => vi.unstubAllGlobals());

describe("copy SQL", () => {
  const sql = "SELECT $1::int8\nFROM orders;";

  it("copies the exact current SQL after an explicit click", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    vi.stubGlobal("navigator", { clipboard: { writeText } });
    render(<CopySqlButton sql={sql} />);
    expect(writeText).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Copy SQL" }));
    expect(await screen.findByText("SQL copied.")).toBeInTheDocument();
    expect(writeText).toHaveBeenCalledExactlyOnceWith(sql);
  });

  it("announces clipboard failure and offers a selectable fallback", async () => {
    vi.stubGlobal("navigator", { clipboard: { writeText: vi.fn().mockRejectedValue(new Error("permission denied")) } });
    render(<CopySqlButton sql={sql} />);
    fireEvent.click(screen.getByRole("button", { name: "Copy SQL" }));
    const fallback = (await screen.findByRole("textbox", { name: "SQL to copy" })) as HTMLTextAreaElement;
    expect(screen.getByRole("status")).toHaveTextContent("Clipboard unavailable.");
    expect(fallback).toHaveValue(sql);
    fireEvent.focus(fallback);
    expect(fallback.selectionEnd).toBe(sql.length);
  });
});
