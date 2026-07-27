import { IconClose, IconImagine } from "@/components/icons";
import { Tip } from "@/components/ui/tooltip";

type Props = {
  label: string;
  disabled?: boolean;
  onDisable: () => void;
};

/**
 * Visible only while plan mode is active. The bulb becomes a close affordance
 * on hover/focus so the control communicates its immediate action.
 */
export function ComposerPlanModeButton({
  label,
  disabled = false,
  onDisable,
}: Props) {
  return (
    <Tip label={label}>
      <button
        type="button"
        className="chip chip--plan-active"
        disabled={disabled}
        aria-label={label}
        onClick={onDisable}
      >
        <span className="chip--plan-active__icon" aria-hidden>
          <IconImagine size={14} className="chip--plan-active__bulb" />
          <IconClose size={13} className="chip--plan-active__close" />
        </span>
        <span className="chip__label">{label}</span>
      </button>
    </Tip>
  );
}
