// @vitest-environment jsdom

import { createRef, type ComponentProps, type ReactNode } from "react";
import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { ComposerPlusEntry } from "@/components/ComposerPlusPanel";
import type { SlashItem } from "@/lib/slashCatalog";
import type { ComposerDockProps } from "./ComposerDock";

type EditorProps = ComponentProps<
  typeof import("@/components/ComposerEditor").ComposerEditor
>;
type AttachmentCardProps = ComponentProps<
  typeof import("@/components/AttachmentCard").AttachmentCard
>;
type PlusPanelProps = ComponentProps<
  typeof import("@/components/ComposerPlusPanel").ComposerPlusPanel
>;
type ProjectMenuProps = ComponentProps<
  typeof import("@/components/ComposerProjectMenu").ComposerProjectMenu
>;
type AccessMenuProps = ComponentProps<
  typeof import("@/components/ComposerModelMenu").ComposerAccessMenu
>;
type ModelMenuProps = ComponentProps<
  typeof import("@/components/ComposerModelMenu").ComposerModelMenu
>;
type PlanButtonProps = ComponentProps<
  typeof import("@/components/ComposerPlanModeButton").ComposerPlanModeButton
>;
type ContextChipProps = ComponentProps<
  typeof import("@/components/ContextUsageChip").ContextUsageChip
>;
type ProgressRailProps = ComponentProps<
  typeof import("@/components/lobe-chat/TaskProgressRail").TaskProgressRail
>;
type MemoryBadgeProps = ComponentProps<
  typeof import("@/components/MemoryContextBadge").MemoryContextBadge
>;

vi.mock("@/components/ui/tooltip", () => ({
  Tip: ({ children }: { children: ReactNode }) => children,
}));

vi.mock("@/components/ComposerEditor", () => ({
  ComposerEditor: ({
    editorRef,
    value,
    disabled,
    placeholder,
    onChange,
    onPasteFiles,
    onPasteMediaFallback,
    onSlashQueryChange,
    onKeyDown,
  }: EditorProps) => (
    <div>
      <div
        ref={editorRef}
        role="textbox"
        tabIndex={0}
        aria-disabled={disabled ? "true" : "false"}
        data-placeholder={placeholder}
        data-value={value}
        onKeyDown={onKeyDown}
      />
      <button type="button" onClick={() => onChange("edited")}>edit draft</button>
      <button type="button" onClick={() => onPasteFiles?.([new File(["x"], "x.txt")])}>
        paste files
      </button>
      <button type="button" onClick={() => onPasteMediaFallback?.({ expectMedia: true })}>
        paste fallback
      </button>
      <button
        type="button"
        onClick={() => onSlashQueryChange?.({ start: 0, query: "go", end: 3 })}
      >
        slash query
      </button>
    </div>
  ),
}));

vi.mock("@/components/AttachmentCard", () => ({
  AttachmentCard: ({
    attachment,
    galleryPaths,
    onRemove,
    onAddToComposer,
  }: AttachmentCardProps) => (
    <div data-testid={`attachment-${attachment.name}`} data-gallery={galleryPaths?.join("|")}>
      <span>{attachment.name}</span>
      <button type="button" onClick={() => onRemove?.(attachment)}>remove attachment</button>
      <button type="button" onClick={() => onAddToComposer?.(attachment)}>add attachment</button>
    </div>
  ),
}));

