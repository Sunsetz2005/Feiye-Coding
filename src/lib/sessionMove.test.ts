import { describe, expect, it } from "vitest";
import {
  sessionMoveAvailable,
  sessionMoveDestinations,
} from "./sessionMove";

describe("sessionMoveDestinations", () => {
  it("lists every project plus the no-project row", () => {
    expect(sessionMoveDestinations(["alpha", "beta"], "alpha")).toEqual([
      { projectId: "alpha", current: true },
      { projectId: "beta", current: false },
      { projectId: null, current: false },
    ]);
  });

  it("marks the no-project row current for orphan sessions", () => {
    expect(sessionMoveDestinations(["alpha"], null)).toEqual([
      { projectId: "alpha", current: false },
      { projectId: null, current: true },
    ]);
  });

  it("is unavailable when the only destination is already current", () => {
    expect(sessionMoveAvailable(sessionMoveDestinations([], null))).toBe(
      false,
    );
    expect(sessionMoveAvailable(sessionMoveDestinations(["alpha"], "alpha"))).toBe(
      true,
    );
  });
});
