import {
  useEffect,
  useMemo,
  useRef,
  useState,
  type ChangeEvent,
} from "react";
import {
  IconClose,
  IconPlus,
  IconRefresh,
  IconTrash,
} from "@/components/icons";
import { useViewedMessages } from "@/entities/session";
import type { ChatMessage } from "@/lib/session";
import {
  SkillMaterialError,
  buildSkillGenerationRequest,
  buildSkillRangeOptions,
  defaultSkillMessageRange,
  validateSkillDraft,
  type SkillDraft,
  type SkillDraftReference,
  type SkillDraftScope,
  type SkillGenerationRequest,
  type SkillRangeOption,
} from "@/lib/skillDraft";
import "./skill-recorder-sheet.css";

export type SkillRecorderLabels = {
  title: string;
  close: string;
  sourceRange: string;
  start: string;
  end: string;
  userRole: string;
  assistantRole: string;
  activityRole: string;
  noMaterial: string;
  materialTooLarge: string;
  visibleRequest: string;
  generate: string;
  generating: string;
  regenerate: string;
  retry: string;
  draft: string;
  name: string;
  nameHint?: string;
  description: string;
  descriptionHint?: string;
  skillMd: string;
  skillMdHint?: string;
  references: string;
  addReference: string;
  removeReference: string;
  referencePath: string;
  referenceContent: string;
  noReferences: string;
  scope: string;
  projectScope: string;
  projectScopeHint?: string;
  userScope: string;
  userScopeHint?: string;
  save: string;
  saving: string;
  saved: string;
  validationFailed: string;
  overwriteTitle: string;
  overwriteBody: string;
  overwriteConfirm: string;
  overwriteCancel: string;
};

export type SkillRecorderSaveResult =
  | { status: "saved" }
  | { status: "conflict"; message?: string };

export interface SkillRecorderSheetProps {
  open: boolean;
  messages?: readonly ChatMessage[];
  projectPath?: string | null;
  labels: SkillRecorderLabels;
  /**
   * Optional externally generated draft. A new object replaces the local
   * editor once; subsequent local edits remain owned by this sheet.
   */
  draft?: SkillDraft | null;
  onGenerate?: (request: SkillGenerationRequest) => Promise<SkillDraft>;
  onSave: (
    draft: SkillDraft,
    scope: SkillDraftScope,
    overwrite: boolean,
  ) => Promise<SkillRecorderSaveResult | void>;
  onSaved?: (draft: SkillDraft, scope: SkillDraftScope) => void;
  onClose: () => void;
}

type ConflictState = {
  draft: SkillDraft;
  message: string;
} | null;

function cloneDraft(draft: SkillDraft): SkillDraft {
  return {
    ...draft,
    references: draft.references.map((reference) => ({ ...reference })),
  };
}

function rawError(error: unknown): string {
  if (error instanceof Error && error.message.trim()) return error.message;
  return String(error || "");
}

function isSaveConflict(error: unknown): boolean {
  if (!error || typeof error !== "object") return false;
  const value = error as { code?: unknown; status?: unknown };
  return (
    value.code === "SKILL_EXISTS" ||
    value.code === "ALREADY_EXISTS" ||
    value.status === "conflict"
  );
}

function replaceTemplate(
  template: string,
  values: Record<string, string | number>,
): string {
  let result = template;
  for (const [key, value] of Object.entries(values)) {
    result = result.replaceAll(`{${key}}`, String(value));
  }
  return result;
}

function roleLabel(
  option: SkillRangeOption,
  labels: SkillRecorderLabels,
): string {
  if (option.kind === "user") return labels.userRole;
  if (option.kind === "assistant") return labels.assistantRole;
  return labels.activityRole;
}

function rangeOptionLabel(
  option: SkillRangeOption,
  ordinal: number,
  labels: SkillRecorderLabels,
): string {
  return `${ordinal}. ${roleLabel(option, labels)} · ${option.preview}`;
}

function mapMaterialError(
  error: SkillMaterialError,
  labels: SkillRecorderLabels,
): string {
  if (error.code === "MATERIAL_TOO_LARGE") {
    return `${labels.materialTooLarge}\n${error.message}`;
  }
  if (error.code === "NO_VISIBLE_MESSAGES") return labels.noMaterial;
  return error.message;
}

