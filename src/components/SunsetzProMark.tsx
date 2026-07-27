import { memo } from "react";
import { SunsetzLogo } from "@/components/SunsetzLogo";

export type SunsetzProBrandKind = "sunsetz-pro" | "heavy";

/** Product-owned subscription mark; upstream membership values are mapped in accountUi. */
export const SunsetzProMark = memo(function SunsetzProMark({
  kind,
  className = "",
  title,
}: {
  kind: SunsetzProBrandKind;
  className?: string;
  title?: string;
}) {
  const label = kind === "heavy" ? "Sunsetz Pro Heavy" : "Sunsetz Pro";
  return (
    <span
      className={`sunsetz-pro-mark sunsetz-pro-mark--${kind} ${className}`.trim()}
      role="img"
      aria-label={title ?? label}
      title={title ?? label}
    >
      <SunsetzLogo size={30} />
      <span className="sunsetz-pro-mark__word">Sunsetz Pro</span>
      {kind === "heavy" ? <span className="sunsetz-pro-mark__badge">Heavy</span> : null}
    </span>
  );
});
