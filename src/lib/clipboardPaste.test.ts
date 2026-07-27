import { describe, expect, it } from "vitest";
import {
  clipboardLooksLikeMedia,
  clipboardPlainText,
  clipboardTextPreservingLayout,
  collectFilesFromDataTransfer,
  isFileUrlOnlyText,
  normalizePastedTextLayout,
} from "./clipboardPaste";

function fakeFile(name: string, type: string, size = 12): File {
  const buf = new Uint8Array(size);
  return new File([buf], name, { type, lastModified: 1 });
}

describe("collectFilesFromDataTransfer", () => {
  it("returns empty for null", () => {
    expect(collectFilesFromDataTransfer(null)).toEqual([]);
  });

  it("collects files from items kind=file", () => {
    const f = fakeFile("shot.png", "image/png");
    const data = {
      files: { length: 0, item: () => null } as unknown as FileList,
      items: [
        {
          kind: "file",
          type: "image/png",
          getAsFile: () => f,
        },
      ],
      types: ["Files", "image/png"],
      getData: () => "",
    } as unknown as DataTransfer;
    const files = collectFilesFromDataTransfer(data);
    expect(files).toHaveLength(1);
    expect(files[0]?.name).toBe("shot.png");
  });

  it("dedupes same file from files + items", () => {
    const f = fakeFile("a.png", "image/png");
    const data = {
      files: {
        length: 1,
        item: (i: number) => (i === 0 ? f : null),
        0: f,
        [Symbol.iterator]: function* () {
          yield f;
        },
      } as unknown as FileList,
      items: [
        {
          kind: "file",
          type: "image/png",
          getAsFile: () => f,
        },
      ],
      types: ["Files"],
      getData: () => "",
    } as unknown as DataTransfer;
    expect(collectFilesFromDataTransfer(data)).toHaveLength(1);
  });
});

describe("clipboardLooksLikeMedia", () => {
  it("detects image types without File objects", () => {
    const data = {
      files: { length: 0, item: () => null } as unknown as FileList,
      items: [{ kind: "string", type: "image/png", getAsFile: () => null }],
      types: ["image/png"],
      getData: () => "",
    } as unknown as DataTransfer;
    expect(clipboardLooksLikeMedia(data)).toBe(true);
  });

  it("false for plain text only", () => {
    const data = {
      files: { length: 0, item: () => null } as unknown as FileList,
      items: [{ kind: "string", type: "text/plain", getAsFile: () => null }],
      types: ["text/plain"],
      getData: () => "hello",
    } as unknown as DataTransfer;
    expect(clipboardLooksLikeMedia(data)).toBe(false);
  });
});

describe("clipboardPlainText / isFileUrlOnlyText", () => {
  it("normalizes newlines", () => {
    const data = {
      getData: (t: string) => (t === "text/plain" ? "a\r\nb\rc" : ""),
    } as unknown as DataTransfer;
    expect(clipboardPlainText(data)).toBe("a\nb\nc");
  });

  it("preserves Unicode line separators and restores flattened bullet lists", () => {
    expect(
      normalizePastedTextLayout(
        "创建：• 可重用组件• 无障碍\u2028结果：• 实现• 示例",
      ),
    ).toBe(
      "创建：\n• 可重用组件\n• 无障碍\n结果：\n• 实现\n• 示例",
    );
  });

  it("keeps an already multiline prompt unchanged", () => {
    const data = {
      getData: (t: string) =>
        t === "text/plain" ? "标题\n  缩进内容\n\n结尾" : "",
    } as unknown as DataTransfer;
    expect(clipboardTextPreservingLayout(data)).toBe(
      "标题\n  缩进内容\n\n结尾",
    );
  });

  it("detects file url only", () => {
    expect(isFileUrlOnlyText("file:///tmp/x.png")).toBe(true);
    expect(isFileUrlOnlyText("hello\nfile:///tmp/x.png")).toBe(false);
  });
});
