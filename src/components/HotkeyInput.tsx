import { useState } from "react";

import { acceleratorFromEvent, formatAccelerator, hasModifier } from "../lib/format";

type HotkeyInputProps = {
  id: string;
  label: string;
  value: string;
  defaultValue: string;
  onCapture: (accelerator: string) => void;
  onReset: () => void;
  disabled?: boolean;
  /** Rejection from `validate_hotkey`, or a `hotkey://error` registration failure. */
  error?: string | null;
};

const NEEDS_MODIFIER = "Hold Ctrl, Alt, Shift or Super together with the key.";

export function HotkeyInput({
  id,
  label,
  value,
  defaultValue,
  onCapture,
  onReset,
  disabled = false,
  error,
}: HotkeyInputProps) {
  const [capturing, setCapturing] = useState(false);
  const [localError, setLocalError] = useState<string | null>(null);

  function onKeyDown(event: React.KeyboardEvent<HTMLButtonElement>) {
    // Tab must still move focus, and Escape should let the user back out.
    if (event.key === "Tab") return;
    if (event.key === "Escape") {
      event.currentTarget.blur();
      return;
    }

    event.preventDefault();
    const accelerator = acceleratorFromEvent(event.nativeEvent);
    if (accelerator === null) return;

    if (!hasModifier(accelerator)) {
      setLocalError(NEEDS_MODIFIER);
      return;
    }

    setLocalError(null);
    onCapture(accelerator);
  }

  const shown = localError ?? error ?? null;

  return (
    <>
      <div className="hotkey-row">
        <button
          type="button"
          id={id}
          className="hotkey"
          aria-label={label}
          aria-invalid={shown ? true : undefined}
          disabled={disabled}
          onKeyDown={onKeyDown}
          onFocus={() => setCapturing(true)}
          onBlur={() => {
            setCapturing(false);
            setLocalError(null);
          }}
        >
          {capturing ? (
            <span className="hotkey-prompt">Press a key combination…</span>
          ) : (
            <span className="hotkey-value">{formatAccelerator(value)}</span>
          )}
        </button>
        {value !== defaultValue ? (
          <button
            type="button"
            className="btn btn--quiet btn--small"
            disabled={disabled}
            onClick={() => {
              setLocalError(null);
              onReset();
            }}
          >
            Reset
          </button>
        ) : null}
      </div>
      {shown ? (
        <p className="field-error" role="alert">
          {shown}
        </p>
      ) : null}
    </>
  );
}
