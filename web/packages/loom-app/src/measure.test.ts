// jsdom doesn't run layout, so `getBoundingClientRect` always returns
// zeros. We stub the prototype method per-test to feed measure.ts the
// pixel rect a real browser would produce — that way the actual math
// is exercised end-to-end (dividing the sample width, clamping
// non-finites, the cells-fit-viewport calculation), while the test
// still runs deterministically.

import { afterEach, describe, expect, test, vi } from "vitest";
import { cellsForViewport, measureCellSize } from "./measure.js";

afterEach(() => {
  vi.restoreAllMocks();
});

function stubBoundingRect(width: number, height: number): void {
  vi.spyOn(Element.prototype, "getBoundingClientRect").mockImplementation(
    () =>
      ({
        x: 0,
        y: 0,
        top: 0,
        left: 0,
        right: width,
        bottom: height,
        width,
        height,
        toJSON: () => ({}),
      }) as DOMRect,
  );
}

describe("measureCellSize", () => {
  test("divides probe width by the sample count to get cellWidth", () => {
    // 100 'M's wide = 800px → 8px per cell.
    stubBoundingRect(800, 16);
    const size = measureCellSize({
      fontFamily: "monospace",
      fontSize: "14px",
    });
    expect(size.cellWidth).toBeCloseTo(8, 6);
    expect(size.cellHeight).toBeCloseTo(16, 6);
  });

  test("honors a custom sampleSize", () => {
    stubBoundingRect(50, 12); // 50px / 10 samples = 5px / cell
    const size = measureCellSize({
      fontFamily: "monospace",
      fontSize: "10px",
      sampleSize: 10,
    });
    expect(size.cellWidth).toBeCloseTo(5, 6);
    expect(size.cellHeight).toBeCloseTo(12, 6);
  });

  test("removes the probe even when the bounding rect throws", () => {
    const before = document.body.childElementCount;
    // Force getBoundingClientRect to throw. The probe must still be
    // detached so the `finally` block runs to completion — otherwise
    // the next call would mount a second hidden probe on top.
    vi.spyOn(Element.prototype, "getBoundingClientRect").mockImplementation(
      () => {
        throw new Error("simulated layout failure");
      },
    );
    expect(() =>
      measureCellSize({ fontFamily: "monospace", fontSize: "14px" }),
    ).toThrow(/simulated layout failure/);
    expect(document.body.childElementCount).toBe(before);
  });

  test("zero-size probe throws (font not loaded yet)", () => {
    // The default jsdom layout returns zero — no need to stub.
    expect(() =>
      measureCellSize({ fontFamily: "monospace", fontSize: "14px" }),
    ).toThrow(/zero size/);
    // Probe must still be detached.
    expect(document.body.querySelector("div")).toBeNull();
  });

  test("non-finite probe size throws", () => {
    stubBoundingRect(Number.NaN, 16);
    expect(() =>
      measureCellSize({ fontFamily: "monospace", fontSize: "14px" }),
    ).toThrow(/non-finite/);
  });

  test("rounds non-integer sampleSize down and clamps to >= 1", () => {
    // 5px probe → cellWidth = 5 (sampleSize floors to 1).
    stubBoundingRect(5, 12);
    const size = measureCellSize({
      fontFamily: "monospace",
      fontSize: "10px",
      sampleSize: 1.7,
    });
    expect(size.cellWidth).toBeCloseTo(5, 6);
  });
});

describe("cellsForViewport", () => {
  test("floors to fit and clamps to at least one row/col", () => {
    const r = cellsForViewport(800, 600, { cellWidth: 8, cellHeight: 16 });
    expect(r.cols).toBe(100);
    expect(r.rows).toBe(37); // 600 / 16 = 37.5
  });

  test("clamps to 1 when container is smaller than a single cell", () => {
    const r = cellsForViewport(2, 2, { cellWidth: 8, cellHeight: 16 });
    expect(r.cols).toBe(1);
    expect(r.rows).toBe(1);
  });

  test("handles zero / negative container size by clamping to 1", () => {
    const r = cellsForViewport(0, -10, { cellWidth: 8, cellHeight: 16 });
    expect(r.cols).toBe(1);
    expect(r.rows).toBe(1);
  });
});
