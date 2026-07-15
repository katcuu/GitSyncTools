import { describe, expect, it } from "vitest";
import { duplicateTopNames, topName } from "./pathUtils";

describe("path utilities", () => {
  it("reads Windows and macOS top-level names", () => {
    expect(topName("C:\\Users\\me\\说明.txt")).toBe("说明.txt");
    expect(topName("/Users/me/Documents/report.pdf")).toBe("report.pdf");
  });

  it("finds case-insensitive duplicate target names", () => {
    const duplicates = duplicateTopNames([
      "C:\\one\\Report.docx",
      "D:\\two\\report.docx",
      "C:\\one\\photo.png",
    ]);
    expect([...duplicates]).toEqual(["report.docx"]);
  });
});
