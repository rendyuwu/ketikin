import { useEffect, useId, useRef } from "react";

type ConfirmDialogProps = {
  title: string;
  message: string;
  confirmLabel: string;
  onConfirm: () => void;
  onCancel: () => void;
  busy?: boolean;
};

export function ConfirmDialog({
  title,
  message,
  confirmLabel,
  onConfirm,
  onCancel,
  busy = false,
}: ConfirmDialogProps) {
  const dialog = useRef<HTMLDivElement>(null);
  const cancelButton = useRef<HTMLButtonElement>(null);
  const titleId = useId();
  const messageId = useId();

  /**
   * Both buttons are `disabled` while a delete is in flight, and the browser
   * blurs a control the moment it is disabled — so focus fell to `<body>`,
   * outside `.overlay`, where neither the Tab trap nor the Escape handler below
   * ever sees a keystroke. Parking focus on the dialog keeps both working; that
   * is what `tabIndex={-1}` on the container is for.
   *
   * Keyed on `busy` rather than run once on mount, which also covers the way
   * back: a delete that fails leaves the dialog up with its buttons live again,
   * and focus belongs on Cancel rather than on the container it was parked on.
   *
   * The parked container draws the app's focus ring, and only on the path where
   * that is right: Chromium carries `:focus-visible` across a programmatic
   * focus from whatever held it, so a delete started with the keyboard rings
   * the dialog and one started with the mouse does not.
   */
  useEffect(() => {
    if (busy) dialog.current?.focus();
    else cancelButton.current?.focus();
  }, [busy]);

  // Escape cancels; Tab stays inside the dialog.
  function onKeyDown(event: React.KeyboardEvent<HTMLDivElement>) {
    if (event.key === "Escape") {
      event.preventDefault();
      onCancel();
      return;
    }
    if (event.key !== "Tab") return;

    const focusable = dialog.current?.querySelectorAll<HTMLButtonElement>(
      "button:not([disabled])",
    );
    // Nothing inside is focusable: both buttons are disabled while `busy`.
    // Without swallowing the key, Tab walks out of the dialog and into the
    // panel behind the scrim — the one place a modal must not let it go.
    if (!focusable || focusable.length === 0) {
      event.preventDefault();
      return;
    }

    const first = focusable[0];
    const last = focusable[focusable.length - 1];
    const active = document.activeElement;

    if (event.shiftKey && active === first) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && active === last) {
      event.preventDefault();
      first.focus();
    }
  }

  return (
    <div className="overlay" onKeyDown={onKeyDown}>
      <div
        className="dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        aria-describedby={messageId}
        ref={dialog}
        tabIndex={-1}
      >
        <h2 className="dialog-title" id={titleId}>
          {title}
        </h2>
        <p className="dialog-message" id={messageId}>
          {message}
        </p>
        <div className="dialog-actions">
          <button
            type="button"
            className="btn"
            onClick={onCancel}
            ref={cancelButton}
            disabled={busy}
          >
            Cancel
          </button>
          <button
            type="button"
            className="btn btn--danger"
            onClick={onConfirm}
            disabled={busy}
          >
            {busy ? "Deleting…" : confirmLabel}
          </button>
        </div>
      </div>
    </div>
  );
}
