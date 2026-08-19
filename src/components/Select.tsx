import type { ReactNode } from "react";

import { ChevronIcon } from "./Icons";

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
      <ChevronIcon size={12} />
    </span>
  );
}
