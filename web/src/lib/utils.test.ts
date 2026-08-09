import { describe, expect, it } from "vitest";
import { formatBytes, shortId, formatTime } from "./utils";

describe("formatBytes", () => {
  it("formats zero", () => expect(formatBytes(0)).toBe("0 B"));
  it("formats B/KB/MB/GB", () => {
    expect(formatBytes(512)).toBe("512 B");
    expect(formatBytes(3_412_480)).toBe("3.3 MB");
    expect(formatBytes(78_304_192)).toBe("74.7 MB");
    expect(formatBytes(5 * 1024 ** 3)).toBe("5.0 GB");
  });
});

describe("shortId", () => {
  it("truncates long ids and keeps short ones", () => {
    expect(shortId("7f3c9a2e-1b44-4c5d-9a2b")).toBe("7f3c9a2e…");
    expect(shortId("abc", 8)).toBe("abc");
  });
});

describe("formatTime", () => {
  it("renders a valid date string", () => {
    const out = formatTime(1_725_000_000);
    expect(out.length).toBeGreaterThan(5);
  });
});
