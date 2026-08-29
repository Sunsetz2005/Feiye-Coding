/**
 * Loading placeholder. Put `aria-busy` on the parent region.
 *
 * @example
 * <div aria-busy="true" aria-label="Loading plugins">
 *   <Skeleton lines={3} />
 * </div>
 */
import { cn } from "@/lib/utils";

export function Skeleton({
  lines = 3,
  media = false,
  className,
}: {
  lines?: number;
  media?: boolean;
  className?: string;
}) {
  return (
    <div className={cn("ui-skeleton", className)} aria-hidden>
      {media ? <div className="ui-skeleton__media" /> : null}
      {Array.from({ length: Math.max(1, lines) }, (_, i) => (
        <div
          key={i}
          className="ui-skeleton__line"
          style={{ width: `${88 - (i % 3) * 14}%` }}
        />
      ))}
    </div>
  );
}
