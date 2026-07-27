/**
 * Composer clipboard helpers — paste images/files into attachments.
 *
 * Tauri / WKWebView often omits File objects from the paste event for
 * screenshots (only `image/png` types, or nothing usable). Callers should:
 * 1. collectFilesFromDataTransfer(clipboardData)
 * 2. if empty + clipboardLooksLikeMedia → readClipboardMediaFiles()
 * 3. if still empty → native Host clipboard (arboard)
 */

/** Collect File objects from a paste/drop DataTransfer (deduped). */
export function collectFilesFromDataTransfer(
  data: DataTransfer | null | undefined,
): File[] {
  if (!data) return [];
  const fileMap = new Map<string, File>();

  if (data.files?.length) {
    for (let i = 0; i < data.files.length; i++) {
      const f = data.files.item(i);
      if (f) fileMap.set(fileKey(f), f);
    }
  }

  const items = data.items;
  if (items) {
    for (let i = 0; i < items.length; i++) {
      const item = items[i];
      if (!item) continue;
      // Screenshots: kind "file" + type image/*; some WebViews only expose type.
      if (item.kind === "file" || item.type.startsWith("image/")) {
        const f = item.getAsFile();
        if (f) fileMap.set(fileKey(f), f);
      }
    }
  }

  return Array.from(fileMap.values());
}

function fileKey(f: File): string {
  return `${f.name}:${f.size}:${f.type}:${f.lastModified}`;
}

/**
 * True when the paste payload likely carries binary media even if File
 * extraction returned nothing (common for macOS screenshot → WKWebView).
 */
export function clipboardLooksLikeMedia(
  data: DataTransfer | null | undefined,
): boolean {
  if (!data) return false;
  const types = Array.from(data.types ?? []);
  if (types.some((t) => t === "Files" || t.startsWith("image/"))) return true;
  if (data.files && data.files.length > 0) return true;
  const items = data.items;
  if (items) {
    for (let i = 0; i < items.length; i++) {
      const item = items[i];
      if (!item) continue;
      if (item.kind === "file") return true;
      if (item.type.startsWith("image/")) return true;
    }
  }
  return false;
}

/**
 * Normalize layout-bearing characters from macOS / rich editors.
 *
 * Some clipboard producers flatten HTML lists into
 * `Heading:• one• two` in their text/plain flavor. Treat repeated typographic
 * bullets as list boundaries so the composer and sent message keep the layout
 * the user saw before copying.
 */
export function normalizePastedTextLayout(text: string): string {
  return text
    .replace(/\r\n/g, "\n")
    .replace(/\r/g, "\n")
    .replace(/[\u0085\u2028\u2029]/g, "\n")
    .replace(/([^\n])[ \t]*([•●◦▪▫‣⁃])[ \t]*/g, "$1\n$2 ");
}

/** Plain text from paste (normalized newlines and list boundaries). */
export function clipboardPlainText(
  data: DataTransfer | null | undefined,
): string {
  if (!data) return "";
  const plain =
    data.getData("text/plain") || data.getData("text") || "";
  return normalizePastedTextLayout(plain);
}

const HTML_BLOCK_TAGS = new Set([
  "ADDRESS",
  "ARTICLE",
  "ASIDE",
  "BLOCKQUOTE",
  "DIV",
  "DL",
  "DT",
  "DD",
  "FIGCAPTION",
  "FIGURE",
  "FOOTER",
  "HEADER",
  "H1",
  "H2",
  "H3",
  "H4",
  "H5",
  "H6",
  "MAIN",
  "NAV",
  "P",
  "PRE",
  "SECTION",
  "TR",
]);

function clipboardHtmlToText(html: string): string {
  if (!html || typeof DOMParser === "undefined") return "";
  try {
    const doc = new DOMParser().parseFromString(html, "text/html");
    const out: string[] = [];
    const newline = () => {
      if (out.length && !out[out.length - 1]!.endsWith("\n")) out.push("\n");
    };
    const walk = (node: Node) => {
      if (node.nodeType === Node.TEXT_NODE) {
        out.push(node.textContent ?? "");
        return;
      }
      if (node.nodeType !== Node.ELEMENT_NODE) return;
      const el = node as HTMLElement;
      if (el.tagName === "SCRIPT" || el.tagName === "STYLE") return;
      if (el.tagName === "BR") {
        newline();
        return;
      }
      if (el.tagName === "LI") {
        newline();
        out.push("• ");
        el.childNodes.forEach(walk);
        newline();
        return;
      }
      const block = HTML_BLOCK_TAGS.has(el.tagName);
      if (block) newline();
      el.childNodes.forEach(walk);
      if (block) newline();
    };
    doc.body.childNodes.forEach(walk);
    return normalizePastedTextLayout(out.join(""))
      .replace(/[ \t]+\n/g, "\n")
      .replace(/\n{3,}/g, "\n\n")
      .trim();
  } catch {
    return "";
  }
}

/**
 * Best plain-text representation of a rich clipboard.
 * HTML is used only when it carries more line structure than text/plain.
 */
export function clipboardTextPreservingLayout(
  data: DataTransfer | null | undefined,
): string {
  if (!data) return "";
  const plain = clipboardPlainText(data);
  const html = clipboardHtmlToText(data.getData("text/html") || "");
  if (!html) return plain;
  const plainLines = plain.split("\n").filter((line) => line.trim()).length;
  const htmlLines = html.split("\n").filter((line) => line.trim()).length;
  return htmlLines > plainLines ? html : plain;
}

/** file:// only paste — skip inserting as text when we already attached files. */
export function isFileUrlOnlyText(text: string): boolean {
  const t = text.trim();
  if (!t) return false;
  return /^file:\/\//i.test(t) && !t.includes("\n");
}

function extForMime(mime: string): string {
  const m = mime.split(";")[0]?.trim().toLowerCase() ?? "";
  if (m === "image/jpeg" || m === "image/jpg") return "jpg";
  if (m === "image/png") return "png";
  if (m === "image/gif") return "gif";
  if (m === "image/webp") return "webp";
  if (m === "image/bmp") return "bmp";
  if (m === "image/svg+xml") return "svg";
  if (m.startsWith("image/")) return m.slice("image/".length) || "png";
  return "bin";
}

/**
 * Async Clipboard API fallback (Chromium / some WKWebView builds).
 * Returns empty array when denied, unsupported, or no image items.
 */
export async function readClipboardMediaFiles(): Promise<File[]> {
  if (typeof navigator === "undefined" || !navigator.clipboard?.read) {
    return [];
  }
  try {
    const items = await navigator.clipboard.read();
    const out: File[] = [];
    for (const item of items) {
      for (const type of item.types) {
        if (!type.startsWith("image/") && type !== "application/pdf") continue;
        try {
          const blob = await item.getType(type);
          if (!blob || blob.size === 0) continue;
          const ext = extForMime(type);
          out.push(
            new File([blob], `paste.${ext}`, {
              type: type || blob.type || "application/octet-stream",
              lastModified: Date.now(),
            }),
          );
        } catch {
          /* type not readable */
        }
      }
    }
    return out;
  } catch {
    // NotAllowedError / empty clipboard / no permission
    return [];
  }
}
