/**
 * Labeled search field. Pass `label` (visible or visually hidden).
 *
 * @example
 * <SearchField label="Search plugins" value={q} onChange={setQ} />
 */
import { cn } from "@/lib/utils";

export function SearchField({
  label,
  value,
  onChange,
  placeholder,
  visuallyHideLabel = false,
  className,
}: {
  label: string;
  value: string;
  onChange: (value: string) => void;
  placeholder?: string;
  visuallyHideLabel?: boolean;
  className?: string;
}) {
  const id = `search-${label.replace(/\s+/g, "-").toLowerCase()}`;
  return (
    <div className={cn("ui-search", className)}>
      <label
        htmlFor={id}
        className={visuallyHideLabel ? "sr-only" : "ui-search__label"}
      >
        {label}
      </label>
      <input
        id={id}
        type="search"
        className="ui-search__input"
        value={value}
        placeholder={placeholder ?? label}
        onChange={(event) => onChange(event.target.value)}
        autoComplete="off"
        spellCheck={false}
      />
    </div>
  );
}
