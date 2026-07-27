/** Sunsetz sunset mark. The stroke inherits the current theme accent. */
export function SunsetzLogo({ size = 22 }: { size?: number }) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 64 64"
      fill="none"
      xmlns="http://www.w3.org/2000/svg"
      className="sunsetz-logo"
      role="img"
      aria-label="Sunsetz"
    >
      <path
        d="M20 31a12 12 0 0 1 24 0"
        stroke="currentColor"
        strokeWidth="5"
        strokeLinecap="round"
      />
      <path d="M12 37h40" stroke="currentColor" strokeWidth="5" strokeLinecap="round" />
      <path
        d="M22 48h20"
        stroke="currentColor"
        strokeOpacity=".58"
        strokeWidth="5"
        strokeLinecap="round"
      />
    </svg>
  );
}
