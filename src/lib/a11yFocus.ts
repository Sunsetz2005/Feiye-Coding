/**
 * Small focus helpers for dialogs / permission bars (T15).
 * No React dependency — unit-testable.
 */

const FOCUSABLE_SEL = [
  "a[href]",
  "button:not([disabled])",
  "textarea:not([disabled])",
  "input:not([disabled]):not([type='hidden'])",
  "select:not([disabled])",
  "[tabindex]:not([tabindex='-1'])",
].join(",");

/** Visible, enabled focus targets inside `root`. */
export function listFocusable(root: ParentNode | null | undefined): HTMLElement[] {
  if (!root || typeof (root as Element).querySelectorAll !== "function") {
    return [];
  }
  const nodes = Array.from(
    (root as Element).querySelectorAll<HTMLElement>(FOCUSABLE_SEL),
  );
  return nodes.filter((el) => {
    if (el.hasAttribute("disabled")) return false;
    if (el.getAttribute("aria-hidden") === "true") return false;
    // offsetParent null for display:none (except fixed); still allow fixed.
    const style =
      typeof window !== "undefined" ? window.getComputedStyle(el) : null;
    if (style && (style.visibility === "hidden" || style.display === "none")) {
      return false;
    }
    return true;
  });
}

/** Focus the first focusable control; returns it or null. */
export function focusFirst(
  root: ParentNode | null | undefined,
): HTMLElement | null {
  const list = listFocusable(root);
  const el = list[0] ?? null;
  el?.focus();
  return el;
}

/**
 * Keep Tab / Shift+Tab cycling inside `root` (basic focus trap).
 * Call from keydown when the dialog is open.
 */
export function trapTabKey(
  e: { key: string; shiftKey: boolean; preventDefault: () => void },
  root: ParentNode | null | undefined,
): void {
  if (e.key !== "Tab") return;
  const list = listFocusable(root);
  if (list.length === 0) {
    e.preventDefault();
    return;
  }
  const first = list[0]!;
  const last = list[list.length - 1]!;
  const active =
    typeof document !== "undefined"
      ? (document.activeElement as HTMLElement | null)
      : null;

  if (e.shiftKey) {
    if (!active || active === first || !rootContains(root, active)) {
      e.preventDefault();
      last.focus();
    }
  } else if (!active || active === last || !rootContains(root, active)) {
    e.preventDefault();
    first.focus();
  }
}

function rootContains(
  root: ParentNode | null | undefined,
  el: Node | null,
): boolean {
  if (!root || !el) return false;
  if (root === el) return true;
  return typeof (root as Node).contains === "function"
    ? (root as Node).contains(el)
    : false;
}

/** Prefer primary allow / solid button when focusing a permission bar. */
export function preferPermissionFocus(
  root: ParentNode | null | undefined,
): HTMLElement | null {
  if (!root || typeof (root as Element).querySelector !== "function") {
    return focusFirst(root);
  }
  const allow = (root as Element).querySelector<HTMLElement>(
    ".perm-bar__btn--allow, .perm-bar__btn--session, .btn--primary, .btn--solid",
  );
  if (allow && !allow.hasAttribute("disabled")) {
    allow.focus();
    return allow;
  }
  return focusFirst(root);
}

const KEYBOARD_FOCUS_ATTR = "data-kb-focus";
const KEYBOARD_FOCUS_KEYS = new Set([
  "Tab",
  "ArrowUp",
  "ArrowDown",
  "ArrowLeft",
  "ArrowRight",
  "Home",
  "End",
  "Escape",
  "Enter",
  " ",
]);

type ChromeFocusOptions = FocusOptions & { focusVisible?: boolean };

function documentRoot(): HTMLElement | null {
  return typeof document === "undefined" ? null : document.documentElement;
}

/** Mark the document as keyboard-operated so chrome can show a focus ring. */
export function markKeyboardFocus(): void {
  documentRoot()?.setAttribute(KEYBOARD_FOCUS_ATTR, "true");
}

/** Clear keyboard-focus mode after pointer input. */
export function clearKeyboardFocus(): void {
  documentRoot()?.removeAttribute(KEYBOARD_FOCUS_ATTR);
}

/**
 * WebView2 does not always set `:focus-visible` for Tab. Pair that selector
 * with `html[data-kb-focus]` so restored pane triggers stay visible.
 */
export function installKeyboardFocusMode(
  doc: Document | null | undefined = typeof document === "undefined"
    ? null
    : document,
): () => void {
  if (!doc?.documentElement || typeof doc.addEventListener !== "function") {
    return () => {};
  }
  const root = doc.documentElement;
  const onKeyDown = (event: Event) => {
    const key = (event as KeyboardEvent).key;
    if (typeof key !== "string" || !KEYBOARD_FOCUS_KEYS.has(key)) return;
    const typed = event as KeyboardEvent;
    if (typed.metaKey || typed.ctrlKey || typed.altKey) return;
    root.setAttribute(KEYBOARD_FOCUS_ATTR, "true");
  };
  const onPointer = () => {
    root.removeAttribute(KEYBOARD_FOCUS_ATTR);
  };
  doc.addEventListener("keydown", onKeyDown, true);
  doc.addEventListener("mousedown", onPointer, true);
  doc.addEventListener("pointerdown", onPointer, true);
  return () => {
    doc.removeEventListener("keydown", onKeyDown, true);
    doc.removeEventListener("mousedown", onPointer, true);
    doc.removeEventListener("pointerdown", onPointer, true);
    root.removeAttribute(KEYBOARD_FOCUS_ATTR);
  };
}

/** Focus a control and request a visible keyboard ring when the engine allows it. */
export function focusElement(
  el: HTMLElement | null | undefined,
): HTMLElement | null {
  if (!el || typeof el.focus !== "function") return null;
  markKeyboardFocus();
  const options: ChromeFocusOptions = {
    preventScroll: true,
    focusVisible: true,
  };
  try {
    el.focus(options);
  } catch {
    el.focus();
  }
  return el;
}

/**
 * Restore focus after a pane unmounts. Retry a few frames so a newly mounted
 * trigger (Show sidebar) is attached before we give up.
 */
export function scheduleFocusRestore(
  getTarget: () => HTMLElement | null | undefined,
  attempts = 4,
): () => void {
  let cancelled = false;
  let remaining = Math.max(1, attempts);
  let frame = 0;
  let timer = 0;

  const queue = (cb: () => void) => {
    if (typeof requestAnimationFrame === "function") {
      frame = requestAnimationFrame(cb);
      return;
    }
    timer = setTimeout(cb, 0) as unknown as number;
  };

  const run = () => {
    if (cancelled) return;
    const el = getTarget();
    if (el) {
      focusElement(el);
      return;
    }
    remaining -= 1;
    if (remaining <= 0) return;
    queue(run);
  };

  queue(run);

  return () => {
    cancelled = true;
    if (frame && typeof cancelAnimationFrame === "function") {
      cancelAnimationFrame(frame);
    }
    if (timer) clearTimeout(timer);
  };
}
