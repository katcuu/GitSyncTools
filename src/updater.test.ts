import { describe, expect, it } from "vitest";
import { friendlyUpdateError, updateProgress } from "./updater";

describe("updateProgress", () => {
  it("calculates and clamps download percentages", () => {
    expect(updateProgress(25, 100)).toBe(25);
    expect(updateProgress(150, 100)).toBe(100);
    expect(updateProgress(-10, 100)).toBe(0);
  });

  it("returns undefined when total size is unavailable", () => {
    expect(updateProgress(10)).toBeUndefined();
    expect(updateProgress(10, 0)).toBeUndefined();
  });
});

describe("friendlyUpdateError", () => {
  it("explains that a connection failure does not affect file sync", () => {
    const message = friendlyUpdateError("error sending request for url (https://github.com/latest.json)");
    expect(message).toContain("不影响文件同步");
    expect(message).toContain("GitHub");
  });

  it("explains missing and private releases", () => {
    expect(friendlyUpdateError("HTTP 404 Not Found")).toContain("尚未发布");
    expect(friendlyUpdateError("HTTP 403 Forbidden")).toContain("不允许匿名下载");
  });
});
