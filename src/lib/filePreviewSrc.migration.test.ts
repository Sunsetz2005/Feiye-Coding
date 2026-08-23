// @vitest-environment jsdom

import { afterEach, describe, expect, it, vi } from "vitest";

const convertMock = vi.hoisted(() =>
  vi.fn((value: string, protocol?: string) => `${protocol || "asset"}://${value}`),
);

vi.mock("@tauri-apps/api/core", () => ({ convertFileSrc: convertMock }));

import type { FsReadResult } from "./api";
import {
  fetchPreviewArrayBuffer,
  pathToPreviewUrl,
  resolvePreviewSrc,
} from "./filePreviewSrc";

function setDesktop(enabled: boolean): void {
  if (enabled) {
    Object.defineProperty(window, "__TAURI_INTERNALS__", {
      value: {},
      configurable: true,
    });
  } else {
    delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
  }
}

function preview(fields: Partial<FsReadResult>): FsReadResult {
  return {
    relativePath: "a",
    absolutePath: "/project/a",
    name: "a",
    kind: "text",
    mime: "text/plain",
    stream: false,
    ...fields,
  } as FsReadResult;
}

describe("authorized preview URLs", () => {
  afterEach(() => {
    setDesktop(false);
    convertMock.mockClear();
    vi.unstubAllGlobals();
  });

  it("requires desktop authorization for local previews", async () => {
    expect(await pathToPreviewUrl("", "image")).toBe(null);
    expect(await pathToPreviewUrl("/tmp/report.html", "html")).toContain("file:///tmp/report.html");
    expect(await pathToPreviewUrl("/tmp/a.png", "image")).toBe(null);
  });

  it("prefers opaque handles and keeps media on the checked protocol", async () => {
    setDesktop(true);
    expect(await pathToPreviewUrl("/private/a.pdf", "pdf", "h1")).toBe("resource://h1");
    expect(await pathToPreviewUrl("/project/a.mp4", "video")).toBe("media:///project/a.mp4");
    expect(await pathToPreviewUrl("/project/a.txt", "text")).toBe(null);
    convertMock.mockImplementationOnce(() => {
      throw new Error("conversion failed");
    });
    expect(await pathToPreviewUrl("/project/a.png", "image")).toBe(null);
  });

  it("resolves HTML, stream, base64, legacy, and empty preview shapes", async () => {
    expect(await resolvePreviewSrc(preview({ kind: "html", text: "<p>x</p>" }))).toBe(null);
    setDesktop(true);
    expect(
      await resolvePreviewSrc(
        preview({ kind: "image", stream: true, resourceHandleId: "h2" }),
      ),
    ).toBe("resource://h2");
    setDesktop(false);
    expect(
      await resolvePreviewSrc(preview({ base64: "YWJj", mime: "image/png" })),
    ).toBe("data:image/png;base64,YWJj");
    setDesktop(true);
    expect(await resolvePreviewSrc(preview({ kind: "office", resourceHandleId: "h3" }))).toBe(
      "resource://h3",
    );
    setDesktop(false);
    expect(await resolvePreviewSrc(preview({ absolutePath: "", mime: "" }))).toBe(null);
  });

  it("fetches handle bytes and rejects missing or failed URLs", async () => {
    setDesktop(true);
    vi.stubGlobal(
      "fetch",
      vi.fn(async () => ({ ok: true, arrayBuffer: async () => new Uint8Array([1, 2]).buffer })),
    );
    const bytes = await fetchPreviewArrayBuffer("/project/a.pdf", "pdf", "h4");
    expect(bytes.byteLength).toBe(2);

    vi.stubGlobal("fetch", vi.fn(async () => ({ ok: false, status: 403 })));
    await expect(fetchPreviewArrayBuffer("/project/a.pdf", "pdf", "h4")).rejects.toThrow("403");
    setDesktop(false);
    await expect(fetchPreviewArrayBuffer("/project/a.txt", "text")).rejects.toThrow(
      "cannot resolve",
    );
  });
});