vi.mock("@/components/ComposerPlusPanel", () => ({
  ComposerPlusPanel: ({
    mode,
    panelRef,
    filterQuery,
    style,
    entries,
    onActiveIndexChange,
    onSelectUpload,
    onSelectAction,
    onSelectSlash,
  }: PlusPanelProps) => (
    <div
      ref={panelRef}
      role="menu"
      data-mode={mode}
      data-filter={filterQuery}
      data-z={style?.zIndex}
    >
      <button type="button" onClick={onSelectUpload}>panel upload</button>
      <button
        type="button"
        onClick={() => {
          const entry = entries.find((item) => item.kind === "action");
          if (entry?.kind === "action") onSelectAction?.(entry);
        }}
      >
        panel action
      </button>
      <button
        type="button"
        onClick={() => {
          const entry = entries.find((item) => item.kind === "slash");
          if (entry?.kind === "slash") onSelectSlash(entry.item);
        }}
      >
        panel slash
      </button>
      <button type="button" onClick={() => onActiveIndexChange(1)}>
        panel next
      </button>
    </div>
  ),
}));

vi.mock("@/components/ComposerProjectMenu", () => ({
  ComposerWorktreeMenu: () => null,
  ComposerProjectMenu: ({
    activeProject,
    projects,
    worktrees,
    disabled,
    instructionTip,
    onSelect,
    onAdd,
    onSwitchWorktree,
    onOpen,
  }: ProjectMenuProps) => (
    <div data-testid="project-menu" data-disabled={disabled ? "true" : "false"}>
      {instructionTip ? (
        <span data-testid="project-instruction-chip">{instructionTip}</span>
      ) : null}
      <span>{activeProject?.name ?? "No project"}</span>
      <button type="button" onClick={() => onSelect(projects[0] ?? null)}>select project</button>
      <button type="button" onClick={onAdd}>add project</button>
      <button type="button" onClick={() => worktrees?.[0] && onSwitchWorktree?.(worktrees[0])}>
        switch worktree
      </button>
      <button type="button" onClick={onOpen}>open project</button>
    </div>
  ),
}));

vi.mock("@/components/ComposerModelMenu", () => ({
  ComposerAccessMenu: ({ onPolicy }: AccessMenuProps) => (
    <div>
      <button type="button" onClick={() => onPolicy("always_approve")}>choose policy</button>
    </div>
  ),
  ComposerModelMenu: ({ onModel, onEffort, onReset }: ModelMenuProps) => (
    <div>
      <button type="button" onClick={() => onModel("model-b")}>choose model</button>
      <button type="button" onClick={() => onEffort("high")}>choose effort</button>
      <button type="button" onClick={onReset}>reset preferences</button>
    </div>
  ),
}));

vi.mock("@/components/ComposerPlanModeButton", () => ({
  ComposerPlanModeButton: ({ onDisable }: PlanButtonProps) => (
    <button type="button" onClick={onDisable}>disable plan</button>
  ),
}));

vi.mock("@/components/ContextUsageChip", () => ({
  ContextUsageChip: ({ onCompact }: ContextChipProps) => (
    <button type="button" onClick={onCompact}>compact context</button>
  ),
}));

vi.mock("@/components/lobe-chat/TaskProgressRail", () => ({
  TaskProgressRail: ({ goalSummary, onOpenDetails }: ProgressRailProps) => (
    <div data-testid="task-progress-rail">
      <span>{goalSummary}</span>
      <button type="button" onClick={onOpenDetails}>open details</button>
    </div>
  ),
}));

vi.mock("@/components/MemoryContextBadge", () => ({
  MemoryContextBadge: ({ disabled, onClear }: MemoryBadgeProps) => (
    <div
      data-testid="memory-badge"
      data-disabled={disabled ? "true" : "false"}
    >
      <button type="button" onClick={onClear}>clear memory</button>
    </div>
  ),
}));

import { ComposerDock } from "./ComposerDock";

afterEach(cleanup);

const slashItem = {
  id: "goal",
  kind: "mode" as const,
  name: "goal",
  mode: "goal" as const,
};
const menuEntries: ComposerPlusEntry[] = [
  { id: "upload", kind: "upload" },
  { id: "folder", kind: "action", action: "folder", title: "Folder" },
  { id: "goal", kind: "slash", item: slashItem },
];

type DockOverrides = Partial<
  Omit<ComposerDockProps, "project" | "queue" | "menu" | "preferences" | "refs">
