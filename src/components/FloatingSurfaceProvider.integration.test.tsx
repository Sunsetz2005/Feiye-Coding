// @vitest-environment jsdom

import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { afterEach, describe, expect, it } from "vitest";
import {
  FloatingSurfaceProvider,
  useFloatingSurfacePresence,
} from "./FloatingSurfaceProvider";

function TestSurface({ id, label }: { id: string; label: string }) {
  const [open, setOpen] = useState(false);
  useFloatingSurfacePresence({
    id,
    open,
    onRequestClose: () => setOpen(false),
  });

  return (
    <button
      type="button"
      aria-label={label}
      aria-expanded={open}
      onClick={() => setOpen(true)}
    >
      {label}
    </button>
  );
}

afterEach(cleanup);

describe("FloatingSurfaceProvider", () => {
  it("closes the previous controlled surface when another one opens", async () => {
    const user = userEvent.setup();
    render(
      <FloatingSurfaceProvider>
        <TestSurface id="model" label="Model" />
        <TestSurface id="context" label="Context" />
      </FloatingSurfaceProvider>,
    );

    const model = screen.getByRole("button", { name: "Model" });
    const context = screen.getByRole("button", { name: "Context" });

    await user.click(model);
    expect(model.getAttribute("aria-expanded")).toBe("true");

    await user.click(context);
    expect(model.getAttribute("aria-expanded")).toBe("false");
    expect(context.getAttribute("aria-expanded")).toBe("true");
  });
});
