/**
 * Composer draft document model: text segments + inline skill chips.
 * Recovery storage can bind a chip to an exact Skill identity with a v1 token;
 * user-visible journals keep the stable legacy `[[skill:name]]` marker.
 * Agent prompts serialize skills as `/name` (Grok Build invocable form).
 */

export type SkillBindingV1 = {
  version: 1;
  id: string;
  expectedTreeHash: string;
  selection: "explicit" | "accepted_suggestion";
};

export type DraftSegment =
  | { type: "text"; text: string }
  | { type: "skill"; name: string; binding?: SkillBindingV1 };

/** Skill name character class: letters, digits, `_` `.` `:` `-`. */
const SKILL_NAME_SOURCE = "[a-zA-Z0-9_.:-]+";
export const SKILL_NAME_RE = new RegExp(`^${SKILL_NAME_SOURCE}$`);

const HEX_64_SOURCE = "[a-fA-F0-9]{64}";
const SKILL_TOKEN_RE = new RegExp(
  `^\\[\\[skill:(${SKILL_NAME_SOURCE})\\]\\]$`,
);
const SKILL_V1_TOKEN_RE = new RegExp(
  `^\\[\\[skill-v1:(${SKILL_NAME_SOURCE})\\|(${HEX_64_SOURCE})\\|(${HEX_64_SOURCE})\\|(explicit|accepted_suggestion)\\]\\]$`,
);
const LEGACY_SKILL_TOKEN_PREFIX = "[[skill:";
const V1_SKILL_TOKEN_PREFIX = "[[skill-v1:";

function validHex64(value: unknown): value is string {
  return typeof value === "string" && /^[a-fA-F0-9]{64}$/.test(value);
}

function validSkillName(value: unknown): value is string {
  return typeof value === "string" && SKILL_NAME_RE.test(value);
}

function validSkillBinding(value: unknown): value is SkillBindingV1 {
  if (!value || typeof value !== "object") return false;
  const binding = value as Partial<SkillBindingV1>;
  return (
    binding.version === 1 &&
    validHex64(binding.id) &&
    validHex64(binding.expectedTreeHash) &&
    (binding.selection === "explicit" ||
      binding.selection === "accepted_suggestion")
  );
}

function serializeSkillToken(
  segment: Extract<DraftSegment, { type: "skill" }>,
  displayOnly = false,
): string {
  if (!validSkillName(segment.name)) {
    throw new Error("invalid Skill name in draft segment");
  }
  if (segment.binding !== undefined && !validSkillBinding(segment.binding)) {
    throw new Error("invalid Skill binding in draft segment");
  }
  if (displayOnly || segment.binding === undefined) {
    return `[[skill:${segment.name}]]`;
  }
  return `[[skill-v1:${segment.name}|${segment.binding.id}|${segment.binding.expectedTreeHash}|${segment.binding.selection}]]`;
}

/**
 * Slash names that are App/Build commands, not skill chips, when rehydrating
 * agent-form history (`/name` lines saved from session_send).
 */
const NON_SKILL_SLASH = new Set(
  [
    "goal",
    "plan",
    "compact",
    "status",
    "mcp",
    "doctor",
    "new",
    "newchat",
    "automations",
    "settings",
    "yolo",
    "always-approve",
    "loop",
    "model",
    "effort",
    "help",
    "clear",
    "resume",
    "export",
    "copy",
    "feedback",
  ].map((s) => s.toLowerCase()),
);

/**
 * Convert agent-form user text (`/skill-name\nbody`) into display tokens
 * (`[[skill:name]]\nbody`) so history bubbles can render chips.
 * Already-tokenized content is left unchanged.
 */
export function hydrateDisplayContent(content: string): string {
  if (!content) return content;
  if (
    content.includes(LEGACY_SKILL_TOKEN_PREFIX) ||
    content.includes(V1_SKILL_TOKEN_PREFIX)
  ) {
    return content;
  }

  let rest = content;
  // Drop goal mode prefix from display hydration (mode is session chrome, not a chip).
  if (rest.startsWith("/goal\n")) {
    rest = rest.slice("/goal\n".length);
  } else if (rest === "/goal") {
    return content;
  }

  const nl = rest.indexOf("\n");
  const firstLine = (nl === -1 ? rest : rest.slice(0, nl)).trim();
  const body = nl === -1 ? "" : rest.slice(nl + 1);

  if (!firstLine) return content;

  const parts = firstLine.split(/\s+/).filter(Boolean);
  if (parts.length === 0) return content;
  if (!parts.every((p) => /^\/[a-zA-Z0-9_.:-]+$/.test(p))) return content;

  const names = parts.map((p) => p.slice(1));
  // Require at least one invocable skill; skip pure built-in command lines.
  const skillNames = names.filter(
    (n) => !NON_SKILL_SLASH.has(n.toLowerCase()),
  );
  if (skillNames.length === 0) return content;
  // Only convert when every first-line token is a skill (not mixed with builtins).
  if (skillNames.length !== names.length) return content;

  const chips = skillNames.map((n) => `[[skill:${n}]]`).join("");
  if (!body) return chips;
  // Preserve body; chips sit before the rest of the message.
  return `${chips}\n${body}`;
}

