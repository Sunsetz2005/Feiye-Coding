// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { useRef } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { AppDialog } from "@/hooks/useAppDialog";
import { AppDialogHost } from "./AppDialogHost";

const pickDirectoryMock = vi.fn<() => Promise<string | null>>();

vi.mock("@/lib/api", () => ({
  pickDirectory: () => pickDirectoryMock(),
}));

afterEach(() => cleanup());

const tr = (key: string) => key;

function Harness({
  dialog,
  onClose,
}: {
  dialog: AppDialog;
  onClose?: () => void;
}) {
  const dialogInputRef = useRef<HTMLInputElement>(null);
  const confirmBtnRef = useRef<HTMLButtonElement>(null);
  const appDialogRef = useRef<AppDialog>(dialog);
  appDialogRef.current = dialog;
  return (
    <AppDialogHost
      appDialog={dialog}
      setAppDialog={() => onClose?.()}
      dialogInput={dialog?.kind === "edit-project" ? dialog.name : dialog?.kind === "prompt" ? dialog.initial : ""}
      setDialogInput={() => {}}
      dialogPath={dialog?.kind === "edit-project" ? dialog.path : ""}
      setDialogPath={() => {}}
      dialogInputRef={dialogInputRef}
      confirmBtnRef={confirmBtnRef}
      appDialogRef={appDialogRef}
      tr={tr as never}
    />
  );
}

describe("AppDialogHost", () => {
  it("renders nothing when appDialog is null", () => {
    render(<Harness dialog={null} />);
    expect(document.querySelector(".app-dialog")).toBeNull();
  });

  it("renders a confirm dialog and submits via the form", () => {
    const onConfirm = vi.fn();
    render(
      <Harness
        dialog={{
          kind: "confirm",
          title: "Delete project",
          message: "Are you sure?",
          confirmLabel: "Delete",
          danger: true,
          onConfirm,
        }}
      />,
    );
    expect(screen.getByText("Delete project")).toBeTruthy();
    expect(screen.getByText("Are you sure?")).toBeTruthy();
    fireEvent.submit(document.querySelector(".app-dialog__form")!);
    expect(onConfirm).toHaveBeenCalledTimes(1);
  });

  it("closes a confirm dialog via the Cancel button", () => {
    const onClose = vi.fn();
    render(
      <Harness
        dialog={{
          kind: "confirm",
          title: "Delete project",
          message: "Are you sure?",
          onConfirm: vi.fn(),
        }}
        onClose={onClose}
      />,
    );
    fireEvent.click(screen.getByText("common.cancel"));
    expect(onClose).toHaveBeenCalled();
  });

  it("closes a dialog via the header close button", () => {
    const onClose = vi.fn();
    render(
      <Harness
        dialog={{
          kind: "confirm",
          title: "Delete project",
          message: "Are you sure?",
          onConfirm: vi.fn(),
        }}
        onClose={onClose}
      />,
    );
    fireEvent.click(screen.getByLabelText("common.close"));
    expect(onClose).toHaveBeenCalled();
  });

  it("closes a dialog on overlay mousedown outside the modal", () => {
    const onClose = vi.fn();
    render(
      <Harness
        dialog={{
          kind: "confirm",
          title: "Delete project",
          message: "Are you sure?",
          onConfirm: vi.fn(),
        }}
        onClose={onClose}
      />,
    );
    fireEvent.mouseDown(document.querySelector(".app-dialog-overlay")!);
    expect(onClose).toHaveBeenCalled();
  });

  it("renders an edit-project dialog and submits name/path, and can pick a folder", async () => {
    const onSubmit = vi.fn();
    pickDirectoryMock.mockResolvedValue("/tmp/picked");
    render(
      <Harness
        dialog={{
          kind: "edit-project",
          title: "Edit project",
          projectId: "p1",
          name: "My Project",
          path: "/tmp/my-project",
          onSubmit,
        }}
      />,
    );
    fireEvent.click(screen.getByText("project.chooseFolder"));
    expect(pickDirectoryMock).toHaveBeenCalled();
    fireEvent.submit(document.querySelector(".app-dialog__form")!);
    expect(onSubmit).toHaveBeenCalledWith("My Project", "/tmp/my-project");
  });

  it("renders a prompt dialog and submits the current value", () => {
    const onSubmit = vi.fn();
    render(
      <Harness
        dialog={{
          kind: "prompt",
          title: "Rename",
          initial: "old-name",
          placeholder: "New name",
          onSubmit,
        }}
      />,
    );
    fireEvent.submit(document.querySelector(".app-dialog__form")!);
    expect(onSubmit).toHaveBeenCalledWith("old-name");
  });
});
