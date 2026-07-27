import { describe, expect, it, beforeAll } from "vitest";
import {
  computeCascadePos,
  computeFloatingPos,
  floatingStyle,
} from "./floatingMenu";

beforeAll(() => {
  Object.defineProperty(globalThis, "innerWidth", {
    value: 1024,
    configurable: true,
  });
  Object.defineProperty(globalThis, "innerHeight", {
    value: 768,
    configurable: true,
  });
});

function rect(
  partial: Partial<DOMRect> & {
    top: number;
    left: number;
    width: number;
    height: number;
  },
): DOMRect {
  const bottom = partial.top + partial.height;
  const right = partial.left + partial.width;
  return {
    x: partial.left,
    y: partial.top,
    top: partial.top,
    left: partial.left,
    width: partial.width,
    height: partial.height,
    bottom,
    right,
    toJSON: () => ({}),
  };
}

describe("computeFloatingPos", () => {
  it("prefers above when more space above", () => {
    const r = rect({ top: 600, left: 40, width: 120, height: 32 });
    const pos = computeFloatingPos(r, {
      placement: "auto",
      estHeight: 200,
      fitContent: false,
      width: 200,
    });
    expect(pos.placeAbove).toBe(true);
    expect(pos.top).toBeLessThan(r.top + 1);
  });

  it("honors placement down", () => {
    const r = rect({ top: 100, left: 40, width: 80, height: 28 });
    const pos = computeFloatingPos(r, {
      placement: "down",
      fitContent: false,
      width: 160,
    });
    expect(pos.placeAbove).toBe(false);
    expect(pos.top).toBeGreaterThan(r.bottom);
  });

  it("clamps left within viewport for fixed width", () => {
    const r = rect({ top: 100, left: 9000, width: 80, height: 28 });
    const pos = computeFloatingPos(r, {
      width: 200,
      fitContent: false,
      placement: "down",
    });
    expect(pos.left + pos.width).toBeLessThanOrEqual(1024);
  });

  it("matchTriggerWidth expands fixed panel", () => {
    const r = rect({ top: 100, left: 20, width: 280, height: 32 });
    const pos = computeFloatingPos(r, {
      width: 100,
      matchTriggerWidth: true,
      fitContent: false,
      placement: "down",
    });
    expect(pos.width).toBeGreaterThanOrEqual(280);
  });

  it("fitContent leaves width 0 (CSS max-content)", () => {
    const r = rect({ top: 100, left: 40, width: 80, height: 28 });
    const pos = computeFloatingPos(r, { placement: "down", fitContent: true });
    expect(pos.fitContent).toBe(true);
    expect(pos.width).toBe(0);
    expect(pos.maxWidth).toBeGreaterThan(100);
  });
});

describe("floatingStyle", () => {
  it("uses translateY for above placement", () => {
    const s = floatingStyle({
      left: 10,
      top: 100,
      width: 200,
      placeAbove: true,
      maxHeight: 200,
      maxWidth: 1000,
      fitContent: false,
    });
    expect(s?.transform).toContain("translateY(-100%)");
    expect(s?.position).toBe("fixed");
    expect(s?.width).toBe(200);
  });

  it("hides panel until settled to avoid open flash", () => {
    const s = floatingStyle(
      {
        left: 10,
        top: 100,
        width: 200,
        placeAbove: false,
        maxHeight: 200,
        maxWidth: 1000,
        fitContent: true,
      },
      { settled: false },
    );
    expect(s?.visibility).toBe("hidden");
    expect(s?.pointerEvents).toBe("none");
  });

  it("uses max-content when fitContent", () => {
    const s = floatingStyle(
      {
        left: 10,
        top: 100,
        width: 0,
        placeAbove: false,
        maxHeight: 200,
        maxWidth: 800,
        fitContent: true,
      },
      { minWidth: 120 },
    );
    expect(s?.width).toBe("max-content");
    expect(s?.minWidth).toBe(120);
    expect(s?.maxWidth).toBe(800);
  });
});

describe("computeCascadePos", () => {
  it("opens to the source row's right when it fits", () => {
    const r = rect({ top: 180, left: 120, width: 220, height: 38 });
    const pos = computeCascadePos(r, { width: 240, height: 180 });
    expect(pos.side).toBe("right");
    expect(pos.left).toBeGreaterThan(r.right);
    expect(pos.top).toBeLessThanOrEqual(r.top);
  });

  it("flips to the left near the viewport edge", () => {
    const r = rect({ top: 180, left: 820, width: 190, height: 38 });
    const pos = computeCascadePos(r, { width: 240, height: 180 });
    expect(pos.side).toBe("left");
    expect(pos.left + pos.width).toBeLessThan(r.left);
  });

  it("clamps an oversized cascade inside a zoomed viewport", () => {
    Object.defineProperty(globalThis, "innerWidth", {
      value: 360,
      configurable: true,
    });
    Object.defineProperty(globalThis, "innerHeight", {
      value: 240,
      configurable: true,
    });
    const r = rect({ top: 210, left: 160, width: 40, height: 24 });
    const pos = computeCascadePos(r, { width: 480, height: 500 });
    expect(pos.left).toBeGreaterThanOrEqual(8);
    expect(pos.left + pos.width).toBeLessThanOrEqual(352);
    expect(pos.top).toBeGreaterThanOrEqual(8);
    expect(pos.maxHeight).toBe(224);

    Object.defineProperty(globalThis, "innerWidth", {
      value: 1024,
      configurable: true,
    });
    Object.defineProperty(globalThis, "innerHeight", {
      value: 768,
      configurable: true,
    });
  });
});
