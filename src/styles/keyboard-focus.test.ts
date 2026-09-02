import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

const stylesDir = dirname(fileURLToPath(import.meta.url));

function readStyle(name: string): string {
  return readFileSync(join(stylesDir, name), "utf8");
}

describe("keyboard focus rings", () => {
  it("keeps workbench keyboard rings inset so overflow shells cannot clip them", () => {
    const css = readStyle("workbench.css");
    const keyboardBlock = css.slice(
      css.indexOf("WebView2 Tab often does not set"),
      css.indexOf("html[data-kb-focus=\"true\"] .composer:focus-within"),
    );
    expect(keyboardBlock).toContain("html[data-kb-focus=\"true\"]");
    expect(keyboardBlock).toContain("outline-offset: -2px");
    expect(keyboardBlock).not.toContain("outline-offset: 2px");
  });

  it("repeats keyboard-mode chrome rings in the last-loaded spatial layer", () => {
    const css = readStyle("apple.css");
    expect(css).toContain("html[data-kb-focus=\"true\"]");
    expect(css).toContain("outline-offset: -2px");
    expect(css).toContain(
      "html[data-kb-focus=\"true\"] .composer:focus-within",
    );
  });

  it("keeps empty-session suggestion cards stacked so a focused card stays visible", () => {
    const css = readStyle("workbench.css");
    expect(css).toContain(".composer-empty-hero__card:focus");
    expect(css).toContain("z-index: 2");
    expect(css).toContain("isolation: isolate");
    expect(css).toContain("@media (max-height: 760px)");
  });
});
