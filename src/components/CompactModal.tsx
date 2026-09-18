import type { RefObject } from "react";
import { IconClose } from "@/components/icons";
import { createT, type Locale } from "@/i18n";

export interface CompactModalProps {
  open: boolean;
  locale: Locale;
  note: string;
  noteRef: RefObject<HTMLInputElement | null>;
  onNoteChange: (note: string) => void;
  onClose: () => void;
  onSubmit: (note: string) => void;
}

export function CompactModal({
  open,
  locale,
  note,
  noteRef,
  onNoteChange,
  onClose,
  onSubmit,
}: CompactModalProps) {
  if (!open) return null;
  const tr = createT(locale);

  return (
    <div className="overlay" role="presentation" onClick={onClose}>
      <form
        className="modal compact-modal"
        onClick={(event) => event.stopPropagation()}
        role="dialog"
        aria-modal="true"
        aria-labelledby="compact-modal-title"
        onSubmit={(event) => {
          event.preventDefault();
          onSubmit(note);
        }}
      >
        <header className="modal-head">
          <h2 id="compact-modal-title" className="modal-title">
            {tr("slash.compact")}
          </h2>
          <button
            type="button"
            className="icon-btn modal-close"
            onClick={onClose}
            aria-label={tr("common.close")}
          >
            <IconClose size={16} />
          </button>
        </header>
        <p className="compact-modal__msg">{tr("slash.compactConfirm")}</p>
        <input
          ref={noteRef}
          className="compact-modal__field"
          value={note}
          onChange={(event) => onNoteChange(event.target.value)}
          placeholder={tr("slash.compactNote")}
          autoFocus
          autoComplete="off"
        />
        <div className="modal-actions">
          <button type="button" className="btn btn--ghost" onClick={onClose}>
            {tr("slash.compactConfirmCancel")}
          </button>
          <button type="submit" className="btn btn--solid">
            {tr("slash.compactConfirmOk")}
          </button>
        </div>
      </form>
    </div>
  );
}
