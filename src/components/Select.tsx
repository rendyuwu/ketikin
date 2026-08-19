import type { ReactNode } from "react";

type SelectProps = {
  id?: string;
  value: string;
  onChange: (value: string) => void;
  children: ReactNode;
  ariaLabel?: string;
};

/**
 * A native `<select>` with only its closed state restyled, so it matches
 * `.input` instead of arriving with the platform's own chrome and font metrics.
 * The popup is still the OS one — a JS listbox would mean owning keyboard
 * handling, screen-reader semantics and positioning, and this app ships no
 * component library to borrow them from.
 *
 * `onChange` hands back the raw string; callers cast it to their own union,
 * which is what the backend validates anyway.
 */
export function Select({ id, value, onChange, children, ariaLabel }: SelectProps) {
  return (
    <span className="select-wrap">
      <select
        id={id}
        className="input select"
        value={value}
        aria-label={ariaLabel}
        onChange={(e) => onChange(e.target.value)}
      >
        {children}
      </select>
      <ChevronIcon />
    </span>
  );
}

/**
 * Drawn to the same spec as `CloseIcon`: 16 grid, 1.5px stroke, `currentColor`,
 * hidden from assistive tech. Anything else added to this app's icon set
 * follows these five attributes.
 */
export function ChevronIcon() {
  return (
    <svg
      className="select-chevron"
      viewBox="0 0 16 16"
      width="12"
      height="12"
      aria-hidden="true"
      focusable="false"
    >
      <path
        d="M4 6.5l4 4 4-4"
        stroke="currentColor"
        strokeWidth="1.5"
        strokeLinecap="round"
        strokeLinejoin="round"
        fill="none"
      />
    </svg>
  );
}
