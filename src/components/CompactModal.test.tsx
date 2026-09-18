// @vitest-environment jsdom

import { useRef, useState } from "react";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { CompactModal } from "./CompactModal";

afterEach(cleanup);

function Harness({
  open = true,
  onClose = () => {},
  onSubmit = () => {},
}: {
  open?: boolean;
  onClose?: () => void;
  onSubmit?: (note: string) => void;
}) {
  const [note, setNote] = useState("");
  const noteRef = useRef<HTMLInputElement>(null);
  return (
    <CompactModal
      open={open}
      locale="en"
      note={note}
      noteRef={noteRef}
      onNoteChange={setNote}
      onClose={onClose}
      onSubmit={onSubmit}
    />
  );
}

describe("CompactModal", () => {
  it("does not render while closed", () => {
    render(<Harness open={false} />);
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("submits the current optional note", async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn();
    render(<Harness onSubmit={onSubmit} />);

    await user.type(
      screen.getByPlaceholderText("Optional note (what to keep)"),
      "Keep decisions",
    );
    await user.click(screen.getByRole("button", { name: "Compact" }));

    expect(onSubmit).toHaveBeenCalledWith("Keep decisions");
  });

  it("closes from cancel or the overlay but not the panel", () => {
    const onClose = vi.fn();
    const { container } = render(<Harness onClose={onClose} />);
    const dialog = screen.getByRole("dialog", { name: "Compact context" });
    const overlay = container.querySelector<HTMLElement>(".overlay")!;

    fireEvent.click(dialog);
    expect(onClose).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    fireEvent.click(overlay);

    expect(onClose).toHaveBeenCalledTimes(2);
  });
});
