/**
 * The app's entire icon set, inline.
 *
 * No icon library, deliberately: nine glyphs is eighty lines of SVG, and shipping
 * zero UI dependencies is one of this project's real assets — a small binary and
 * nothing in the supply chain to audit.
 *
 * Every icon here obeys the same five attributes, which is what keeps a
 * hand-drawn set from looking hand-drawn: a 16 viewBox, a 1.5px `currentColor`
 * stroke with round caps and joins, no fill, and hidden from assistive tech
 * (`aria-hidden` plus `focusable`, the latter for the IE-era attribute Edge
 * still honours on SVG). An icon-only control carries the name instead — on an
 * `aria-label` where the name exists nowhere else, or on a visually-hidden
 * sibling where the same string is also drawn at wider windows (the tabs, whose
 * label is shed to `.visually-hidden` rather than removed so the accessible
 * name cannot change with the window size).
 *
 * `size` is the *rendered* size, not the grid: 14px next to the 13px body step,
 * 12px where the glyph sits inside something already small — a 22px icon button
 * or a text link. Stroke width stays 1.5 across both, so the optical weight
 * tracks the text rather than the container.
 */

type IconProps = {
  size?: number;
};

const DEFAULT_SIZE = 14;

/** Shared by all nine, so the five attributes are declared exactly once. */
function Icon({
  size = DEFAULT_SIZE,
  d,
  className,
}: IconProps & { d: string; className?: string }) {
  return (
    <svg
      className={className}
      viewBox="0 0 16 16"
      width={size}
      height={size}
      aria-hidden="true"
      focusable="false"
    >
      <path
        d={d}
        stroke="currentColor"
        strokeWidth="1.5"
        strokeLinecap="round"
        strokeLinejoin="round"
        fill="none"
      />
    </svg>
  );
}

export function CloseIcon({ size }: IconProps) {
  return <Icon size={size} d="M3.5 3.5l9 9m0-9l-9 9" />;
}

export function ChevronIcon({ size }: IconProps) {
  return <Icon className="select-chevron" size={size} d="M4 6.5l4 4 4-4" />;
}

export function PlusIcon({ size }: IconProps) {
  return <Icon size={size} d="M8 3.5v9M3.5 8h9" />;
}

/** Body as one closed outline, plus the cut across the ferrule. */
export function PencilIcon({ size }: IconProps) {
  return (
    <Icon size={size} d="M2.75 13.25h2.5l7-7-2.5-2.5-7 7v2.5M8.75 4.75l2.5 2.5" />
  );
}

/**
 * Lid, handle, can. The two staves most trash glyphs carry are left out on
 * purpose: at 14px with a 1.5 stroke they close up against the can's walls and
 * the whole thing reads as a filled block.
 */
export function TrashIcon({ size }: IconProps) {
  return (
    <Icon
      size={size}
      d="M2.75 4.25h10.5M6.25 4.25V2.75h3.5v1.5M4.25 4.25l.5 9h6.5l.5-9"
    />
  );
}

/** Box with its top-right corner left open, and the arrow leaving through it. */
export function ExternalIcon({ size }: IconProps) {
  return (
    <Icon
      size={size}
      d="M9 3.25h3.75V7M12.25 3.75L7.5 8.5M12.25 9.5v2.75h-8.5v-8.5H6.5"
    />
  );
}

/* The three below stand in for the tab labels once the window is too narrow to
   draw them. They are drawn as strokes on the same 16 grid rather than as
   filled marks for the reason the whole set is: three glyphs that have to be
   told apart at 14px, in a row, with nothing else to disambiguate them, so what
   matters is the silhouette and not the detail. The candidates that lost are
   recorded in #50 — a keycap (closes into a block at this size, the same
   failure `src-tauri/icons/icon.svg` documents in its own header), a gear
   (reads as a sun), horizontal sliders (collide with the Templates list), and
   `>_` (says "command line", which this is not). */

/** A serif T: crossbar, stem, foot. The compose surface, in one letter. */
export function TypeIcon({ size }: IconProps) {
  return <Icon size={size} d="M3.5 4.25V2.75h9v1.5M8 2.75v10.5M6 13.25h4" />;
}

/** A bulleted list, last line short so it reads as a list and not as a grid. */
export function TemplatesIcon({ size }: IconProps) {
  return (
    <Icon
      size={size}
      d="M2.75 4.5h.01M6 4.5h7M2.75 8h.01M6 8h7M2.75 11.5h.01M6 11.5h4"
    />
  );
}

/**
 * Two vertical faders, each broken at mid-travel by its own handle. The break
 * is what the handle sits in, so at 14px the two read as one shape rather than
 * as a bar with a dot on it; below that the break closes up and the handles go
 * with it, which is why these three are only ever drawn at the default size.
 */
export function SettingsIcon({ size }: IconProps) {
  return (
    <Icon
      size={size}
      d="M4 13.25V9.5M4 6.5V2.75M12 13.25v-3.75M12 6.5V2.75M2.5 8h3M10.5 8h3"
    />
  );
}