> & {
  project?: Partial<ComposerDockProps["project"]>;
  queue?: Partial<ComposerDockProps["queue"]>;
  menu?: Partial<ComposerDockProps["menu"]>;
  preferences?: Partial<ComposerDockProps["preferences"]>;
};

function makeProps(overrides: DockOverrides = {}): ComposerDockProps {
  const entries = overrides.menu?.entries ?? menuEntries;
  const menuEntriesRef = { current: entries };
  const project = {
    active: null,
    options: [{ id: "p1", name: "Sunsetz", path: "/repo", trusted: true, pathOk: true }],
    openRequestKey: 0,
    worktrees: [{ path: "/repo", branch: "main", detached: false, isMain: true, locked: false, prunable: false }],
    worktreesAvailable: true,
    worktreesLoading: false,
    worktreesReason: null,
    onSelect: vi.fn(),
    onAdd: vi.fn(),
    onSwitchWorktree: vi.fn(),
    onOpen: vi.fn(),
    ...overrides.project,
  } satisfies ComposerDockProps["project"];
  const queue = {
    items: [],
    flushHold: false,
    previewLabels: { filesCount: (n: number) => `${n} files`, empty: "empty" },
    onClear: vi.fn(),
    onRemove: vi.fn(),
    onRetry: vi.fn(),
    onSteer: vi.fn(),
    onEdit: vi.fn(),
    onPause: vi.fn(),
    ...overrides.queue,
  } satisfies ComposerDockProps["queue"];
  const menu = {
    open: false,
    positioned: false,
    plusMode: false,
    showPlus: false,
    liveSlashPresent: false,
    slashFilterQuery: "",
    skillsLoading: false,
    activeIndex: 0,
    entries,
    onActiveIndexChange: vi.fn(),
    onPickFiles: vi.fn(),
    onSelectAction: vi.fn(),
    onSelectSlash: vi.fn(),
    resolveTitle: (item: SlashItem) => item.name,
    resolveDescription: () => "description",
    onClose: vi.fn(),
    onTogglePlus: vi.fn(),
    ...overrides.menu,
  } satisfies ComposerDockProps["menu"];
  return {
    locale: "en",
    welcomeSession: false,
    goalMode: false,
    settingsLocked: false,
    sessionState: "idle",
    connecting: false,
    dropReady: false,
    draft: "",
    attachments: [],
    attachmentLabels: {
      open: "Open",
      reveal: "Reveal",
      copyPath: "Copy path",
      copyImage: "Copy image",
      addToComposer: "Add",
      remove: "Remove",
    },
    contextUsage: {
      tokens: null,
      source: "unknown",
      label: "—",
      lastCompact: null,
      contextWindowTokens: null,
      remainingTokens: null,
      percentUsed: null,
      runtime: null,
    },
    onDraftChange: vi.fn(),
    onRemoveAttachment: vi.fn(),
    onAddAttachment: vi.fn(),
    onPasteFiles: vi.fn(),
    onPasteMediaFallback: vi.fn(),
    onSlashQueryChange: vi.fn(),
    onCompact: vi.fn(),
    onSend: vi.fn(),
    onStop: vi.fn(),
    ...overrides,
    project,
    queue,
    menu,
    preferences: {
      mode: "agent",
      policy: "ask",
      modelId: "model-a",
      effort: "medium",
      models: [{ id: "model-a", label: "Model A" }],
      onMode: vi.fn(),
      onPolicy: vi.fn(),
      onDisablePlan: vi.fn(),
      onClearGoal: vi.fn(),
      onModel: vi.fn(),
      onEffort: vi.fn(),
      onReset: vi.fn(),
      ...overrides.preferences,
    },
    refs: {
      wrap: createRef<HTMLDivElement>(),
      input: createRef<HTMLDivElement>(),
      shell: createRef<HTMLDivElement>(),
      plusTrigger: createRef<HTMLButtonElement>(),
      plusPanel: createRef<HTMLDivElement>(),
      menuEntries: menuEntriesRef,
    },
  };
}

