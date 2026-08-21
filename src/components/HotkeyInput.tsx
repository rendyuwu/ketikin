import { useCallback, useEffect, useRef, useState } from "react";

import { acceleratorFromEvent, formatAccelerator, hasModifier } from "../lib/format";
import { acquireCapture, releaseCapture } from "../lib/hotkeyCapture";

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
  const [captureError, setCaptureError] = useState<string | null>(null);
  const held = useRef(false);

  const release = useCallback(() => {
    if (!held.current) return;
    held.current = false;
    releaseCapture((message) =>
      setCaptureError(`Global hotkeys could not be re-enabled. ${message}`),
    );
  }, []);

  const acquire = useCallback(() => {
    if (held.current) return;
    held.current = true;
    setCaptureError(null);
    acquireCapture((message) =>
      setCaptureError(
        `Global hotkeys could not be paused, so pressing your current shortcut here may start a typing run. ${message}`,
      ),
    );
  }, []);

  // Unmount is the path blur can't cover — switching tabs mid-capture would
  // otherwise leave the hotkeys suspended for the rest of the session.
  useEffect(() => release, [release]);

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
          onFocus={() => {
            setCapturing(true);
            acquire();
          }}
          onBlur={() => {
            setCapturing(false);
            setLocalError(null);
            release();
          }}
        >
          {/* Both states are always in the DOM, one of them hidden, so the
              button is as wide as the wider of the two and focusing it cannot
              change its width — see `.hotkey-swap`. Focus on this control is
              capture, so that swap happens on every focus. */}
          <span className="hotkey-swap">
            <span className="hotkey-value" data-off={capturing || undefined}>
              {formatAccelerator(value)}
            </span>
            <span className="hotkey-prompt" data-off={!capturing || undefined}>
              Press a key combination…
            </span>
          </span>
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
      {captureError ? (
        <p className="field-error" role="alert">
          {captureError}
        </p>
      ) : null}
      {shown ? (
        <p className="field-error" role="alert">
          {shown}
        </p>
      ) : null}
    </>
  );
}