export function SkillRecorderSheet({
  open,
  messages,
  projectPath = null,
  labels,
  draft = null,
  onGenerate,
  onSave,
  onSaved,
  onClose,
}: SkillRecorderSheetProps) {
  const storedMessages = useViewedMessages();
  const thread = messages ?? storedMessages;
  const sheetRef = useRef<HTMLElement>(null);
  const openerRef = useRef<HTMLElement | null>(null);
  const loadedDraftRef = useRef<SkillDraft | null>(null);
  const requestEpochRef = useRef(0);
  const openRef = useRef(open);
  const onCloseRef = useRef(onClose);

  const rangeOptions = useMemo(
    () => buildSkillRangeOptions(thread),
    [thread],
  );
  const defaultRange = useMemo(
    () => defaultSkillMessageRange(thread),
    [thread],
  );

  const [start, setStart] = useState(defaultRange?.start ?? -1);
  const [end, setEnd] = useState(defaultRange?.end ?? -1);
  const [workingDraft, setWorkingDraft] = useState<SkillDraft | null>(
    draft ? cloneDraft(draft) : null,
  );
  const [scope, setScope] = useState<SkillDraftScope>(
    projectPath ? "project" : "user",
  );
  const [generating, setGenerating] = useState(false);
  const [saving, setSaving] = useState(false);
  const [generateError, setGenerateError] = useState("");
  const [saveError, setSaveError] = useState("");
  const [savedMessage, setSavedMessage] = useState("");
  const [conflict, setConflict] = useState<ConflictState>(null);

  openRef.current = open;
  onCloseRef.current = onClose;

  useEffect(() => {
    if (!open) {
      loadedDraftRef.current = null;
      requestEpochRef.current += 1;
      return;
    }
    setStart((current) =>
      rangeOptions.some((option) => option.index === current)
        ? current
        : (defaultRange?.start ?? -1),
    );
    setEnd((current) =>
      rangeOptions.some((option) => option.index === current)
        ? current
        : (defaultRange?.end ?? -1),
    );
  }, [open, rangeOptions, defaultRange]);

  useEffect(() => {
    if (!open || !draft || draft === loadedDraftRef.current) return;
    loadedDraftRef.current = draft;
    setWorkingDraft(cloneDraft(draft));
    setGenerateError("");
    setSaveError("");
    setSavedMessage("");
    setConflict(null);
  }, [draft, open]);

  useEffect(() => {
    if (!open) return;
    setScope(projectPath ? "project" : "user");
    setGenerateError("");
    setSaveError("");
    setSavedMessage("");
    setConflict(null);
  }, [open, projectPath]);

  useEffect(() => {
    if (!open) return;
    const sheet = sheetRef.current;
    const active = document.activeElement;
    openerRef.current = active instanceof HTMLElement ? active : null;
    const frame = window.requestAnimationFrame(() => {
      sheet
        ?.querySelector<HTMLElement>("[data-skill-recorder-autofocus]")
        ?.focus({ preventScroll: true });
    });

    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape" || event.defaultPrevented) return;
      event.preventDefault();
      onCloseRef.current();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => {
      window.cancelAnimationFrame(frame);
      window.removeEventListener("keydown", onKeyDown);
      const focused = document.activeElement;
      if (
        !focused ||
        focused === document.body ||
        (focused instanceof Node && sheet?.contains(focused))
      ) {
        openerRef.current?.focus({ preventScroll: true });
      }
    };
  }, [open]);

  const rangeOrdinals = useMemo(() => {
    const map = new Map<number, number>();
    rangeOptions.forEach((option, index) => map.set(option.index, index + 1));
    return map;
  }, [rangeOptions]);

  const startOptions = rangeOptions.filter((option) => option.index <= end);
  const endOptions = rangeOptions.filter((option) => option.index >= start);
  const hasRange =
    start >= 0 &&
    end >= start &&
    rangeOptions.some((option) => option.index === start) &&
    rangeOptions.some((option) => option.index === end);
  const busy = generating || saving;

  const validation = useMemo(
    () => (workingDraft ? validateSkillDraft(workingDraft) : null),
    [workingDraft],
  );

  const changeRange = (nextStart: number, nextEnd: number) => {
    setStart(nextStart);
    setEnd(nextEnd);
    setWorkingDraft(null);
    setGenerateError("");
    setSaveError("");
    setSavedMessage("");
    setConflict(null);
  };

  const generate = async () => {
    if (!onGenerate || !hasRange || generating) return;
    const epoch = ++requestEpochRef.current;
    setGenerating(true);
    setGenerateError("");
    setSaveError("");
    setSavedMessage("");
    setConflict(null);

    try {
      const visibleRequest = replaceTemplate(labels.visibleRequest, {
        start: rangeOrdinals.get(start) ?? 1,
        end: rangeOrdinals.get(end) ?? rangeOptions.length,
        count: Math.max(
          1,
          (rangeOrdinals.get(end) ?? 1) -
            (rangeOrdinals.get(start) ?? 1) +
            1,
        ),
      });
      const request = buildSkillGenerationRequest({
        messages: thread,
        range: { start, end },
        visibleRequest,
      });
      const nextDraft = await onGenerate(request);
      const nextValidation = validateSkillDraft(nextDraft);
      if (!nextValidation.valid) {
        throw new Error(nextValidation.errors.join("\n"));
      }
      if (epoch !== requestEpochRef.current || !openRef.current) return;
      setWorkingDraft(cloneDraft(nextValidation.draft));
    } catch (error) {
      if (epoch !== requestEpochRef.current || !openRef.current) return;
      setGenerateError(
        error instanceof SkillMaterialError
          ? mapMaterialError(error, labels)
          : rawError(error),
      );
    } finally {
      if (epoch === requestEpochRef.current && openRef.current) {
        setGenerating(false);
      }
    }
  };

  const updateDraft = (update: (current: SkillDraft) => SkillDraft) => {
    setWorkingDraft((current) => (current ? update(current) : current));
    setSaveError("");
    setSavedMessage("");
    setConflict(null);
  };

  const updateReference = (
    index: number,
    update: Partial<SkillDraftReference>,
  ) => {
    updateDraft((current) => ({
      ...current,
      references: current.references.map((reference, referenceIndex) =>
        referenceIndex === index
          ? { ...reference, ...update }
          : reference,
      ),
    }));
  };

  const save = async (overwrite: boolean) => {
    if (!workingDraft || saving) return;
    const checked = validateSkillDraft(workingDraft);
    if (!checked.valid) {
      setSaveError(
        `${labels.validationFailed}\n${checked.errors.join("\n")}`,
      );
      return;
    }

    setSaving(true);
    setSaveError("");
    setSavedMessage("");
    try {
      const result = await onSave(checked.draft, scope, overwrite);
      if (result?.status === "conflict") {
        setConflict({
          draft: cloneDraft(checked.draft),
          message: result.message || labels.overwriteBody,
        });
        return;
      }
      setConflict(null);
      setSavedMessage(labels.saved);
      onSaved?.(checked.draft, scope);
    } catch (error) {
      if (isSaveConflict(error)) {
        setConflict({
          draft: cloneDraft(checked.draft),
          message: rawError(error) || labels.overwriteBody,
        });
      } else {
        setSaveError(rawError(error));
      }
    } finally {
      setSaving(false);
    }
  };

  return (
    <aside
      ref={sheetRef}
      className={`skill-recorder-sheet${open ? " is-open" : ""}`}
      aria-hidden={!open}
      aria-labelledby="skill-recorder-title"
      aria-busy={busy || undefined}
      inert={!open ? true : undefined}
      data-testid="skill-recorder-sheet"
    >
      <header className="skill-recorder-sheet__header">
        <h2 id="skill-recorder-title" className="skill-recorder-sheet__title">
          {labels.title}
        </h2>
        <button
          type="button"
          className="skill-recorder-sheet__icon-button"
          onClick={onClose}
          aria-label={labels.close}
          title={labels.close}
          data-skill-recorder-autofocus
        >
          <IconClose size={17} />
        </button>
      </header>

      <div className="skill-recorder-sheet__scroll">
        <section className="skill-recorder-sheet__section">
          <h3 className="skill-recorder-sheet__section-title">
            {labels.sourceRange}
          </h3>
          {defaultRange ? (
            <div className="skill-recorder-sheet__range">
              <label className="skill-recorder-sheet__field">
                <span className="skill-recorder-sheet__label">
                  {labels.start}
                </span>
                <select
                  value={start}
                  onChange={(event: ChangeEvent<HTMLSelectElement>) =>
                    changeRange(Number(event.target.value), end)
                  }
                  disabled={busy}
                >
                  {startOptions.map((option) => (
                    <option key={`${option.messageId}-${option.index}`} value={option.index}>
                      {rangeOptionLabel(
                        option,
                        rangeOrdinals.get(option.index) ?? 1,
                        labels,
                      )}
                    </option>
                  ))}
                </select>
              </label>
              <label className="skill-recorder-sheet__field">
                <span className="skill-recorder-sheet__label">
                  {labels.end}
                </span>
                <select
                  value={end}
                  onChange={(event: ChangeEvent<HTMLSelectElement>) =>
                    changeRange(start, Number(event.target.value))
                  }
                  disabled={busy}
                >
                  {endOptions.map((option) => (
                    <option key={`${option.messageId}-${option.index}`} value={option.index}>
                      {rangeOptionLabel(
                        option,
                        rangeOrdinals.get(option.index) ?? 1,
                        labels,
                      )}
                    </option>
                  ))}
                </select>
              </label>
            </div>
          ) : (
            <p className="skill-recorder-sheet__empty">{labels.noMaterial}</p>
          )}

          {onGenerate ? (
            <button
              type="button"
              className="skill-recorder-sheet__button skill-recorder-sheet__button--secondary"
              onClick={() => void generate()}
              disabled={!hasRange || busy}
            >
              <IconRefresh size={16} />
              {generating
                ? labels.generating
                : generateError
                  ? labels.retry
                  : workingDraft
                    ? labels.regenerate
                    : labels.generate}
            </button>
          ) : null}

          {generateError ? (
            <div className="skill-recorder-sheet__error" role="alert">
              {generateError}
            </div>
          ) : null}
        </section>

        {workingDraft ? (
          <>
            <section className="skill-recorder-sheet__section">
              <h3 className="skill-recorder-sheet__section-title">
                {labels.draft}
              </h3>
              <label className="skill-recorder-sheet__field">
                <span className="skill-recorder-sheet__label">
                  {labels.name}
                </span>
                <input
                  value={workingDraft.name}
                  onChange={(event) =>
                    updateDraft((current) => ({
                      ...current,
                      name: event.target.value,
                    }))
                  }
                  placeholder={labels.nameHint}
                  autoCapitalize="none"
                  autoCorrect="off"
                  spellCheck={false}
                  disabled={busy}
                />
              </label>
              <label className="skill-recorder-sheet__field">
                <span className="skill-recorder-sheet__label">
                  {labels.description}
                </span>
                <textarea
                  value={workingDraft.description}
                  onChange={(event) =>
                    updateDraft((current) => ({
                      ...current,
                      description: event.target.value,
                    }))
                  }
                  placeholder={labels.descriptionHint}
                  rows={3}
                  disabled={busy}
                />
              </label>
              <label className="skill-recorder-sheet__field">
                <span className="skill-recorder-sheet__label">
                  {labels.skillMd}
                </span>
                <textarea
                  className="skill-recorder-sheet__code"
                  value={workingDraft.skillMd}
                  onChange={(event) =>
                    updateDraft((current) => ({
                      ...current,
                      skillMd: event.target.value,
                    }))
                  }
                  placeholder={labels.skillMdHint}
                  rows={16}
                  spellCheck={false}
                  disabled={busy}
                />
              </label>
            </section>

            <section className="skill-recorder-sheet__section">
              <div className="skill-recorder-sheet__section-heading">
                <h3 className="skill-recorder-sheet__section-title">
                  {labels.references}
                </h3>
                <button
                  type="button"
                  className="skill-recorder-sheet__text-button"
                  onClick={() =>
                    updateDraft((current) => ({
                      ...current,
                      references: [
                        ...current.references,
                        { path: "references/", content: "" },
                      ],
                    }))
                  }
                  disabled={busy}
                >
                  <IconPlus size={15} />
                  {labels.addReference}
                </button>
              </div>

              {!workingDraft.references.length ? (
                <p className="skill-recorder-sheet__empty">
                  {labels.noReferences}
                </p>
              ) : (
                <div className="skill-recorder-sheet__references">
                  {workingDraft.references.map((reference, index) => (
                    <fieldset
                      key={`reference-${index}`}
                      className="skill-recorder-sheet__reference"
                    >
                      <legend>
                        {labels.references} {index + 1}
                      </legend>
                      <label className="skill-recorder-sheet__field">
                        <span className="skill-recorder-sheet__label">
                          {labels.referencePath}
                        </span>
                        <input
                          value={reference.path}
                          onChange={(event) =>
                            updateReference(index, {
                              path: event.target.value,
                            })
                          }
                          autoCapitalize="none"
                          autoCorrect="off"
                          spellCheck={false}
                          disabled={busy}
                        />
                      </label>
                      <label className="skill-recorder-sheet__field">
                        <span className="skill-recorder-sheet__label">
                          {labels.referenceContent}
                        </span>
                        <textarea
                          value={reference.content}
                          onChange={(event) =>
                            updateReference(index, {
                              content: event.target.value,
                            })
                          }
                          rows={6}
                          disabled={busy}
                        />
                      </label>
                      <button
                        type="button"
                        className="skill-recorder-sheet__text-button skill-recorder-sheet__text-button--danger"
                        onClick={() =>
                          updateDraft((current) => ({
                            ...current,
                            references: current.references.filter(
                              (_, referenceIndex) =>
                                referenceIndex !== index,
                            ),
                          }))
                        }
                        disabled={busy}
                        aria-label={`${labels.removeReference} ${index + 1}`}
                      >
                        <IconTrash size={15} />
                        {labels.removeReference}
                      </button>
                    </fieldset>
                  ))}
                </div>
              )}
            </section>

            <section className="skill-recorder-sheet__section">
              <h3 className="skill-recorder-sheet__section-title">
                {labels.scope}
              </h3>
              <div className="skill-recorder-sheet__scope" role="radiogroup">
                {projectPath ? (
                  <label className="skill-recorder-sheet__scope-option">
                    <input
                      type="radio"
                      name="skill-recorder-scope"
                      checked={scope === "project"}
                      onChange={() => setScope("project")}
                      disabled={busy}
                    />
                    <span>
                      <strong>{labels.projectScope}</strong>
                      {labels.projectScopeHint ? (
                        <small>{labels.projectScopeHint}</small>
                      ) : null}
                    </span>
                  </label>
                ) : null}
                <label className="skill-recorder-sheet__scope-option">
                  <input
                    type="radio"
                    name="skill-recorder-scope"
                    checked={scope === "user"}
                    onChange={() => setScope("user")}
                    disabled={busy}
                  />
                  <span>
                    <strong>{labels.userScope}</strong>
                    {labels.userScopeHint ? (
                      <small>{labels.userScopeHint}</small>
                    ) : null}
                  </span>
                </label>
              </div>
            </section>

            {validation && !validation.valid ? (
              <div
                className="skill-recorder-sheet__validation"
                role="status"
              >
                <strong>{labels.validationFailed}</strong>
                <ul>
                  {validation.errors.map((error, index) => (
                    <li key={`${index}-${error}`}>{error}</li>
                  ))}
                </ul>
              </div>
            ) : null}

            {saveError ? (
              <div className="skill-recorder-sheet__error" role="alert">
                {saveError}
              </div>
            ) : null}
            {savedMessage ? (
              <div className="skill-recorder-sheet__success" role="status">
                {savedMessage}
              </div>
            ) : null}

            {conflict ? (
              <section
                className="skill-recorder-sheet__conflict"
                aria-labelledby="skill-recorder-overwrite-title"
              >
                <h3 id="skill-recorder-overwrite-title">
                  {labels.overwriteTitle}
                </h3>
                <p>{conflict.message || labels.overwriteBody}</p>
                <div className="skill-recorder-sheet__conflict-actions">
                  <button
                    type="button"
                    className="skill-recorder-sheet__button skill-recorder-sheet__button--secondary"
                    onClick={() => setConflict(null)}
                    disabled={saving}
                  >
                    {labels.overwriteCancel}
                  </button>
                  <button
                    type="button"
                    className="skill-recorder-sheet__button skill-recorder-sheet__button--danger"
                    onClick={() => void save(true)}
                    disabled={saving}
                  >
                    {saving ? labels.saving : labels.overwriteConfirm}
                  </button>
                </div>
              </section>
            ) : null}
          </>
        ) : null}
      </div>

      {workingDraft && !conflict ? (
        <footer className="skill-recorder-sheet__footer">
          <button
            type="button"
            className="skill-recorder-sheet__button skill-recorder-sheet__button--primary"
            onClick={() => void save(false)}
            disabled={busy || !validation?.valid}
          >
            {saving ? labels.saving : labels.save}
          </button>
        </footer>
      ) : null}
    </aside>
  );
}
