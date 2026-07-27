import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import {
  buildComposerPlusEntries,
  buildComposerPlusRows,
  ComposerPlusPanel,
  type ComposerPlusEntry,
} from "./ComposerPlusPanel";

const command = {
  id: "goal",
  kind: "mode" as const,
  name: "goal",
  mode: "goal" as const,
};

function renderPanel(mode: "plus" | "slash", filterQuery?: string) {
  return renderToStaticMarkup(
    <ComposerPlusPanel
      open
      mode={mode}
      locale="en"
      entries={[{ id: "goal", kind: "slash", item: command }]}
      filterQuery={filterQuery}
      activeIndex={0}
      onActiveIndexChange={() => undefined}
      onSelectUpload={() => undefined}
      onSelectSlash={() => undefined}
      resolveTitle={() => "Goal"}
      resolveDescription={() => "Set a goal"}
    />,
  );
}

describe("ComposerPlusPanel semantics", () => {
  it("keeps an empty slash query as a listbox", () => {
    const html = renderPanel("slash", "");
    expect(html).toContain('role="listbox"');
    expect(html).toContain('role="option"');
    expect(html).not.toContain('role="menu"');
  });

  it("renders the plus surface as an action menu", () => {
    const html = renderPanel("plus");
    expect(html).toContain('role="menu"');
    expect(html).toContain('role="menuitem"');
    expect(html).not.toContain('role="listbox"');
  });

  it("renders file and folder as separate real picker actions", () => {
    const entries = buildComposerPlusEntries({
      showUpload: true,
      actions: [
        {
          id: "folder",
          kind: "action",
          action: "folder",
          title: "Folder",
          description: "Choose a folder",
        },
      ],
      commands: [],
      skills: [],
      plusMenu: true,
    });
    const html = renderToStaticMarkup(
      <ComposerPlusPanel
        open
        mode="plus"
        locale="en"
        entries={entries}
        activeIndex={0}
        onActiveIndexChange={() => undefined}
        onSelectUpload={() => undefined}
        onSelectAction={() => undefined}
        onSelectSlash={() => undefined}
        resolveTitle={() => ""}
        resolveDescription={() => ""}
      />,
    );

    expect(entries.map((entry) => entry.id)).toEqual(["upload", "folder"]);
    expect(html).toContain(">Files<");
    expect(html).toContain(">Folder<");
    expect(html).toContain("Choose one or more files");
    expect(html).toContain("Choose a folder");
  });

  it("announces Finder empty-selection feedback inside the menu", () => {
    const html = renderToStaticMarkup(
      <ComposerPlusPanel
        open
        mode="plus"
        locale="en"
        entries={[
          {
            id: "finder",
            kind: "action",
            action: "finder",
            title: "Finder selection",
            description: "Nothing is selected in Finder",
            feedback: true,
          },
        ]}
        activeIndex={0}
        onActiveIndexChange={() => undefined}
        onSelectUpload={() => undefined}
        onSelectAction={() => undefined}
        onSelectSlash={() => undefined}
        resolveTitle={() => ""}
        resolveDescription={() => ""}
      />,
    );

    expect(html).toContain('role="status"');
    expect(html).toContain('aria-live="polite"');
    expect(html).toContain("Nothing is selected in Finder");
  });
});

describe("composer plus rows", () => {
  it("keeps actions under Add and installed skills under Plugins", () => {
    const action: ComposerPlusEntry = {
      id: "finder",
      kind: "action",
      action: "finder",
      title: "Finder selection",
    };
    const skill = {
      id: "skill:review",
      kind: "skill" as const,
      name: "review",
      displayTitle: "Review",
    };
    const entries = buildComposerPlusEntries({
      showUpload: true,
      actions: [action],
      commands: [],
      skills: [skill],
      plusMenu: true,
    });
    const rows = buildComposerPlusRows(
      entries,
      {
        add: "Add",
        commands: "Commands",
        skills: "Skills",
        plugins: "Plugins",
      },
      true,
    );

    expect(
      rows.filter((row) => row.type === "section").map((row) => row.label),
    ).toEqual(["Add", "Plugins"]);
    expect(
      rows.filter((row) => row.type === "entry").map((row) => row.navIndex),
    ).toEqual([0, 1, 2]);
  });
});
