import { useEffect, useRef, useState, type Dispatch, type RefObject, type SetStateAction } from "react";

/** In-app dialogs — window.prompt/confirm are unreliable in Tauri WebView. */
export type AppDialog =
  | {
      kind: "confirm";
      title: string;
      message: string;
      confirmLabel?: string;
      danger?: boolean;
      onConfirm: () => void | Promise<void>;
    }
  | {
      kind: "prompt";
      title: string;
      initial: string;
      placeholder?: string;
      onSubmit: (value: string) => void | Promise<void>;
    }
  | {
      kind: "edit-project";
      title: string;
      projectId: string;
      name: string;
      path: string;
      onSubmit: (name: string, path: string) => void | Promise<void>;
    }
  | null;

export interface UseAppDialogApi {
  appDialog: AppDialog;
  setAppDialog: Dispatch<SetStateAction<AppDialog>>;
  dialogInput: string;
  setDialogInput: Dispatch<SetStateAction<string>>;
  dialogPath: string;
  setDialogPath: Dispatch<SetStateAction<string>>;
  dialogInputRef: RefObject<HTMLInputElement | null>;
  confirmBtnRef: RefObject<HTMLButtonElement | null>;
  /** Latest dialog for Enter/Escape handlers (avoids stale chained confirms). */
  appDialogRef: RefObject<AppDialog>;
}

export function useAppDialog(): UseAppDialogApi {
  const [appDialog, setAppDialog] = useState<AppDialog>(null);
  const [dialogInput, setDialogInput] = useState("");
  const [dialogPath, setDialogPath] = useState("");
  const dialogInputRef = useRef<HTMLInputElement>(null);
  const confirmBtnRef = useRef<HTMLButtonElement>(null);
  const appDialogRef = useRef<AppDialog>(null);
  appDialogRef.current = appDialog;

  useEffect(() => {
    if (!appDialog) return;
    if (appDialog.kind === "prompt") {
      setDialogInput(appDialog.initial);
      const t = window.setTimeout(() => {
        dialogInputRef.current?.focus();
        dialogInputRef.current?.select();
      }, 0);
      return () => window.clearTimeout(t);
    }
    if (appDialog.kind === "edit-project") {
      setDialogInput(appDialog.name);
      setDialogPath(appDialog.path);
      const t = window.setTimeout(() => {
        dialogInputRef.current?.focus();
        dialogInputRef.current?.select();
      }, 0);
      return () => window.clearTimeout(t);
    }
    // Confirm: focus primary action so keyboard users land on Confirm.
    // Enter is also handled globally below so it still confirms if focus
    // sits on Cancel / close (needed for multi-step YOLO Enter spam).
    if (appDialog.kind === "confirm") {
      const t = window.setTimeout(() => {
        confirmBtnRef.current?.focus();
      }, 0);
      return () => window.clearTimeout(t);
    }
  }, [appDialog]);

  useEffect(() => {
    if (!appDialog) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        setAppDialog(null);
        return;
      }
      // Confirm dialogs: Enter always accepts (including chained YOLO steps).
      // Capture phase + preventDefault so we don't double-fire with a focused
      // submit button's native activation.
      if (e.key !== "Enter" && e.key !== "NumpadEnter") return;
      if (e.isComposing || e.altKey || e.ctrlKey || e.metaKey) return;
      const dialog = appDialogRef.current;
      if (!dialog || dialog.kind !== "confirm") return;
      e.preventDefault();
      e.stopPropagation();
      const run = dialog.onConfirm;
      setAppDialog(null);
      void run();
    };
    document.addEventListener("keydown", onKey, true);
    return () => document.removeEventListener("keydown", onKey, true);
  }, [appDialog]);

  return {
    appDialog,
    setAppDialog,
    dialogInput,
    setDialogInput,
    dialogPath,
    setDialogPath,
    dialogInputRef,
    confirmBtnRef,
    appDialogRef,
  };
}
