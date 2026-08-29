// @vitest-environment jsdom

import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { MarkdownChat } from "./MarkdownChat";

afterEach(() => cleanup());

describe("MarkdownChat", () => {
  it("applies muted and streaming classes", () => {
    const { container } = render(
      <MarkdownChat locale="en" muted streaming>
        {""}
      </MarkdownChat>,
    );
    expect(container.querySelector(".chat-md--muted")).toBeTruthy();
    expect(container.querySelector(".chat-md--streaming")).toBeTruthy();
  });

  it("renders finished markdown without a frozen wrapper", () => {
    const { container } = render(
      <MarkdownChat locale="en">Hello **world**</MarkdownChat>,
    );
    expect(screen.getByText("world")).toBeTruthy();
    expect(container.querySelector(".chat-md__frozen")).toBeNull();
  });

  it("freezes completed paragraphs while streaming", () => {
    const { container } = render(
      <MarkdownChat locale="en" streaming>
        {"Done paragraph.\n\nLive tail"}
      </MarkdownChat>,
    );
    expect(container.querySelector(".chat-md__frozen")?.textContent).toContain(
      "Done paragraph",
    );
    expect(container.querySelector(".chat-md__live")?.textContent).toContain(
      "Live",
    );
  });

  it("renders headings and fenced blocks", () => {
    const { container } = render(
      <MarkdownChat locale="en">
        {`## Notes\n\n\`\`\`ts\nconst n = 1;\n\`\`\`\n`}
      </MarkdownChat>,
    );
    expect(container.textContent).toContain("Notes");
    expect(container.textContent).toContain("const n = 1");
  });

  it("soft-closes an open fence while streaming", () => {
    const { container } = render(
      <MarkdownChat locale="en" streaming>
        {"```ts\nconst x = 1;"}
      </MarkdownChat>,
    );
    expect(container.querySelector(".chat-md--streaming")).toBeTruthy();
  });
});
