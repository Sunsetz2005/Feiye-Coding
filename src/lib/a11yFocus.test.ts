import { afterEach, describe, expect, it, vi } from "vitest";
import {
  clearKeyboardFocus,
  focusElement,
  focusFirst,
  installKeyboardFocusMode,
  listFocusable,
  markKeyboardFocus,
  preferPermissionFocus,
  scheduleFocusRestore,
  trapTabKey,
} from "./a11yFocus";

type FakeEl = HTMLElement & {
  id: string;
  disabled?: boolean;
  className?: string;
  _focus: ReturnType<typeof vi.fn>;
};

function fakeEl(
  id: string,
  opts?: { disabled?: boolean; className?: string },
): FakeEl {
  const focus = vi.fn();
  const el = {
    id,
    className: opts?.className ?? "",
    hasAttribute: (n: string) => n === "disabled" && !!opts?.disabled,
    getAttribute: (_n: string) => null as string | null,
    focus,
    _focus: focus,
  };
  return el as unknown as FakeEl;
}

function fakeRoot(els: FakeEl[]): ParentNode {
  return {
    querySelectorAll: () => els,
    querySelector: (sel: string) => {
      if (sel.includes("perm-bar__btn--allow")) {
        return els.find((e) => e.className.includes("perm-bar__btn--allow")) ?? null;
      }
      return els[0] ?? null;
    },
    contains: (n: Node) => els.includes(n as FakeEl),
  } as unknown as ParentNode;
}

afterEach(() => {
  vi.unstubAllGlobals();
  vi.useRealTimers();
});

describe("listFocusable / focusFirst", () => {
  it("lists enabled controls and skips disabled", () => {
    vi.stubGlobal("window", {
      getComputedStyle: () => ({ visibility: "visible", display: "block" }),
    });
    const a = fakeEl("a");
    const b = fakeEl("b");
    const disabled = fakeEl("x", { disabled: true });
    // disabled has hasAttribute true — listFocusable filters it
    const root = fakeRoot([disabled, a, b]);
    // querySelectorAll returns all; listFocusable filters disabled
    const list = listFocusable(root);
    expect(list.map((e) => (e as FakeEl).id)).toEqual(["a", "b"]);
    const focused = focusFirst(root);
    expect((focused as FakeEl | null)?.id).toBe("a");
    expect(a._focus).toHaveBeenCalled();
  });
});

describe("trapTabKey", () => {
  it("wraps from last to first on Tab", () => {
    vi.stubGlobal("window", {
      getComputedStyle: () => ({ visibility: "visible", display: "block" }),
    });
    const a = fakeEl("a");
    const b = fakeEl("b");
    const root = fakeRoot([a, b]);
    vi.stubGlobal("document", { activeElement: b });
    const e = {
      key: "Tab",
      shiftKey: false,
      preventDefault: vi.fn(),
    };
    trapTabKey(e, root);
    expect(e.preventDefault).toHaveBeenCalled();
    expect(a._focus).toHaveBeenCalled();
  });

  it("wraps from first to last on Shift+Tab", () => {
    vi.stubGlobal("window", {
      getComputedStyle: () => ({ visibility: "visible", display: "block" }),
    });
    const a = fakeEl("a");
    const b = fakeEl("b");
    const root = fakeRoot([a, b]);
    vi.stubGlobal("document", { activeElement: a });
    const e = {
      key: "Tab",
      shiftKey: true,
      preventDefault: vi.fn(),
    };
    trapTabKey(e, root);
    expect(e.preventDefault).toHaveBeenCalled();
    expect(b._focus).toHaveBeenCalled();
  });
});

describe("preferPermissionFocus", () => {
  it("focuses allow button when present", () => {
    vi.stubGlobal("window", {
      getComputedStyle: () => ({ visibility: "visible", display: "block" }),
    });
    const deny = fakeEl("deny", { className: "perm-bar__btn--deny" });
    const ok = fakeEl("ok", { className: "perm-bar__btn--allow" });
    const root = fakeRoot([deny, ok]);
    const el = preferPermissionFocus(root);
    expect((el as FakeEl | null)?.id).toBe("ok");
    expect(ok._focus).toHaveBeenCalled();
  });
});

describe("focusElement", () => {
  it("requests a visible keyboard ring and falls back without options", () => {
    const focus = vi.fn();
    const el = { focus } as unknown as HTMLElement;
    expect(focusElement(el)).toBe(el);
    expect(focus).toHaveBeenCalledWith({
      preventScroll: true,
      focusVisible: true,
    });

    const failing = vi.fn((options?: unknown) => {
      if (options) throw new Error("options unsupported");
    });
    focusElement({ focus: failing } as unknown as HTMLElement);
    expect(failing).toHaveBeenCalledTimes(2);
    expect(focusElement(null)).toBeNull();
    expect(focusElement({} as HTMLElement)).toBeNull();
  });
});

