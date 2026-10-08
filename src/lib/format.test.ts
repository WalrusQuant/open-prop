import { describe, expect, it } from "vitest";
import { dnpSentence, formatLine } from "./format";

describe("dnpSentence", () => {
  it("says nothing when every game was played", () => {
    expect(dnpSentence(0)).toBe("");
    expect(dnpSentence(Number.NaN)).toBe("");
  });

  it("counts one and many", () => {
    expect(dnpSentence(1)).toBe("1 game at 0 minutes (DNP) is left out.");
    expect(dnpSentence(3)).toBe("3 games at 0 minutes (DNP) are left out.");
  });
});

describe("formatLine", () => {
  it("drops a trailing .0 and rounds to one place", () => {
    expect(formatLine(20)).toBe("20");
    expect(formatLine(20.299999)).toBe("20.3");
    expect(formatLine(12.5)).toBe("12.5");
  });
});
