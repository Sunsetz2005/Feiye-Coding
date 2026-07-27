import { describe, expect, it } from "vitest";
import {
  capabilityAvailable,
  capabilityReason,
  capabilityState,
  hostCapability,
  type HostCapabilities,
} from "./api";

const legacy: HostCapabilities = {
  platform: "macos",
  finderSelection: true,
  speechRecognition: false,
  skillDraftSave: true,
};

describe("capabilityAvailable", () => {
  it("falls back to a legacy capability during migration", () => {
    expect(capabilityAvailable(legacy, "finderSelection")).toBe(true);
    expect(capabilityState(legacy, "speechRecognition")).toBe("unavailable");
  });

  it("prefers an explicit capability state over a legacy boolean", () => {
    const host: HostCapabilities = {
      ...legacy,
      capabilities: {
        finderSelection: { state: "needs_permission", reason: "Permission required" },
      },
    };
    expect(capabilityAvailable(host, "finderSelection", true)).toBe(false);
    expect(capabilityState(host, "finderSelection", true)).toBe(
      "needs_permission",
    );
    expect(capabilityReason(host, "finderSelection", true)).toBe(
      "Permission required",
    );
  });

  it("reads a serialized v2 capability and its reason", () => {
    const host = JSON.parse(
      JSON.stringify({
        ...legacy,
        version: 2,
        capabilities: {
          skillDraftSave: { state: "available", version: "1" },
          backgroundScheduler: {
            state: "unavailable",
            reason: "No persistent background scheduler is registered",
          },
        },
      }),
    ) as HostCapabilities;

    expect(hostCapability(host, "skillDraftSave")).toEqual({
      state: "available",
      version: "1",
    });
    expect(capabilityState(host, "backgroundScheduler")).toBe("unavailable");
    expect(capabilityReason(host, "backgroundScheduler")).toBe(
      "No persistent background scheduler is registered",
    );
  });

  it("hides unknown v2 capabilities instead of trusting a legacy override", () => {
    const host: HostCapabilities = {
      ...legacy,
      version: 2,
      capabilities: {},
    };

    expect(hostCapability(host, "futureCapability", true)).toBeUndefined();
    expect(capabilityState(host, "futureCapability", true)).toBeUndefined();
    expect(capabilityReason(host, "futureCapability", true)).toBeUndefined();
    expect(capabilityAvailable(host, "futureCapability", true)).toBe(false);
  });
});
