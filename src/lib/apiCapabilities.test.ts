import { describe, expect, it } from "vitest";
import { capabilityAvailable, type HostCapabilities } from "./api";

const legacy: HostCapabilities = {
  platform: "macos",
  finderSelection: true,
  speechRecognition: false,
  skillDraftSave: true,
};

describe("capabilityAvailable", () => {
  it("falls back to a legacy capability during migration", () => {
    expect(capabilityAvailable(legacy, "finderSelection", legacy.finderSelection)).toBe(true);
  });

  it("prefers an explicit capability state over a legacy boolean", () => {
    const host: HostCapabilities = {
      ...legacy,
      capabilities: {
        finderSelection: { state: "needs_permission", reason: "Permission required" },
      },
    };
    expect(capabilityAvailable(host, "finderSelection", true)).toBe(false);
  });
});