/** Parse user message for display/edit (hydrates agent-form history first). */
export function parseUserMessageContent(content: string): DraftSegment[] {
  return parseStoredContent(hydrateDisplayContent(content));
}

/** Empty draft (no segments). */
export function emptyDraft(): DraftSegment[] {
  return [];
}

/** Single text segment, or empty draft when text is empty. */
export function draftFromPlainText(text: string): DraftSegment[] {
  if (!text) return [];
  return [{ type: "text", text }];
}

/**
 * Parse legacy `[[skill:name]]` and identity-bound `[[skill-v1:…]]` tokens.
 * Invalid / incomplete v1 tokens stay as plain text and never downgrade to an
 * unbound Skill segment.
 */
export function parseStoredContent(content: string): DraftSegment[] {
  if (!content) return [];
  const segments: DraftSegment[] = [];
  const pushText = (text: string) => {
    if (!text) return;
    const previous = segments[segments.length - 1];
    if (previous?.type === "text") {
      previous.text += text;
    } else {
      segments.push({ type: "text", text });
    }
  };
  let cursor = 0;
  while (cursor < content.length) {
    const legacyStart = content.indexOf(LEGACY_SKILL_TOKEN_PREFIX, cursor);
    const v1Start = content.indexOf(V1_SKILL_TOKEN_PREFIX, cursor);
    const starts = [legacyStart, v1Start].filter((index) => index >= 0);
    if (starts.length === 0) {
      pushText(content.slice(cursor));
      break;
    }
    const start = Math.min(...starts);
    pushText(content.slice(cursor, start));
    const isV1 = start === v1Start;
    const end = content.indexOf("]]", start);
    if (end < 0) {
      pushText(content.slice(start));
      break;
    }
    const rawToken = content.slice(start, end + 2);
    const match = isV1
      ? SKILL_V1_TOKEN_RE.exec(rawToken)
      : SKILL_TOKEN_RE.exec(rawToken);
    if (isV1 && match) {
      segments.push({
        type: "skill",
        name: match[1]!,
        binding: {
          version: 1,
          id: match[2]!,
          expectedTreeHash: match[3]!,
          selection: match[4] as SkillBindingV1["selection"],
        },
      });
    } else if (!isV1 && match) {
      segments.push({ type: "skill", name: match[1]! });
    } else {
      pushText(rawToken);
    }
    cursor = end + 2;
  }
  return segments;
}

/** Serialize recovery/editor storage, preserving exact v1 Skill bindings. */
export function serializeStored(segments: DraftSegment[]): string {
  return segments
    .map((segment) =>
      segment.type === "text" ? segment.text : serializeSkillToken(segment),
    )
    .join("");
}

/**
 * Serialize the user-visible journal form. Identity and selection metadata are
 * deliberately omitted; the Host verifies them through the separate v2 DTO.
 */
export function serializeDisplayForJournal(segments: DraftSegment[]): string {
  return segments
    .map((segment) =>
      segment.type === "text"
        ? segment.text
        : serializeSkillToken(segment, true),
    )
    .join("");
}

/**
 * Replace valid legacy and v1 Skill tokens with `/name` in one-line previews.
 * Invalid v1-shaped text stays visible as text.
 * (queue strip, titles). Keeps surrounding text order — unlike
 * {@link serializeForAgent}, which groups skills first.
 */
export function previewStoredAsSlash(stored: string): string {
  if (!stored) return stored;
  return parseStoredContent(stored)
    .map((segment) =>
      segment.type === "text" ? segment.text : `/${segment.name}`,
    )
    .join("");
}

/**
 * Text of text segments only (skills omitted).
 * Do not use alone for "has content" when skills may be present — use `isDraftEmpty`.
 */
export function plainTextOf(segments: DraftSegment[]): string {
  return segments
    .filter((s): s is { type: "text"; text: string } => s.type === "text")
    .map((s) => s.text)
    .join("");
}

/** Empty when there are no skills and no non-whitespace text. */
export function isDraftEmpty(segments: DraftSegment[]): boolean {
  for (const s of segments) {
    if (s.type === "skill") return false;
    if (s.type === "text" && s.text.trim() !== "") return false;
  }
  return true;
}

