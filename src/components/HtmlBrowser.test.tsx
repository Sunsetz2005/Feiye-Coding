// @vitest-environment jsdom

import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  read: vi.fn(),
  tauri: true,
}));

vi.mock("@/lib/api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/api")>();
  return {
    ...actual,
    isTauri: () => mocks.tauri,
    fsReadAbsolute: mocks.read,
  };
});

import {
  buildStaticPreviewDocument,
  HtmlBrowser,
} from "@/components/HtmlBrowser";

beforeEach(() => {
  mocks.tauri = true;
  mocks.read.mockReset();
});

afterEach(cleanup);

describe("HtmlBrowser static isolation", () => {
  it("injects a network- and script-blocking CSP into full documents", () => {
    const result = buildStaticPreviewDocument(
      "<!doctype html><html><head><title>x</title></head><body>ok</body></html>",
    );
    expect(result).toContain('http-equiv="Content-Security-Policy"');
    expect(result).toContain("script-src 'none'");
    expect(result).toContain("connect-src 'none'");
    expect(result).toContain("form-action 'none'");
    expect(buildStaticPreviewDocument("<html><body>ok</body></html>")).toContain(
      "<head><meta",
    );
    expect(buildStaticPreviewDocument("<p>fragment</p>")).toContain(
      "<!doctype html><html><head>",
    );
  });

  it("renders an opaque-origin sandbox without privileged allow features", () => {
    render(<HtmlBrowser html="<style>body{color:red}</style><p>safe</p>" />);
    const frame = screen.getByTitle("HTML");
    expect(frame.getAttribute("sandbox")).toBe("");
    expect(frame.hasAttribute("allow")).toBe(false);
    expect(frame.getAttribute("referrerpolicy")).toBe("no-referrer");
    expect(frame.getAttribute("srcdoc")).toContain("default-src 'none'");
  });

  it("loads local HTML only through the authorized Host read command", async () => {
    mocks.read.mockResolvedValue({ text: "<html><body>host-safe</body></html>" });
    render(<HtmlBrowser absolutePath="/trusted/report.html" />);

    const frame = await screen.findByTitle("HTML");
    expect(mocks.read).toHaveBeenCalledWith("/trusted/report.html");
    expect(frame.getAttribute("srcdoc")).toContain("host-safe");
  });

  it("fails closed for rejected, binary, and browser-only local reads", async () => {
    mocks.read.mockResolvedValueOnce({ error: "outside trusted roots" });
    const denied = render(<HtmlBrowser absolutePath="/private/report.html" />);
    expect((await screen.findByRole("alert")).textContent).toContain(
      "outside trusted roots",
    );
    denied.unmount();

    mocks.read.mockResolvedValueOnce({ text: null });
    const binary = render(<HtmlBrowser absolutePath="/trusted/binary.html" />);
    expect((await screen.findByRole("alert")).textContent).toContain(
      "not UTF-8 text",
    );
    binary.unmount();

    mocks.tauri = false;
    render(<HtmlBrowser absolutePath="/trusted/browser.html" />);
    expect((await screen.findByRole("alert")).textContent).toContain(
      "Tauri required",
    );
  });
});
