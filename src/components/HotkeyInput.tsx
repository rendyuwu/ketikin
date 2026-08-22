import { useCallback, useEffect, useId, useRef, useState } from "react";

import { acceleratorFromEvent, formatAccelerator, hasModifier } from "../lib/format";
import { acquireCapture, releaseCapture } from "../lib/hotkeyCapture";

type HotkeyInputProps = {
  /** Also what the enclosing `Field` points its label at, as `<id>-label`. */
  id: string;
  value: string;
  defaultValue: string;
  onCapture: (accelerator: string) => void;
  onReset: () => void;
  disabled?: boolean;
  /** Rejection from `validate_hotkey`, or a `hotkey://error` registration failure. */
  error?: string | null;
};

const NEEDS_MODIFIER = "Hold Ctrl, Alt, Shift or Super together with the key.";

const CAPTURE_HINT = "Press a key combination to change this shortcut.";

export function HotkeyInput({
  id,
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
  const valueId = useId();
  const hintId = useId();

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
        {/* The name is the field's label plus the accelerator on the button,
            named by reference rather than written out. It used to be an
            `aria-label` of "Start typing hotkey", and an accessible name from
            an attribute replaces the element's content — so the one thing a
            reader needs from this control, the shortcut it is currently bound
            to, was the one thing it never said.
            Content alone does not fix it. The enclosing `Field`'s `<label for>`
            names a `<button>` too, and a label wins over the subtree: dropping
            the attribute leaves "Start typing" and still no accelerator.
            Measured in Chromium, all four states: attribute "Start typing
            hotkey", content "Start typing", this "Start typing Alt+K".
            Pointing at `.hotkey-value` also survives capture, when that span is
            at `visibility: hidden` to hold its grid cell open. A hidden node
            referenced directly by `aria-labelledby` still contributes its text,
            which is what keeps the name from changing to the prompt and back
            every time the button takes focus. */}
        <button
          type="button"
          id={id}
          className="hotkey"
          aria-labelledby={`${id}-label ${valueId}`}
          // Focus on this control is capture, so this is announced at exactly
          // the moment it becomes true — which is why the prompt does not need
          // to be live, and must not be part of the name.
          aria-describedby={hintId}
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
            <span
              className="hotkey-value"
              id={valueId}
              data-off={capturing || undefined}
            >
              {formatAccelerator(value)}
            </span>
            {/* Drawn, not spoken: it is the visible half of the description
                below, and the same instruction twice is worse than once. */}
            <span
              className="hotkey-prompt"
              data-off={!capturing || undefined}
              aria-hidden="true"
            >
              Press a key combination…
            </span>
          </span>
        </button>
        <span className="visually-hidden" id={hintId}>
          {CAPTURE_HINT}
        </span>
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
