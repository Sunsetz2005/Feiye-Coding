import type { ReactNode } from "react";

export interface WorkbenchShellProps {
  children: ReactNode;
}

/**
 * Structural boundary for the three-pane workbench.
 *
 * Pane state and Host coordination stay with App while the shell owns the
 * layout root. Sidebar, conversation, and resource regions intentionally
 * remain mounted by their callers so existing focus and session behavior is
 * unchanged during the staged extraction.
 */
export function WorkbenchShell({ children }: WorkbenchShellProps) {
  return (
    <div className="workbench" data-testid="workbench-shell">
      {children}
    </div>
  );
}
