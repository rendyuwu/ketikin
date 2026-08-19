import { useCallback, useEffect, useId, useRef, useState } from "react";

type NumberInputProps = {
  id?: string;
  value: number;
  min: number;
  max: number;
  onCommit: (value: number) => void;
  ariaLabel?: string;
  className?: string;
  /** Unit drawn inside the field's right edge, e.g. `ms`. */
  suffix?: string;
  /** How that unit is spoken, when the symbol reads badly aloud. */
  suffixLabel?: string;
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
  suffix,
  suffixLabel,
}: NumberInputProps) {
  const [raw, setRaw] = useState(String(value));
  const [focused, setFocused] = useState(false);
  const timer = useRef<number | null>(null);
  const unitId = useId();

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

  const classes = ["input", "input--number"];
  if (suffix) classes.push("input--suffixed");
  if (className) classes.push(className);

  const field = (
    <input
      id={id}
      type="number"
      className={classes.join(" ")}
      inputMode="numeric"
      min={min}
      max={max}
      value={raw}
      aria-label={ariaLabel}
      // The unit is drawn, not part of the label, so it is described rather
      // than named — a name would have to repeat the visible label to carry it.
      aria-describedby={suffix ? unitId : undefined}
      onFocus={() => setFocused(true)}
      onChange={(e) => onChange(e.target.value)}
      onBlur={onBlur}
      onKeyDown={(e) => {
        if (e.key === "Enter") e.currentTarget.blur();
      }}
    />
  );

  if (!suffix) return field;

  return (
    <span className="input-affix">
      {field}
      <span className="input-suffix" aria-hidden="true">
        {suffix}
      </span>
      <span className="visually-hidden" id={unitId}>
        {suffixLabel ?? suffix}
      </span>
    </span>
  );
}
