import {
  useEffect,
  useRef,
  type ReactNode,
  type RefObject,
} from "react";

export interface WorkbenchShellProps {
  children: ReactNode;
  sidebarCollapsed?: boolean;
  asideCollapsed?: boolean;
  sidebarToggleRef?: RefObject<HTMLElement | null>;
  asideToggleRef?: RefObject<HTMLElement | null>;
}

function scheduleFocus(target: RefObject<HTMLElement | null>) {
  const run = () => target.current?.focus({ preventScroll: true });
  if (typeof window.requestAnimationFrame === "function") {
    const frame = window.requestAnimationFrame(run);
    return () => window.cancelAnimationFrame(frame);
  }
  const timer = window.setTimeout(run, 0);
  return () => window.clearTimeout(timer);
}

/**
 * Structural boundary for the three-pane workbench.
 *
 * Pane state and Host coordination stay with App. The shell owns the layout
 * root and restores focus to the corresponding top-bar control after a pane
 * closes. Conversation content remains mounted, so changing pane visibility
 * preserves its native scroll position.
 */
export function WorkbenchShell({
  children,
  sidebarCollapsed,
  asideCollapsed,
  sidebarToggleRef,
  asideToggleRef,
}: WorkbenchShellProps) {
  const previousSidebarCollapsed = useRef(sidebarCollapsed);
  const previousAsideCollapsed = useRef(asideCollapsed);

  useEffect(() => {
    const justClosed =
      previousSidebarCollapsed.current === false && sidebarCollapsed === true;
    previousSidebarCollapsed.current = sidebarCollapsed;
    if (!justClosed || !sidebarToggleRef) return;
    return scheduleFocus(sidebarToggleRef);
  }, [sidebarCollapsed, sidebarToggleRef]);

  useEffect(() => {
    const justClosed =
      previousAsideCollapsed.current === false && asideCollapsed === true;
    previousAsideCollapsed.current = asideCollapsed;
    if (!justClosed || !asideToggleRef) return;
    return scheduleFocus(asideToggleRef);
  }, [asideCollapsed, asideToggleRef]);

  return (
    <div className="workbench" data-testid="workbench-shell">
      {children}
    </div>
  );
}
