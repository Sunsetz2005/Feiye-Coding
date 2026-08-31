import type {
  MouseEvent as ReactMouseEvent,
  Ref,
} from "react";
import {
  IconClock,
  IconFolder,
  IconMore,
  IconPanel,
  IconPanelRight,
  IconScheduled,
} from "@/components/icons";
import { Tip } from "@/components/ui/tooltip";
import type { ConnPill } from "@/lib/connStatus";

export interface WorkbenchTopbarProps {
  title: string;
  scheduled?: boolean;
  scheduledLabel: string;
  sessionMenuLabel: string;
  projectContext?: boolean;
  sessionMenuOpen?: boolean;
  onOpenSessionMenu?: (event: ReactMouseEvent<HTMLButtonElement>) => void;
  sidebarCollapsed: boolean;
  showSidebarLabel: string;
  onShowSidebar: () => void;
  sidebarToggleRef?: Ref<HTMLButtonElement>;
  asideCollapsed: boolean;
  showAsideLabel: string;
  hideAsideLabel: string;
  onToggleAside: () => void;
  asideToggleRef?: Ref<HTMLButtonElement>;
  connection?: {
    pill: ConnPill;
    label: string;
  } | null;
  retry?: {
    label: string;
    detail?: string;
  } | null;
  onTitlebarDoubleClick?: () => void;
  automationTitle?: boolean;
}

export function WorkbenchTopbar({
  title,
  scheduled = false,
  scheduledLabel,
  sessionMenuLabel,
  projectContext = false,
  sessionMenuOpen = false,
  onOpenSessionMenu,
  sidebarCollapsed,
  showSidebarLabel,
  onShowSidebar,
  sidebarToggleRef,
  asideCollapsed,
  showAsideLabel,
  hideAsideLabel,
  onToggleAside,
  asideToggleRef,
  connection = null,
  retry = null,
  onTitlebarDoubleClick,
  automationTitle = false,
}: WorkbenchTopbarProps) {
  const asideLabel = asideCollapsed ? showAsideLabel : hideAsideLabel;

  return (
    <div className="main__top" onDoubleClick={onTitlebarDoubleClick}>
      <div className="main__title-row">
        {sidebarCollapsed ? (
          <Tip label={showSidebarLabel}>
            <button
              ref={sidebarToggleRef}
              type="button"
              className="chrome-btn chrome-btn--traffic main__pane-toggle"
              aria-label={showSidebarLabel}
              onClick={onShowSidebar}
            >
              <IconPanel size={16} />
            </button>
          </Tip>
        ) : null}

        {automationTitle ? (
          <span className="main__title-icon">
            <IconScheduled size={16} />
          </span>
        ) : scheduled ? (
          <span
            className="main__title-icon"
            title={scheduledLabel}
            aria-label={scheduledLabel}
          >
            <IconClock size={16} />
          </span>
        ) : projectContext ? (
          <span className="main__title-icon" aria-hidden>
            <IconFolder size={16} />
          </span>
        ) : null}

        <div className="main__title-drag" data-tauri-drag-region>
          {automationTitle ? (
            <h1 className="main__title">{title}</h1>
          ) : (
            <Tip label={title}>
              <h1 className="main__title">{title}</h1>
            </Tip>
          )}
        </div>

        {onOpenSessionMenu ? (
          <Tip label={sessionMenuLabel}>
            <button
              type="button"
              className="chrome-btn main__title-menu"
              aria-label={sessionMenuLabel}
              aria-haspopup="menu"
              aria-expanded={sessionMenuOpen}
              data-open={sessionMenuOpen ? "true" : undefined}
              onClick={onOpenSessionMenu}
            >
              <IconMore size={16} />
            </button>
          </Tip>
        ) : null}
      </div>

      <div className="main__top-actions">
        {connection ? (
          <span
            className={`status-pill status-pill--${connection.pill.tone}`}
            role="status"
            title={connection.label}
          >
            <span className="status-pill__dot" aria-hidden />
            {connection.label}
          </span>
        ) : null}

        {retry ? (
          <Tip label={retry.detail || ""} disabled={!retry.detail}>
            <span className="main__sub main__sub--retry">{retry.label}</span>
          </Tip>
        ) : null}

        <Tip label={asideLabel}>
          <button
            ref={asideToggleRef}
            type="button"
            className={
              "chrome-btn main__pane-toggle" +
              (!asideCollapsed ? " is-on" : "")
            }
            aria-label={asideLabel}
            aria-pressed={!asideCollapsed}
            onClick={onToggleAside}
          >
            <IconPanelRight size={16} />
          </button>
        </Tip>
      </div>
    </div>
  );
}
