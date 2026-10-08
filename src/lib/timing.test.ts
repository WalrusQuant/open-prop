import { afterEach, describe, expect, it, vi } from "vitest";
import { later } from "./timing";

describe("later", () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  it("runs right away with no delay", () => {
    const run = vi.fn();
    later(run, 0);
    expect(run).toHaveBeenCalledTimes(1);
  });

  it("waits, and a cancel drops the older call", () => {
    vi.useFakeTimers();
    const first = vi.fn();
    const second = vi.fn();
    const cancel = later(first, 200);
    vi.advanceTimersByTime(150);
    cancel();
    later(second, 200);
    vi.advanceTimersByTime(199);
    expect(second).not.toHaveBeenCalled();
    vi.advanceTimersByTime(1);
    expect(first).not.toHaveBeenCalled();
    expect(second).toHaveBeenCalledTimes(1);
  });
});