function renderDock(overrides: DockOverrides = {}) {
  const props = makeProps(overrides);
  return { props, ...render(<ComposerDock {...props} />) };
}

describe("ComposerDock", () => {
  it("sends a non-empty idle draft and disables an empty draft", async () => {
    const user = userEvent.setup();
    const { props, rerender } = renderDock({ draft: "Ship it" });

    await user.click(screen.getByRole("button", { name: "Send" }));
    expect(props.onSend).toHaveBeenCalledOnce();

    rerender(<ComposerDock {...props} draft="   " />);
    expect((screen.getByRole("button", { name: "Send" }) as HTMLButtonElement).disabled).toBe(true);
    fireEvent.keyDown(screen.getByRole("textbox"), { key: "Enter" });
    expect(props.onSend).toHaveBeenCalledOnce();
  });

  it("stops streaming and never sends through permission or composition guards", async () => {
    const user = userEvent.setup();
    const streaming = renderDock({ sessionState: "streaming", draft: "next" });
    await user.click(screen.getByRole("button", { name: "Stop" }));
    expect(streaming.props.onStop).toHaveBeenCalledOnce();
    expect(screen.getByRole("textbox").getAttribute("aria-disabled")).toBe("false");
    streaming.unmount();

    const permission = renderDock({ sessionState: "awaiting_permission", draft: "blocked" });
    const editor = screen.getByRole("textbox");
    expect(editor.getAttribute("aria-disabled")).toBe("true");
    expect(screen.getByRole("button", { name: "Stop" })).toBeTruthy();
    fireEvent.keyDown(editor, { key: "Enter" });
    fireEvent.keyDown(editor, { key: "Enter", isComposing: true });
    fireEvent.keyDown(editor, { key: "Enter", keyCode: 229 });
    fireEvent.keyDown(editor, { key: "Enter", shiftKey: true });
    expect(permission.props.onSend).not.toHaveBeenCalled();
  });

  it("renders queue previews and delegates clear, retry, remove, and attachment actions", async () => {
    const user = userEvent.setup();
    const image = { path: "/tmp/a.png", name: "a.png", isDir: false };
    const secondImage = { path: "/tmp/b.jpg", name: "b.jpg", isDir: false };
    const folder = { path: "/tmp/folder", name: "folder", isDir: true };
    const queued = {
      id: "q1",
      storedDisplay: `[[skill:review]] ${"long ".repeat(30)}`,
      attachments: [],
      goalMode: false,
      createdAt: 1,
    };
    const { props } = renderDock({
      attachments: [image, secondImage, folder],
      queue: { items: [queued], flushHold: true },
    });

    expect(screen.getByLabelText("1 queued in this chat")).toBeTruthy();
    expect(screen.getByText("1").closest(".composer__queue-count")?.getAttribute("title")).toContain("/review");
    await user.click(screen.getByRole("button", { name: "Steer" }));
    await user.click(screen.getByRole("button", { name: "Remove from queue" }));
    expect(props.queue.onSteer).toHaveBeenCalledOnce();
    expect(props.queue.onRemove).toHaveBeenCalledWith("q1");
    await user.click(screen.getByRole("button", { name: "More" }));
    await user.click(screen.getByRole("menuitem", { name: "Edit message" }));
    expect(props.queue.onEdit).toHaveBeenCalledWith("q1");

    const card = screen.getByTestId("attachment-a.png");
    expect(card.getAttribute("data-gallery")).toBe("/tmp/a.png|/tmp/b.jpg");
    await user.click(within(card).getByRole("button", { name: "remove attachment" }));
    await user.click(within(card).getByRole("button", { name: "add attachment" }));
    expect(props.onRemoveAttachment).toHaveBeenCalledWith(image);
    expect(props.onAddAttachment).toHaveBeenCalledWith(image);
  });

  it("supports the goal and project rails plus all preference actions", async () => {
    const user = userEvent.setup();
    const projectView = renderDock({ preferences: { mode: "plan" } });
    await user.click(screen.getByRole("button", { name: "select project" }));
    await user.click(screen.getByRole("button", { name: "add project" }));
    await user.click(screen.getByRole("button", { name: "switch worktree" }));
    await user.click(screen.getByRole("button", { name: "open project" }));
    await user.click(screen.getByRole("button", { name: "choose policy" }));
    await user.click(screen.getByRole("button", { name: "disable plan" }));
    await user.click(screen.getByRole("button", { name: "choose model" }));
    await user.click(screen.getByRole("button", { name: "choose effort" }));
    await user.click(screen.getByRole("button", { name: "reset preferences" }));
    await user.click(screen.getByRole("button", { name: "compact context" }));
    expect(projectView.props.project.onSelect).toHaveBeenCalledWith(projectView.props.project.options[0]);
    expect(projectView.props.project.onAdd).toHaveBeenCalledOnce();
    expect(projectView.props.project.onSwitchWorktree).toHaveBeenCalledOnce();
    expect(projectView.props.project.onOpen).toHaveBeenCalledOnce();
    expect(projectView.props.preferences.onPolicy).toHaveBeenCalledWith("always_approve");
    expect(projectView.props.preferences.onDisablePlan).toHaveBeenCalledOnce();
    expect(projectView.props.preferences.onModel).toHaveBeenCalledWith("model-b");
    expect(projectView.props.preferences.onEffort).toHaveBeenCalledWith("high");
    expect(projectView.props.preferences.onReset).toHaveBeenCalledOnce();
    expect(projectView.props.onCompact).toHaveBeenCalledOnce();
    projectView.unmount();

    const goalView = renderDock({ goalMode: true, settingsLocked: true });
    await user.click(screen.getByRole("button", { name: "Goal" }));
    expect(document.activeElement).toBe(screen.getByRole("textbox"));
    expect(screen.getByRole("textbox").getAttribute("data-placeholder")).toContain("Describe your goal");
    expect((screen.getByRole("button", { name: "Clear goal mode" }) as HTMLButtonElement).disabled).toBe(true);
    expect(goalView.props.preferences.onClearGoal).not.toHaveBeenCalled();
  });

  it("delegates editor and plus-panel callbacks", async () => {
    const user = userEvent.setup();
    const { props } = renderDock({
      dropReady: true,
      menu: {
        open: true,
        positioned: true,
        plusMode: true,
        showPlus: true,
        liveSlashPresent: true,
        slashFilterQuery: "go",
        style: { left: 10 },
      },
    });

    expect(document.querySelector(".composer")?.classList.contains("composer--drop-ready")).toBe(true);
    expect(screen.getByRole("menu").getAttribute("data-mode")).toBe("plus");
    expect(screen.getByRole("menu").getAttribute("data-filter")).toBe("go");
    expect(screen.getByRole("menu").getAttribute("data-z")).toBe("10050");
    await user.click(screen.getByRole("button", { name: "edit draft" }));
    await user.click(screen.getByRole("button", { name: "paste files" }));
    await user.click(screen.getByRole("button", { name: "paste fallback" }));
    await user.click(screen.getByRole("button", { name: "slash query" }));
    await user.click(screen.getByRole("button", { name: "panel upload" }));
    await user.click(screen.getByRole("button", { name: "panel action" }));
    await user.click(screen.getByRole("button", { name: "panel slash" }));
    await user.click(screen.getByRole("button", { name: "panel next" }));
    await user.click(screen.getByRole("button", { name: "Add" }));
    expect(props.onDraftChange).toHaveBeenCalledWith("edited");
    expect(props.onPasteFiles).toHaveBeenCalledOnce();
    expect(props.onPasteMediaFallback).toHaveBeenCalledWith({ expectMedia: true });
    expect(props.onSlashQueryChange).toHaveBeenCalledWith({ start: 0, query: "go", end: 3 });
    expect(props.menu.onPickFiles).toHaveBeenCalledOnce();
    expect(props.menu.onSelectAction).toHaveBeenCalledWith(menuEntries[1]);
    expect(props.menu.onSelectSlash).toHaveBeenCalledWith(slashItem);
    expect(props.menu.onActiveIndexChange).toHaveBeenCalledOnce();
    expect(props.menu.onTogglePlus).toHaveBeenCalledOnce();
  });

  it("uses keyboard navigation and selection for open plus and slash menus", () => {
    const { props, unmount } = renderDock({
      menu: { open: true, positioned: false, activeIndex: 1 },
      draft: "command",
    });
    const editor = screen.getByRole("textbox");
    fireEvent.keyDown(editor, { key: "ArrowDown" });
    fireEvent.keyDown(editor, { key: "ArrowUp" });
    fireEvent.keyDown(editor, { key: "Enter" });
    fireEvent.keyDown(editor, { key: "Tab" });
    fireEvent.keyDown(editor, { key: "Escape" });
    expect(props.menu.onActiveIndexChange).toHaveBeenCalledTimes(2);
    const down = vi.mocked(props.menu.onActiveIndexChange).mock.calls[0]?.[0];
    const up = vi.mocked(props.menu.onActiveIndexChange).mock.calls[1]?.[0];
    expect(typeof down === "function" ? down(2) : down).toBe(0);
    expect(typeof up === "function" ? up(0) : up).toBe(2);
    expect(props.menu.onSelectAction).toHaveBeenCalledTimes(2);
    expect(props.menu.onClose).toHaveBeenCalledOnce();
    expect(props.onSend).not.toHaveBeenCalled();
    unmount();

    const slashView = renderDock({
      menu: { open: true, entries: menuEntries, activeIndex: 0 },
      draft: "command",
    });
    fireEvent.keyDown(screen.getByRole("textbox"), { key: "Enter" });
    fireEvent.keyDown(screen.getByRole("textbox"), { key: "Tab" });
    expect(slashView.props.menu.onPickFiles).toHaveBeenCalledTimes(2);
    slashView.unmount();

    const slashPick = renderDock({
      menu: { open: true, entries: menuEntries, activeIndex: 2 },
      draft: "command",
    });
    fireEvent.keyDown(screen.getByRole("textbox"), { key: "Enter" });
    fireEvent.keyDown(screen.getByRole("textbox"), { key: "Tab" });
    expect(slashPick.props.menu.onSelectSlash).toHaveBeenCalledTimes(2);
    expect(slashPick.props.menu.onSelectSlash).toHaveBeenCalledWith(slashItem);
    slashPick.unmount();

    const sendView = renderDock({ draft: "Ship it" });
    fireEvent.keyDown(screen.getByRole("textbox"), { key: "Enter" });
    fireEvent.keyDown(screen.getByRole("textbox"), { key: "Escape" });
    expect(sendView.props.onSend).toHaveBeenCalledOnce();
    expect(sendView.props.menu.onClose).toHaveBeenCalledOnce();
  });

  it("owns the floating wrap, progress rail, permission slot, and takeover", async () => {
    const user = userEvent.setup();
    const onOpenDetails = vi.fn();
    const onClear = vi.fn();
    const pack = {
      version: 1 as const,
      items: [
        {
          candidateId: "c1",
          contentHash: "a".repeat(64),
          type: "user_preference" as const,
          content: "Prefer evidence.",
          source: { sessionId: "s1", messageId: "m1" },
        },
      ],
    };
    const { props, rerender, container } = renderDock({
      welcomeSession: true,
      memory: {
        pack,
        labels: {
          regionLabel: "Reviewed Memory context",
          title: "Memory",
          reviewedContext: "reviewed",
          notInstructions: "not instructions",
          noFtsSessionEvidence: "no fts",
          itemCount: "{count} items",
          clear: "clear",
          content: "content",
          expandItem: "expand",
          fullContent: "full",
          emptyContent: "empty",
          provenance: "source",
          typeLabels: {
            user_preference: "preference",
            project_fact: "fact",
            workflow_hint: "hint",
          },
        },
        onClear,
      },
      permission: <div data-testid="permission-slot">Allow once</div>,
    });

    const wrap = container.querySelector(".composer-wrap");
    expect(wrap?.classList.contains("composer-wrap--welcome")).toBe(true);
    expect(wrap).toBe(props.refs.wrap.current);
    expect(screen.getByTestId("project-menu")).toBeTruthy();
    expect(screen.queryByTestId("project-instruction-chip")).toBeNull();
    expect(screen.getByTestId("permission-slot")).toBeTruthy();
    expect(screen.getByTestId("memory-badge").getAttribute("data-disabled")).toBe("false");

    rerender(
      <ComposerDock
        {...props}
        projectInstruction={{
          path: "AGENTS.md",
          truncated: false,
          labels: {
            attached: "Project instructions: AGENTS.md",
            truncated: "Project instructions: AGENTS.md (truncated)",
          },
        }}
      />,
    );
    expect(screen.getByTestId("project-instruction-chip").textContent).toBe(
      "Project instructions: AGENTS.md",
    );
    await user.click(screen.getByRole("button", { name: "clear memory" }));
    expect(onClear).toHaveBeenCalledOnce();

    rerender(
      <ComposerDock {...props} connecting sessionState="connecting" />,
    );
    expect(screen.getByTestId("memory-badge").getAttribute("data-disabled")).toBe(
      "true",
    );

    rerender(
      <ComposerDock
        {...props}
        progress={{
          entries: [{ title: "Ship", status: "in_progress" }],
          changes: [],
          goalSummary: "Land the dock",
          elapsedMs: 1200,
          streaming: true,
          labels: {
            step: "Step {current} / {total}",
            filesChanged: "{count} files",
            activeGoal: "Active goal",
            details: "Details",
            edit: "Edit",
            pause: "Pause",
            delete: "Delete",
          },
          onOpenDetails,
        }}
      />,
    );
    expect(screen.queryByTestId("project-menu")).toBeNull();
    expect(screen.getByTestId("task-progress-rail")).toBeTruthy();
    await user.click(screen.getByRole("button", { name: "open details" }));
    expect(onOpenDetails).toHaveBeenCalledOnce();
    expect(screen.getByRole("textbox")).toBeTruthy();

    rerender(
      <ComposerDock
        {...props}
        connecting
        sessionState="connecting"
        takeover={<div role="region">Question 1 of 2</div>}
      />,
    );
    expect(screen.getByText("Question 1 of 2")).toBeTruthy();
    expect(screen.queryByRole("textbox")).toBeNull();
    expect(screen.queryByTestId("project-menu")).toBeNull();
    expect(container.querySelector(".composer-dock")).toBeNull();
  });

  it("ignores empty plus-menu keyboard events instead of sending", () => {
    const { props } = renderDock({
      menu: { open: true, entries: [], activeIndex: 0 },
      draft: "ready",
    });
    const editor = screen.getByRole("textbox");
    fireEvent.keyDown(editor, { key: "ArrowDown" });
    fireEvent.keyDown(editor, { key: "ArrowUp" });
    fireEvent.keyDown(editor, { key: "Enter" });
    fireEvent.keyDown(editor, { key: "Tab" });
    expect(props.menu.onActiveIndexChange).not.toHaveBeenCalled();
    expect(props.menu.onPickFiles).not.toHaveBeenCalled();
    expect(props.menu.onSelectAction).not.toHaveBeenCalled();
    expect(props.onSend).not.toHaveBeenCalled();
  });
});
