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
import { IconChevronRight } from "./icons";

export type ContextMenuItem = {
  id?: string;
  label: ReactNode;
  icon?: ReactNode;
  danger?: boolean;
  disabled?: boolean;
  separatorBefore?: boolean;
  shortcut?: ReactNode;
  onClick?: () => void;
  submenu?: ContextMenuItem[];
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
  const submenuItemRefs = useRef<Array<HTMLButtonElement | null>>([]);
  const [activeIndex, setActiveIndex] = useState(-1);
  const [submenuIndex, setSubmenuIndex] = useState<number | null>(null);
  const [submenuPos, setSubmenuPos] = useState({ left: 0, top: 0 });
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
    setSubmenuIndex(null);
    if (first >= 0) {
      requestAnimationFrame(() => itemRefs.current[first]?.focus());
    }
  }, [open, items.length]);

  useLayoutEffect(() => {
    if (submenuIndex == null) return;
    const trigger = itemRefs.current[submenuIndex];
    const submenu = items[submenuIndex]?.submenu;
    if (!trigger || !submenu?.length || typeof window === "undefined") return;
    const rect = trigger.getBoundingClientRect();
    const width = 220;
    const height = Math.max(48, submenu.length * 32 + 8);
    let left = rect.right + 4;
    if (left + width > window.innerWidth - 8) {
      left = Math.max(8, rect.left - width - 4);
    }
    let top = rect.top;
    if (top + height > window.innerHeight - 8) {
      top = Math.max(8, window.innerHeight - height - 8);
    }
    setSubmenuPos({ left, top });
  }, [submenuIndex, items]);

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
      if (e.key !== "Escape") return;
      e.preventDefault();
      if (submenuIndex != null) {
        setSubmenuIndex(null);
        itemRefs.current[submenuIndex]?.focus();
        return;
      }
      closeAndRestore();
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
  }, [open, onClose, restoreFocusTo, submenuIndex]);

  if (!open || typeof document === "undefined") return null;

  const visibleItems = items.filter(Boolean);
  const openSubmenu =
    submenuIndex != null ? visibleItems[submenuIndex]?.submenu : undefined;
  const closeRoot = () => {
    setSubmenuIndex(null);
    onClose();
  };
  const activate = (item: ContextMenuItem) => {
    if (item.disabled) return;
    if (item.submenu?.length) return;
    closeRoot();
    item.onClick?.();
  };

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
        if (e.key === "ArrowRight") {
          const item = visibleItems[activeIndex];
          if (item?.submenu?.length && !item.disabled) {
            e.preventDefault();
            setSubmenuIndex(activeIndex);
            requestAnimationFrame(() => submenuItemRefs.current[0]?.focus());
          }
          return;
        }
        if (e.key === "ArrowLeft" && submenuIndex != null) {
          e.preventDefault();
          setSubmenuIndex(null);
          itemRefs.current[submenuIndex]?.focus();
          return;
        }
        if (submenuIndex != null && (e.key === "ArrowDown" || e.key === "ArrowUp" || e.key === "Home" || e.key === "End")) {
          const submenu = visibleItems[submenuIndex]?.submenu ?? [];
          const enabled = submenu
            .map((item, index) => ({ item, index }))
            .filter(({ item }) => !item.disabled);
          if (!enabled.length) return;
          const focused = submenuItemRefs.current.findIndex(
            (node) => node === document.activeElement,
          );
          const current = Math.max(
            0,
            enabled.findIndex(({ index }) => index === focused),
          );
          let next = current;
          if (e.key === "ArrowDown") next = (current + 1) % enabled.length;
          else if (e.key === "ArrowUp")
            next = (current - 1 + enabled.length) % enabled.length;
          else if (e.key === "Home") next = 0;
          else next = enabled.length - 1;
          e.preventDefault();
          submenuItemRefs.current[enabled[next]?.index ?? 0]?.focus();
          return;
        }
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
          closeRoot();
          return;
        } else {
          return;
        }
        e.preventDefault();
        const index = enabled[next]?.index ?? enabled[0].index;
        setActiveIndex(index);
        setSubmenuIndex(null);
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
            aria-haspopup={item.submenu?.length ? "menu" : undefined}
            aria-expanded={
              item.submenu?.length ? submenuIndex === i : undefined
            }
            onFocus={() => setActiveIndex(i)}
            onMouseEnter={() => {
              if (item.disabled) return;
              setSubmenuIndex(item.submenu?.length ? i : null);
            }}
            onClick={() => {
              if (item.disabled) return;
              if (item.submenu?.length) {
                setSubmenuIndex(i);
                return;
              }
              activate(item);
            }}
          >
            {item.icon != null ? (
              <span className="context-menu__ico att-menu__ico" aria-hidden>
                {item.icon}
              </span>
            ) : null}
            <span className="context-menu__label">{item.label}</span>
            {item.submenu?.length ? (
              <span className="context-menu__caret" aria-hidden>
                <IconChevronRight size={14} />
              </span>
            ) : item.shortcut != null ? (
              <span className="context-menu__shortcut" aria-hidden>
                {item.shortcut}
              </span>
            ) : null}
          </button>
        </div>
      ))}
      {openSubmenu?.length && submenuIndex != null ? (
        <div
          className="menu-panel context-menu att-menu context-menu--cascade"
          role="menu"
          aria-orientation="vertical"
          style={{ left: submenuPos.left, top: submenuPos.top }}
          onMouseDown={(e) => e.stopPropagation()}
        >
          {openSubmenu.map((item, i) => (
            <div key={item.id ?? `ctx-sub-${i}`} role="presentation">
              {item.separatorBefore ? (
                <div className="context-menu__separator" role="separator" />
              ) : null}
              <button
                ref={(node) => {
                  submenuItemRefs.current[i] = node;
                }}
                type="button"
                className={cx(
                  "context-menu__item",
                  "att-menu__item",
                  item.danger && "is-danger",
                )}
                role="menuitem"
                tabIndex={-1}
                disabled={item.disabled}
                onClick={() => activate(item)}
              >
                {item.icon != null ? (
                  <span className="context-menu__ico att-menu__ico" aria-hidden>
                    {item.icon}
                  </span>
                ) : null}
                <span className="context-menu__label">{item.label}</span>
              </button>
            </div>
          ))}
        </div>
      ) : null}
      {extra}
    </div>,
    document.body,
  );
}
