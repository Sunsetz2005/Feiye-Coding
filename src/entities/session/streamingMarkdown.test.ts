import { describe, expect, it } from "vitest";
import { splitStreamingMarkdown } from "./streamingMarkdown";

describe("splitStreamingMarkdown", () => {
  it("keeps short undivided text as tail", () => {
    expect(splitStreamingMarkdown("hello")).toEqual({
      frozen: "",
      tail: "hello",
    });
  });

  it("freezes completed paragraphs", () => {
    expect(splitStreamingMarkdown("one\n\ntwo")).toEqual({
      frozen: "one\n\n",
      tail: "two",
    });
  });

  it("freezes a closed fence and leaves trailing prose", () => {
    const src = "```ts\nconst x = 1;\n```\nmore";
    expect(splitStreamingMarkdown(src)).toEqual({
      frozen: "```ts\nconst x = 1;\n```\n",
      tail: "more",
    });
  });

  it("does not freeze an open fence", () => {
    const src = "intro\n\n```ts\nconst x = 1;";
    expect(splitStreamingMarkdown(src)).toEqual({
      frozen: "intro\n\n",
      tail: "```ts\nconst x = 1;",
    });
  });

  it("returns empty parts for empty input", () => {
    expect(splitStreamingMarkdown("")).toEqual({ frozen: "", tail: "" });
  });
});
