/**
 * Compact picker chip (project folder, model, permission).
 *
 * @example
 * <Chip leading={<FolderIcon />} current={selected} onPress={openPicker}>
 *   {projectName}
 * </Chip>
 */
import type { ReactNode } from "react";
import { cn } from "@/lib/utils";

export function Chip({
  leading,
  children,
  current = false,
  disabled = false,
  onPress,
  className,
  ariaLabel,
}: {
  leading?: ReactNode;
  children: ReactNode;
  current?: boolean;
  disabled?: boolean;
  onPress?: () => void;
  className?: string;
  ariaLabel?: string;
}) {
  return (
    <button
      type="button"
      className={cn(
        "ui-chip inline-flex max-w-full items-center gap-1.5 rounded-full border border-[var(--border-subtle)] bg-[var(--bg-elevated)] px-2.5 py-1 text-xs text-[var(--text-primary)] transition-[transform,background-color] duration-100 ease-[var(--ease-press)] hover:bg-[var(--bg-hover)] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--border-focus)] active:scale-[0.98] disabled:opacity-45",
        current && "border-[var(--accent)]",
        className,
      )}
      aria-current={current ? "true" : undefined}
      aria-label={ariaLabel}
      disabled={disabled}
      onClick={onPress}
    >
      {leading}
      <span className="min-w-0 truncate">{children}</span>
    </button>
  );
}
