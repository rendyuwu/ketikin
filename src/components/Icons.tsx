/**
 * The app's entire icon set, inline.
 *
 * No icon library, deliberately: six glyphs is forty lines of SVG, and shipping
 * zero UI dependencies is one of this project's real assets — a small binary and
 * nothing in the supply chain to audit.
 *
 * Every icon here obeys the same five attributes, which is what keeps a
 * hand-drawn set from looking hand-drawn: a 16 viewBox, a 1.5px `currentColor`
 * stroke with round caps and joins, no fill, and hidden from assistive tech
 * (`aria-hidden` plus `focusable`, the latter for the IE-era attribute Edge
 * still honours on SVG). An icon-only control carries the name instead, on an
 * `aria-label`.
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

/** Shared by all six, so the five attributes are declared exactly once. */
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
