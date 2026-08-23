/**
 * Local HTML preview for the resource pane.
 *
 * Nested Tauri WebViews are possible but heavy (positioning, z-index, lifecycle).
 * WKWebView also blocks `file://` inside the main app iframe → blank page.
 *
 * Local reports are rendered as static, isolated documents. They never inherit
 * the main WebView origin or Tauri IPC access.
 */

import { useEffect, useState } from "react";
import { fsReadAbsolute, isTauri } from "@/lib/api";

const STATIC_PREVIEW_CSP = [
  "default-src 'none'",
  "base-uri 'none'",
  "connect-src 'none'",
  "font-src data:",
  "form-action 'none'",
  "frame-src 'none'",
  "img-src data: blob:",
  "media-src data: blob:",
  "object-src 'none'",
  "script-src 'none'",
  "style-src 'unsafe-inline'",
].join("; ");

export function buildStaticPreviewDocument(source: string): string {
  const meta = `<meta http-equiv="Content-Security-Policy" content="${STATIC_PREVIEW_CSP}">`;
  if (/<head(?:\s[^>]*)?>/i.test(source)) {
    return source.replace(/<head(\s[^>]*)?>/i, (match) => `${match}${meta}`);
  }
  if (/<html(?:\s[^>]*)?>/i.test(source)) {
    return source.replace(/<html(\s[^>]*)?>/i, (match) => `${match}<head>${meta}</head>`);
  }
  return `<!doctype html><html><head>${meta}</head><body>${source}</body></html>`;
}

export interface HtmlBrowserProps {
  title?: string;
  /** Absolute filesystem path (for fetch fallback). */
  absolutePath?: string | null;
  /** Full HTML document from host read (preferred). */
  html?: string | null;
  className?: string;
}

async function fetchHtmlText(absolutePath: string): Promise<string> {
  if (!isTauri()) {
    throw new Error("Tauri required to load local HTML");
  }
  const result = await fsReadAbsolute(absolutePath);
  if (result.error) throw new Error(result.error);
  if (typeof result.text !== "string") {
    throw new Error("HTML preview is not UTF-8 text");
  }
  return result.text;
}

export function HtmlBrowser({
  title = "HTML",
  absolutePath,
  html,
  className = "",
}: HtmlBrowserProps) {
  const [doc, setDoc] = useState<string>(html?.trim() ? html : "");
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(!html?.trim() && !!absolutePath);

  useEffect(() => {
    if (html?.trim()) {
      setDoc(html);
      setError(null);
      setLoading(false);
      return;
    }
    if (!absolutePath) {
      setDoc("");
      setError("no HTML content");
      setLoading(false);
      return;
    }
    let cancelled = false;
    setLoading(true);
    setError(null);
    void fetchHtmlText(absolutePath)
      .then((text) => {
        if (cancelled) return;
        setDoc(text);
        setLoading(false);
      })
      .catch((e) => {
        if (cancelled) return;
        setError(String(e));
        setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [html, absolutePath]);

  if (loading) {
    return (
      <div className={"rp-preview-browser rp-preview-browser--msg " + className}>
        <div className="rp-preview__msg">Loading…</div>
      </div>
    );
  }

  if (error || !doc) {
    return (
      <div className={"rp-preview-browser rp-preview-browser--msg " + className}>
        <div className="rp-preview__msg" role="alert">
          {error || "Empty HTML"}
        </div>
      </div>
    );
  }

  return (
    <iframe
      className={
        "rp-preview__frame rp-preview__frame--browser " + className
      }
      title={title}
      sandbox=""
      referrerPolicy="no-referrer"
      srcDoc={buildStaticPreviewDocument(doc)}
    />
  );
}
