// @vitest-environment jsdom

import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ComposerPlanModeButton } from "./ComposerPlanModeButton";

afterEach(cleanup);

describe("ComposerPlanModeButton", () => {
  it("exposes one plan-mode control and disables the mode on click", async () => {
    const user = userEvent.setup();
    const onDisable = vi.fn();

    render(
      <ComposerPlanModeButton label="Plan mode" onDisable={onDisable} />,
    );

    const button = screen.getByRole("button", { name: "Plan mode" });
    expect(button.querySelector(".chip--plan-active__bulb")).toBeTruthy();
    expect(button.querySelector(".chip--plan-active__close")).toBeTruthy();

    await user.click(button);
    expect(onDisable).toHaveBeenCalledTimes(1);
  });

  it("does not invoke the action while composer settings are locked", async () => {
    const user = userEvent.setup();
    const onDisable = vi.fn();

    render(
      <ComposerPlanModeButton
        label="Plan mode"
        disabled
        onDisable={onDisable}
      />,
    );

    await user.click(screen.getByRole("button", { name: "Plan mode" }));
    expect(onDisable).not.toHaveBeenCalled();
  });
});
