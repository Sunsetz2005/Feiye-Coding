/**
 * Unified right-click / context menu (chat att-menu visual baseline).
 *
 * Solid surface, compact padding, optional leading icon.
 * Always portaled to document.body; closes on outside mousedown + Escape.
 *
 * Usage:
 *   <ContextMenu
 *     open={!!menu}
 *     x={menu.x}
 *     y={menu.y}
 *     onClose={() => setMenu(null)}
 *     items={[{ label: "…", icon: <Icon… />, onClick: () => { … } }]}
 *   />
 */

import {
  useEffect,
  useId,
  useLayoutEffect,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { createPortal } from "react-dom";
import { useFloatingSurfacePresence } from "./FloatingSurfaceProvider";

export type ContextMenuItem = {
  id?: string;
  label: ReactNode;
  icon?: ReactNode;
  danger?: boolean;
  disabled?: boolean;
  separatorBefore?: boolean;
  shortcut?: ReactNode;
  onClick: () => void;
};

export type ContextMenuAnchor = Pick<
  DOMRect,
  "left" | "right" | "top" | "bottom" | "width" | "height"
>;

export type ContextMenuProps = {
  open: boolean;
  x: number;
  y: number;
  items: ContextMenuItem[];
  onClose: () => void;
  /** Extra rows after items (legacy ImageUi/VideoUi slot). */
  extra?: ReactNode;
  className?: string;
  /** Used to clamp position near viewport edges. */
  estimatedWidth?: number;
  estimatedHeight?: number;
  /** Element-aligned menus use this instead of the pointer coordinates. */
  anchorRect?: ContextMenuAnchor | null;
  /** Restored after Escape/outside dismissal. */
  restoreFocusTo?: HTMLElement | null;
};

function cx(...parts: Array<string | false | null | undefined>) {
  return parts.filter(Boolean).join(" ");
}

/** Clamp menu anchor so the panel stays in viewport. */
export function clampContextMenuPos(
  x: number,
  y: number,
  width = 200,
  height = 220,
): { left: number; top: number } {
  if (typeof window === "undefined") return { left: x, top: y };
  return {
    left: Math.max(8, Math.min(x, window.innerWidth - width - 8)),
    top: Math.max(8, Math.min(y, window.innerHeight - height - 8)),
  };
}

export function contextMenuPosForAnchor(
  anchor: ContextMenuAnchor,
  width = 200,
  height = 220,
): { left: number; top: number } {
  if (typeof window === "undefined") {
    return { left: anchor.left, top: anchor.bottom + 6 };
  }
  const roomBelow = window.innerHeight - anchor.bottom - 8;
  const top =
    roomBelow >= height
      ? anchor.bottom + 6
      : Math.max(8, anchor.top - height - 6);
  return {
    left: Math.max(
      8,
      Math.min(anchor.left, window.innerWidth - width - 8),
    ),
    top,
  };
}

export function ContextMenu({
  open,
  x,
  y,
  items,
  onClose,
  extra,
  className,
  estimatedWidth = 200,
  estimatedHeight = 240,
  anchorRect,
  restoreFocusTo,
}: ContextMenuProps) {
  const menuId = useId();
  const rootRef = useRef<HTMLDivElement>(null);
  const itemRefs = useRef<Array<HTMLButtonElement | null>>([]);
  const [activeIndex, setActiveIndex] = useState(-1);
  const resolvePos = (width: number, height: number) =>
    anchorRect
      ? contextMenuPosForAnchor(anchorRect, width, height)
      : clampContextMenuPos(x, y, width, height);
  const [pos, setPos] = useState(() =>
    resolvePos(estimatedWidth, estimatedHeight),
  );
  useFloatingSurfacePresence({
    open,
    onRequestClose: onClose,
    id: `context-menu:${menuId}`,
  });

  useLayoutEffect(() => {
    if (!open) return;
    setPos(resolvePos(estimatedWidth, estimatedHeight));
  }, [open, x, y, anchorRect, estimatedWidth, estimatedHeight]);

  // After paint, re-clamp using real menu size if available.
  useLayoutEffect(() => {
    if (!open || !rootRef.current) return;
    const rect = rootRef.current.getBoundingClientRect();
    setPos(
      resolvePos(
        Math.ceil(rect.width) || estimatedWidth,
        Math.ceil(rect.height) || estimatedHeight,
      ),
    );
  }, [
    open,
    x,
    y,
    anchorRect,
    items.length,
    estimatedWidth,
    estimatedHeight,
  ]);

  useLayoutEffect(() => {
    if (!open) return;
    const first = items.findIndex((item) => !item.disabled);
    setActiveIndex(first);
    if (first >= 0) {
      requestAnimationFrame(() => itemRefs.current[first]?.focus());
    }
  }, [open, items.length]);

  useEffect(() => {
    if (!open) return;
    const closeAndRestore = () => {
      onClose();
      requestAnimationFrame(() => restoreFocusTo?.focus());
    };
    const onDoc = (e: MouseEvent) => {
      const t = e.target as HTMLElement | null;
      if (t?.closest?.(".context-menu")) return;
      closeAndRestore();
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        closeAndRestore();
      }
    };
    // Defer so the opening contextmenu / click does not immediately dismiss.
    const timer = window.setTimeout(() => {
      document.addEventListener("mousedown", onDoc, true);
    }, 0);
    document.addEventListener("keydown", onKey);
    return () => {
      window.clearTimeout(timer);
      document.removeEventListener("mousedown", onDoc, true);
      document.removeEventListener("keydown", onKey);
    };
  }, [open, onClose, restoreFocusTo]);

  if (!open || typeof document === "undefined") return null;

  const visibleItems = items.filter(Boolean);

  return createPortal(
    <div
      ref={rootRef}
      id={menuId}
      className={cx("menu-panel context-menu att-menu", className)}
      role="menu"
      aria-orientation="vertical"
      style={{
        left: pos.left,
        top: pos.top,
        transformOrigin:
          anchorRect && pos.top < anchorRect.top ? "bottom left" : "top left",
      }}
      onMouseDown={(e) => e.stopPropagation()}
      onKeyDown={(e) => {
        const enabled = visibleItems
          .map((item, index) => ({ item, index }))
          .filter(({ item }) => !item.disabled);
        if (!enabled.length) return;
        const current = enabled.findIndex(({ index }) => index === activeIndex);
        let next = current;
        if (e.key === "ArrowDown") next = (current + 1) % enabled.length;
        else if (e.key === "ArrowUp")
          next = (current - 1 + enabled.length) % enabled.length;
        else if (e.key === "Home") next = 0;
        else if (e.key === "End") next = enabled.length - 1;
        else if (e.key === "Tab") {
          onClose();
          return;
        } else {
          return;
        }
        e.preventDefault();
        const index = enabled[next]?.index ?? enabled[0].index;
        setActiveIndex(index);
        itemRefs.current[index]?.focus();
      }}
      onContextMenu={(e) => {
        e.preventDefault();
        e.stopPropagation();
      }}
    >
      {visibleItems.map((item, i) => (
        <div key={item.id ?? `ctx-item-${i}`} role="presentation">
          {item.separatorBefore ? (
            <div className="context-menu__separator" role="separator" />
          ) : null}
          <button
            ref={(node) => {
              itemRefs.current[i] = node;
            }}
            type="button"
            className={cx(
              "context-menu__item",
              "att-menu__item",
              item.danger && "is-danger",
            )}
            role="menuitem"
            tabIndex={activeIndex === i ? 0 : -1}
            disabled={item.disabled}
            onFocus={() => setActiveIndex(i)}
            onClick={() => {
              if (item.disabled) return;
              onClose();
              item.onClick();
            }}
          >
            {item.icon != null ? (
              <span className="context-menu__ico att-menu__ico" aria-hidden>
                {item.icon}
              </span>
            ) : null}
            <span className="context-menu__label">{item.label}</span>
            {item.shortcut != null ? (
              <span className="context-menu__shortcut" aria-hidden>
                {item.shortcut}
              </span>
            ) : null}
          </button>
        </div>
      ))}
      {extra}
    </div>,
    document.body,
  );
}
