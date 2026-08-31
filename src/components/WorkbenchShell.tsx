import {
  useEffect,
  useRef,
  type ReactNode,
  type RefObject,
} from "react";
import { scheduleFocusRestore } from "@/lib/a11yFocus";

export interface WorkbenchShellProps {
  children: ReactNode;
  sidebarCollapsed?: boolean;
  asideCollapsed?: boolean;
  sidebarToggleRef?: RefObject<HTMLElement | null>;
  asideToggleRef?: RefObject<HTMLElement | null>;
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
  const restoreSidebar = useRef(false);
  const restoreAside = useRef(false);

  if (previousSidebarCollapsed.current === false && sidebarCollapsed === true) {
    restoreSidebar.current = true;
  }
  if (sidebarCollapsed === false) restoreSidebar.current = false;
  previousSidebarCollapsed.current = sidebarCollapsed;

  if (previousAsideCollapsed.current === false && asideCollapsed === true) {
    restoreAside.current = true;
  }
  if (asideCollapsed === false) restoreAside.current = false;
  previousAsideCollapsed.current = asideCollapsed;

  useEffect(() => {
    if (!restoreSidebar.current || !sidebarToggleRef) return;
    return scheduleFocusRestore(() => {
      const el = sidebarToggleRef.current;
      if (el) restoreSidebar.current = false;
      return el;
    });
  }, [sidebarCollapsed, sidebarToggleRef]);

  useEffect(() => {
    if (!restoreAside.current || !asideToggleRef) return;
    return scheduleFocusRestore(() => {
      const el = asideToggleRef.current;
      if (el) restoreAside.current = false;
      return el;
    });
  }, [asideCollapsed, asideToggleRef]);

  return (
    <div className="workbench" data-testid="workbench-shell">
      {children}
    </div>
  );
}
