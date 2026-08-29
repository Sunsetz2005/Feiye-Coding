/**
 * Column-scoped status banner (not a global toast).
 *
 * @example
 * <Banner tone="warning" action={{ label: "Choose folder", onPress: open }}>
 *   Select a project to edit files on this computer.
 * </Banner>
 */
import type { ReactNode } from "react";
import { cn } from "@/lib/utils";
import { Button } from "./button";

export type BannerTone = "info" | "warning" | "danger";

export function Banner({
  tone = "info",
  children,
  action,
  onDismiss,
  dismissLabel,
  className,
}: {
  tone?: BannerTone;
  children: ReactNode;
  action?: { label: string; onPress: () => void };
  onDismiss?: () => void;
  dismissLabel?: string;
  className?: string;
}) {
  return (
    <div
      className={cn("ui-banner conversation-banner", className)}
      role="status"
      data-tone={tone}
    >
      <div className="ui-banner__body">{children}</div>
      {action ? (
        <Button
          type="button"
          variant="ghost"
          size="sm"
          className="ui-banner__action"
          onClick={action.onPress}
        >
          {action.label}
        </Button>
      ) : null}
      {onDismiss ? (
        <Button
          type="button"
          variant="ghost"
          size="sm"
          aria-label={dismissLabel || "Dismiss"}
          onClick={onDismiss}
        >
          ×
        </Button>
      ) : null}
    </div>
  );
}
