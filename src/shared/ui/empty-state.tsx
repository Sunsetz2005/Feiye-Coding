/**
 * Empty / unavailable surface. Title is required so the state is not icon-only.
 *
 * @example
 * <EmptyState
 *   title="No personal plugins yet"
 *   description="Public connectors are listed on the other tab."
 * />
 */
import type { ReactNode } from "react";
import { cn } from "@/lib/utils";

export function EmptyState({
  title,
  description,
  action,
  className,
}: {
  title: string;
  description?: string;
  action?: ReactNode;
  className?: string;
}) {
  return (
    <div className={cn("ui-empty", className)} role="status">
      <h3 className="ui-empty__title">{title}</h3>
      {description ? <p className="ui-empty__desc">{description}</p> : null}
      {action ? <div className="ui-empty__action">{action}</div> : null}
    </div>
  );
}
