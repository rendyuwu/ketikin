import { useCallback, useEffect, useRef, useState } from "react";

type NumberInputProps = {
  id?: string;
  value: number;
  min: number;
  max: number;
  onCommit: (value: number) => void;
  ariaLabel?: string;
  className?: string;
};

const COMMIT_DEBOUNCE_MS = 350;

const clamp = (n: number, min: number, max: number) =>
  Math.min(max, Math.max(min, n));

/**
 * A number field that tolerates an empty or half-typed value instead of
 * snapping or emitting NaN. Commits on a short debounce and again on blur,
 * where the value is clamped. Re-syncs from the prop only while unfocused, so
 * the backend's clamped response never yanks the caret mid-edit.
 */
export function NumberInput({
  id,
  value,
  min,
  max,
  onCommit,
  ariaLabel,
  className,
}: NumberInputProps) {
  const [raw, setRaw] = useState(String(value));
  const [focused, setFocused] = useState(false);
  const timer = useRef<number | null>(null);

  useEffect(() => {
    if (!focused) setRaw(String(value));
  }, [value, focused]);

  useEffect(
    () => () => {
      if (timer.current !== null) clearTimeout(timer.current);
    },
    [],
  );

  const commit = useCallback(
    (text: string) => {
      const parsed = Number.parseInt(text, 10);
      if (Number.isNaN(parsed)) return null;
      const next = clamp(parsed, min, max);
      onCommit(next);
      return next;
    },
    [min, max, onCommit],
  );

  function onChange(next: string) {
    setRaw(next);
    if (timer.current !== null) clearTimeout(timer.current);

    // Wait for a value that's already in range; clamping mid-keystroke would
    // turn "10" into "10" but "1" into the minimum on the way to "100".
    const parsed = Number.parseInt(next, 10);
    if (Number.isNaN(parsed) || parsed < min || parsed > max) return;
    timer.current = window.setTimeout(() => onCommit(parsed), COMMIT_DEBOUNCE_MS);
  }

  function onBlur() {
    if (timer.current !== null) clearTimeout(timer.current);
    setFocused(false);
    const committed = commit(raw);
    setRaw(String(committed ?? value));
  }

  return (
    <input
      id={id}
      type="number"
      className={className ? `input input--number ${className}` : "input input--number"}
      inputMode="numeric"
      min={min}
      max={max}
      value={raw}
      aria-label={ariaLabel}
      onFocus={() => setFocused(true)}
      onChange={(e) => onChange(e.target.value)}
      onBlur={onBlur}
      onKeyDown={(e) => {
        if (e.key === "Enter") e.currentTarget.blur();
      }}
    />
  );
}