describe("scheduleFocusRestore", () => {
  it("retries until the trigger exists", () => {
    vi.useFakeTimers();
    let target: HTMLElement | null = null;
    const focus = vi.fn();
    const cancel = scheduleFocusRestore(() => target, 3);
    vi.runOnlyPendingTimers();
    expect(focus).not.toHaveBeenCalled();
    target = { focus } as unknown as HTMLElement;
    vi.runOnlyPendingTimers();
    expect(focus).toHaveBeenCalledWith({
      preventScroll: true,
      focusVisible: true,
    });
    cancel();
  });

  it("does not focus after cancel", () => {
    vi.useFakeTimers();
    const focus = vi.fn();
    const cancel = scheduleFocusRestore(
      () => ({ focus }) as unknown as HTMLElement,
      2,
    );
    cancel();
    vi.runOnlyPendingTimers();
    expect(focus).not.toHaveBeenCalled();
  });

  it("gives up when the trigger never appears", () => {
    vi.useFakeTimers();
    scheduleFocusRestore(() => null, 2);
    vi.runOnlyPendingTimers();
    vi.runOnlyPendingTimers();
    vi.runOnlyPendingTimers();
  });

  it("uses requestAnimationFrame when available", () => {
    const cancelAnimationFrame = vi.fn();
    const requestAnimationFrame = vi.fn((cb: FrameRequestCallback) => {
      cb(0);
      return 7;
    });
    vi.stubGlobal("requestAnimationFrame", requestAnimationFrame);
    vi.stubGlobal("cancelAnimationFrame", cancelAnimationFrame);
    const focus = vi.fn();
    const stop = scheduleFocusRestore(
      () => ({ focus }) as unknown as HTMLElement,
      1,
    );
    expect(requestAnimationFrame).toHaveBeenCalled();
    expect(focus).toHaveBeenCalled();
    stop();
    expect(cancelAnimationFrame).toHaveBeenCalled();
  });
});

describe("installKeyboardFocusMode", () => {
  it("sets keyboard mode on Tab and clears it on pointer input", () => {
    const listeners = new Map<string, Set<EventListener>>();
    const root = {
      setAttribute: vi.fn(),
      removeAttribute: vi.fn(),
    };
    const doc = {
      documentElement: root,
      addEventListener: (type: string, fn: EventListener) => {
        const set = listeners.get(type) ?? new Set();
        set.add(fn);
        listeners.set(type, set);
      },
      removeEventListener: (type: string, fn: EventListener) => {
        listeners.get(type)?.delete(fn);
      },
    } as unknown as Document;

    const stop = installKeyboardFocusMode(doc);
    for (const fn of listeners.get("keydown") ?? []) {
      fn({
        key: "Tab",
        metaKey: false,
        ctrlKey: false,
        altKey: false,
      } as unknown as Event);
    }
    expect(root.setAttribute).toHaveBeenCalledWith("data-kb-focus", "true");

    for (const fn of listeners.get("pointerdown") ?? []) {
      fn(new Event("pointerdown"));
    }
    expect(root.removeAttribute).toHaveBeenCalledWith("data-kb-focus");

    stop();
    expect(listeners.get("keydown")?.size ?? 0).toBe(0);
  });

  it("ignores modified keys, unknown keys, and missing documents", () => {
    expect(installKeyboardFocusMode()()).toBeUndefined();
    expect(installKeyboardFocusMode(null)()).toBeUndefined();
    expect(
      installKeyboardFocusMode({ documentElement: {} } as Document)(),
    ).toBeUndefined();
    const root = { setAttribute: vi.fn(), removeAttribute: vi.fn() };
    const listeners = new Map<string, EventListener>();
    const doc = {
      documentElement: root,
      addEventListener: (type: string, fn: EventListener) => {
        listeners.set(type, fn);
      },
      removeEventListener: vi.fn(),
    } as unknown as Document;
    installKeyboardFocusMode(doc);
    const keydown = listeners.get("keydown");
    keydown?.({
      key: "Tab",
      metaKey: true,
      ctrlKey: false,
      altKey: false,
    } as unknown as Event);
    keydown?.({
      key: "Tab",
      metaKey: false,
      ctrlKey: true,
      altKey: false,
    } as unknown as Event);
    keydown?.({
      key: "Tab",
      metaKey: false,
      ctrlKey: false,
      altKey: true,
    } as unknown as Event);
    keydown?.({
      key: "a",
      metaKey: false,
      ctrlKey: false,
      altKey: false,
    } as unknown as Event);
    keydown?.({ key: 1 } as unknown as Event);
    expect(root.setAttribute).not.toHaveBeenCalled();
    listeners.get("mousedown")?.(new Event("mousedown"));
    expect(root.removeAttribute).toHaveBeenCalledWith("data-kb-focus");
  });
});

describe("keyboard focus flags", () => {
  it("no-ops without a document", () => {
    markKeyboardFocus();
    clearKeyboardFocus();
  });

  it("writes the keyboard focus attribute when a document exists", () => {
    const setAttribute = vi.fn();
    const removeAttribute = vi.fn();
    vi.stubGlobal("document", {
      documentElement: { setAttribute, removeAttribute },
    });
    markKeyboardFocus();
    clearKeyboardFocus();
    expect(setAttribute).toHaveBeenCalledWith("data-kb-focus", "true");
    expect(removeAttribute).toHaveBeenCalledWith("data-kb-focus");
  });
});