/**
 * Build the string sent to the agent:
 * - skills in order as `/name`, space-joined
 * - then `\n` + joined text parts (ends trimmed; internal newlines kept)
 * - `goalMode` prefixes `/goal\n`
 */
export function serializeForAgent(
  segments: DraftSegment[],
  opts?: { goalMode?: boolean },
): string {
  const skillTokens: string[] = [];
  const textParts: string[] = [];
  for (const s of segments) {
    if (s.type === "skill") skillTokens.push(`/${s.name}`);
    else textParts.push(s.text);
  }

  const skillsPart = skillTokens.join(" ");
  // Trim only leading/trailing whitespace; keep internal newlines.
  const textPart = textParts.join("").replace(/^\s+/, "").replace(/\s+$/, "");

  let body: string;
  if (skillsPart && textPart) body = `${skillsPart}\n${textPart}`;
  else if (skillsPart) body = skillsPart;
  else body = textPart;

  if (opts?.goalMode) {
    return body ? `/goal\n${body}` : "/goal";
  }
  return body;
}

/**
 * Replace the active slash range `[slashStart, slashEnd)` with a skill token
 * plus a trailing space.
 */
export function applySkillAtSlash(
  stored: string,
  slashStart: number,
  slashEnd: number,
  skillName: string,
  binding?: SkillBindingV1,
): string {
  const token = `${serializeSkillToken({
    type: "skill",
    name: skillName,
    ...(binding !== undefined ? { binding } : {}),
  })} `;
  return stored.slice(0, slashStart) + token + stored.slice(slashEnd);
}

/**
 * Plain text as shown in a contenteditable (not React draft state).
 * Prefer this for live slash filtering — draft/onChange often lags IME.
 */
export function readPlainEditorText(el: HTMLElement): string {
  let t = el.innerText ?? el.textContent ?? "";
  t = t
    .replace(/\u00a0/g, " ")
    .replace(/\r\n/g, "\n")
    .replace(/\r/g, "\n")
    .replace(/\uFF0F/g, "/") // fullwidth solidus
    .replace(/[\u200B-\u200D\uFEFF\u2060]/g, ""); // zero-width
  return t;
}

/**
 * Detect an active slash token at the end of `textBeforeCursor`.
 * `/` must be at index 0 or immediately after whitespace.
 * Query is the non-whitespace rest after `/`.
 * Returns null when there is no active slash (e.g. `https://`).
 *
 * Contenteditable almost always serializes a trailing `\n` (from `<br>`).
 * Without trimming, `/目标\n` fails `$` anchor and filtering looks "broken".
 */
export function detectSlashQuery(
  textBeforeCursor: string,
): { start: number; query: string } | null {
  const text = textBeforeCursor
    .replace(/\uFF0F/g, "/")
    .replace(/[\u200B-\u200D\uFEFF\u2060]/g, "")
    .replace(/[\s\u00a0]+$/u, "");
  const m = /(^|[\s])\/([^\s]*)$/u.exec(text);
  if (!m) return null;
  const start = m.index + m[1]!.length;
  return { start, query: m[2]! };
}

/** Live slash token from a contenteditable element (what the user sees). */
export function detectSlashQueryFromEditor(
  el: HTMLElement | null | undefined,
): { start: number; query: string; end: number } | null {
  if (!el) return null;
  // Try a few normalizations — WebKit IME / contenteditable are messy.
  const raw = readPlainEditorText(el);
  const candidates = [
    raw,
    raw.replace(/\n+/g, "\n"),
    raw.replace(/\n/g, ""),
    // last line only (slash menus are almost always at the caret line)
    raw.split("\n").filter(Boolean).pop() ?? raw,
  ];
  for (const text of candidates) {
    const q = detectSlashQuery(text);
    if (q) {
      const trimmed = text.replace(/[\s\u00a0]+$/u, "");
      return { start: q.start, query: q.query, end: trimmed.length };
    }
  }
  return null;
}

/** Collapse consecutive text segments into one. */
export function mergeAdjacentText(segments: DraftSegment[]): DraftSegment[] {
  if (segments.length === 0) return [];
  const out: DraftSegment[] = [];
  for (const s of segments) {
    const prev = out[out.length - 1];
    if (s.type === "text" && prev?.type === "text") {
      out[out.length - 1] = { type: "text", text: prev.text + s.text };
    } else {
      out.push(s);
    }
  }
  return out;
}

/**
 * Editor projection uses the recovery storage form so identity bindings survive
 * contenteditable round-trips. Journals must use `serializeDisplayForJournal`.
 */
export function segmentsToPlainEditorText(segments: DraftSegment[]): string {
  return serializeStored(segments);
}
