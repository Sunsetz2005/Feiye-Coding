import { createPortal } from "react-dom";
import { IconClose } from "@/components/icons";
import type { UseAppDialogApi } from "@/hooks/useAppDialog";
import type { MessageKey, Vars } from "@/i18n";
import * as api from "@/lib/api";

type TranslateFn = (key: MessageKey, vars?: Vars) => string;

interface AppDialogHostProps extends UseAppDialogApi {
  tr: TranslateFn;
}

/** Renders the in-app confirm/prompt/edit-project modal (see useAppDialog). */
export function AppDialogHost({
  appDialog,
  setAppDialog,
  dialogInput,
  setDialogInput,
  dialogPath,
  setDialogPath,
  dialogInputRef,
  confirmBtnRef,
  appDialogRef,
  tr,
}: AppDialogHostProps) {
  if (!appDialog || typeof document === "undefined") return null;

  return createPortal(
    <div
      className="overlay app-dialog-overlay"
      role="presentation"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) setAppDialog(null);
      }}
    >
      <div
        className="modal app-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="app-dialog-title"
        onMouseDown={(e) => e.stopPropagation()}
      >
        <header className="modal-head">
          <h2 id="app-dialog-title" className="modal-title">
            {appDialog.title}
          </h2>
          <button
            type="button"
            className="icon-btn modal-close"
            onClick={() => setAppDialog(null)}
            aria-label={tr("common.close")}
          >
            <IconClose size={16} />
          </button>
        </header>
        {appDialog.kind === "confirm" ? (
          <form
            className="app-dialog__form"
            onSubmit={(e) => {
              e.preventDefault();
              // Prefer the keyboard path's latest ref so chained
              // dialogs (YOLO step1 → step2) stay consistent.
              const dialog = appDialogRef.current;
              if (!dialog || dialog.kind !== "confirm") return;
              const run = dialog.onConfirm;
              setAppDialog(null);
              void run();
            }}
          >
            <p className="app-dialog__msg">{appDialog.message}</p>
            <div className="app-dialog__actions modal-actions">
              <button
                type="button"
                className="btn btn--ghost"
                onClick={() => setAppDialog(null)}
              >
                {tr("common.cancel")}
              </button>
              <button
                ref={confirmBtnRef}
                type="submit"
                className={`btn ${appDialog.danger ? "btn--danger" : "btn--solid"}`}
              >
                {appDialog.confirmLabel || tr("common.confirm")}
              </button>
            </div>
          </form>
        ) : appDialog.kind === "edit-project" ? (
          <form
            className="app-dialog__form"
            onSubmit={(e) => {
              e.preventDefault();
              if (appDialog.kind !== "edit-project") return;
              const submit = appDialog.onSubmit;
              const name = dialogInput;
              const path = dialogPath;
              setAppDialog(null);
              void submit(name, path);
            }}
          >
            <label className="app-dialog__field">
              <span>{tr("project.name")}</span>
              <input
                ref={dialogInputRef}
                className="app-dialog__input"
                value={dialogInput}
                onChange={(e) => setDialogInput(e.target.value)}
                autoComplete="off"
              />
            </label>
            <label className="app-dialog__field">
              <span>{tr("project.folder")}</span>
              <div className="app-dialog__path-row">
                <input
                  className="app-dialog__input"
                  value={dialogPath}
                  onChange={(e) => setDialogPath(e.target.value)}
                  autoComplete="off"
                />
                <button
                  type="button"
                  className="btn btn--ghost"
                  onClick={() => {
                    void api.pickDirectory().then((picked) => {
                      if (picked) setDialogPath(picked);
                    });
                  }}
                >
                  {tr("project.chooseFolder")}
                </button>
              </div>
            </label>
            <div className="app-dialog__actions modal-actions">
              <button
                type="button"
                className="btn btn--ghost"
                onClick={() => setAppDialog(null)}
              >
                {tr("common.cancel")}
              </button>
              <button type="submit" className="btn btn--solid">
                {tr("common.save")}
              </button>
            </div>
          </form>
        ) : (
          <form
            className="app-dialog__form"
            onSubmit={(e) => {
              e.preventDefault();
              if (appDialog.kind !== "prompt") return;
              const value = dialogInput;
              const submit = appDialog.onSubmit;
              setAppDialog(null);
              void submit(value);
            }}
          >
            <input
              ref={dialogInputRef}
              className="app-dialog__input"
              value={dialogInput}
              placeholder={appDialog.placeholder}
              onChange={(e) => setDialogInput(e.target.value)}
              autoComplete="off"
            />
            <div className="app-dialog__actions modal-actions">
              <button
                type="button"
                className="btn btn--ghost"
                onClick={() => setAppDialog(null)}
              >
                {tr("common.cancel")}
              </button>
              <button type="submit" className="btn btn--solid">
                {tr("common.save")}
              </button>
            </div>
          </form>
        )}
      </div>
    </div>,
    document.body,
  );
}
