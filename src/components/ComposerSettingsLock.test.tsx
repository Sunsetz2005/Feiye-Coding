// @vitest-environment jsdom

import { cleanup, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { resolveContextUsageDisplay, INITIAL_CONTEXT_USAGE } from "@/lib/contextUsage";
import { ContextUsageChip, type ContextUsageChipLabels } from "./ContextUsageChip";
import { ComposerAccessMenu } from "./ComposerModelMenu";

afterEach(cleanup);

const contextLabels: ContextUsageChipLabels = {
  aria: "Context",
  menuTitle: "Context window",
  used: "Used",
  remaining: "Remaining",
  total: "Total",
  latestInput: "Input",
  latestOutput: "Output",
  cacheRead: "Cache",
  reasoning: "Reasoning",
  model: "Model",
  modelCalls: "Calls",
  exactSource: "Runtime",
  updatedAt: "Updated",
  waiting: "Waiting",
  capacityUnknown: "Unknown",
  lastCompact: "Last compact",
  lastCompactNone: "None",
  tokensRange: "{before} → {after}",
  compactAction: "Compact now",
  auto: "Auto",
  manual: "Manual",
  hoverUsedRemain: "{used}% used ({remain}% remaining)",
  hoverTokens: "Used {used} tokens, {total} total",
};

const accessLabels = {
  access: "Access",
  accessHint: "Runtime settings",
  permission: "Permission",
  policyAsk: "Ask",
  policyAcceptEdits: "Accept edits",
  policySession: "Allow for session",
  policyDontAsk: "Do not ask",
  policyYolo: "Full access",
  policyAskDesc: "Ask each time",
  policyAcceptEditsDesc: "Allow edits",
  policySessionDesc: "Allow this session",
  policyDontAskDesc: "Deny requests",
  policyYoloDesc: "Allow everything",
  policyShortAsk: "Ask",
  policyShortAccept: "Edits",
  policyShortSession: "Session",
  policyShortDontAsk: "Deny",
  policyShortYolo: "Full",
};

describe("composer settings lock", () => {
  it("shows an exact compact summary before opening detailed telemetry", () => {
    render(
      <ContextUsageChip
        display={resolveContextUsageDisplay(
          INITIAL_CONTEXT_USAGE,
          [],
          {
            usedTokens: 173_000,
            inputTokens: 171_000,
            outputTokens: 2_000,
            cachedReadTokens: 0,
            reasoningTokens: 0,
            turnInputTokens: 1_000,
            turnOutputTokens: 200,
            modelCalls: 1,
            modelId: "sunsetz-4.5",
            contextWindowTokens: 258_000,
            updatedAt: "2026-07-27T12:00:00Z",
            source: "runtime",
          },
        )}
        labels={contextLabels}
        onCompact={vi.fn()}
      />,
    );

    const tooltip = screen.getByRole("tooltip");
    expect(tooltip.textContent).toContain("Context window");
    expect(tooltip.textContent).toContain("67% used (33% remaining)");
    expect(tooltip.textContent).toContain("Used 173k tokens, 258k total");
    expect(
      screen.getByRole("button", { name: /Context: 67%/ }).getAttribute(
        "aria-describedby",
      ),
    ).toBe(tooltip.id);
  });

  it("keeps context inspectable while disabling compact", async () => {
    const user = userEvent.setup();
    const onCompact = vi.fn();
    render(
      <ContextUsageChip
        display={resolveContextUsageDisplay(INITIAL_CONTEXT_USAGE, [])}
        labels={contextLabels}
        compactDisabled
        onCompact={onCompact}
      />,
    );

    const trigger = screen.getByRole("button", { name: /Context/ });
    expect(trigger.hasAttribute("disabled")).toBe(false);
    await user.click(trigger);

    const dialog = await screen.findByRole("dialog", {
      name: "Context window",
    });
    const compact = within(dialog).getByRole("button", {
      name: "Compact now",
    }) as HTMLButtonElement;
    expect(compact.disabled).toBe(true);
    await user.click(compact);
    expect(onCompact).not.toHaveBeenCalled();
  });

  it("keeps access inspectable without allowing policy changes", async () => {
    const user = userEvent.setup();
    const onPolicy = vi.fn();
    render(
      <ComposerAccessMenu
        policy="ask"
        disabled
        labels={accessLabels}
        onPolicy={onPolicy}
      />,
    );

    const trigger = screen.getByRole("button", { name: "Access" });
    expect(trigger.hasAttribute("disabled")).toBe(false);
    await user.click(trigger);

    const dialog = await screen.findByRole("dialog", { name: "Access" });
    expect(within(dialog).queryByText("Plan")).toBeNull();
    const fullAccessButton = within(dialog)
      .getByText("Full access")
      .closest("button");
    expect(fullAccessButton?.getAttribute("aria-disabled")).toBe("true");

    await user.click(fullAccessButton!);
    expect(onPolicy).not.toHaveBeenCalled();
  });

  it("closes access settings after choosing a policy", async () => {
    const user = userEvent.setup();
    const onPolicy = vi.fn();
    render(
      <ComposerAccessMenu
        policy="ask"
        labels={accessLabels}
        onPolicy={onPolicy}
      />,
    );

    await user.click(screen.getByRole("button", { name: "Access" }));
    const dialog = await screen.findByRole("dialog", { name: "Access" });
    await user.click(
      within(dialog).getByText("Accept edits").closest("button")!,
    );

    expect(onPolicy).toHaveBeenCalledWith("accept_edits");
    expect(screen.queryByRole("dialog", { name: "Access" })).toBeNull();
  });
});
