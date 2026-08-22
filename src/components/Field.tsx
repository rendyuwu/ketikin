import type { ReactNode } from "react";

/**
 * A settings row: label on the left, control against the right edge, optional
 * hint on its own full-width line underneath.
 *
 * Deliberately the same grammar as `Toggle`. This used to stack the label above
 * the control, which meant one Settings section could switch between two row
 * shapes — a stacked Theme select followed immediately by three switch rows —
 * with nothing to explain the switch, and left 300-440px of empty gutter beside
 * every control.
 *
 * `children` is wrapped rather than handed straight to the grid because a
 * control is not always one element: `HotkeyInput` returns a fragment of a
 * button row plus up to two error paragraphs. `.field-control` is what the CSS
 * hangs the "this is the control" rules off; see the note on it in styles.css
 * for why it is `display: contents`.
 *
 * The label gets `<htmlFor>-label` as its own id, so a control that has to
 * build a composite accessible name can point at it. `HotkeyInput` is the one
 * that does — its name is this label plus the accelerator it is displaying —
 * and it derives the same id from the id it was given. Changing the shape of
 * this one means changing that one.
 */
type FieldProps = {
  label: string;
  htmlFor?: string;
  hint?: ReactNode;
  children: ReactNode;
};

export function Field({ label, htmlFor, hint, children }: FieldProps) {
  return (
    <div className="field">
      <label
        className="field-label"
        id={htmlFor ? `${htmlFor}-label` : undefined}
        htmlFor={htmlFor}
      >
        {label}
      </label>
      <div className="field-control">{children}</div>
      {hint ? <p className="field-hint">{hint}</p> : null}
    </div>
  );
}
