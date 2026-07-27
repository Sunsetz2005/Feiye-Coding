import { describe, expect, it } from "vitest";
import {
  isComposerSettingAction,
  rollbackOptimisticSetting,
  shouldLockComposerSettings,
} from "./composerSettings";

describe("shouldLockComposerSettings", () => {
  it.each(["connecting", "streaming", "awaiting_permission"] as const)(
    "locks Runtime settings while the session is %s",
    (state) => {
      expect(
        shouldLockComposerSettings({
          state,
          hasPermissionPrompt: false,
          hasAskUserPrompt: false,
          hasPlanReview: false,
        }),
      ).toBe(true);
    },
  );

  it("also locks permission, AskUser, and plan interactions", () => {
    for (const flag of [
      "hasPermissionPrompt",
      "hasAskUserPrompt",
      "hasPlanReview",
    ] as const) {
      expect(
        shouldLockComposerSettings({
          state: "ready",
          hasPermissionPrompt: flag === "hasPermissionPrompt",
          hasAskUserPrompt: flag === "hasAskUserPrompt",
          hasPlanReview: flag === "hasPlanReview",
        }),
      ).toBe(true);
    }
  });

  it("leaves idle settings editable", () => {
    expect(
      shouldLockComposerSettings({
        state: "ready",
        hasPermissionPrompt: false,
        hasAskUserPrompt: false,
        hasPlanReview: false,
      }),
    ).toBe(false);
  });
});

describe("optimistic composer settings", () => {
  it("rolls back only when the failed value is still current", () => {
    expect(rollbackOptimisticSetting("high", "high", "medium")).toBe(
      "medium",
    );
    expect(rollbackOptimisticSetting("low", "high", "medium")).toBe("low");
  });

  it("classifies only preference-changing + actions as locked", () => {
    expect(isComposerSettingAction("project")).toBe(true);
    expect(isComposerSettingAction("goal")).toBe(true);
    expect(isComposerSettingAction("plan")).toBe(true);
    expect(isComposerSettingAction("record-skill")).toBe(true);
    expect(isComposerSettingAction("finder")).toBe(false);
    expect(isComposerSettingAction("folder")).toBe(false);
  });
});
