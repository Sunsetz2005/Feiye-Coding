/**
 * Icon-only control. `aria-label` is required.
 *
 * @example
 * <IconButton aria-label="Plugins" aria-pressed={open} onClick={toggle}>
 *   <IconPuzzle />
 * </IconButton>
 */
import * as React from "react";
import { cn } from "@/lib/utils";

export interface IconButtonProps
  extends React.ButtonHTMLAttributes<HTMLButtonElement> {
  "aria-label": string;
}

export const IconButton = React.forwardRef<HTMLButtonElement, IconButtonProps>(
  ({ className, type = "button", ...props }, ref) => (
    <button
      ref={ref}
      type={type}
      className={cn(
        "ui-icon-btn inline-flex h-8 w-8 items-center justify-center rounded-md text-[var(--text-secondary)] transition-[transform,background-color] duration-100 ease-[var(--ease-press)] hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--border-focus)] active:scale-[0.98] disabled:pointer-events-none disabled:opacity-45",
        className,
      )}
      {...props}
    />
  ),
);
IconButton.displayName = "IconButton";
