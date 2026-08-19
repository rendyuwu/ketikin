/**
 * The cadence slider's stops, in milliseconds per character, slowest first.
 *
 * `Delay (ms) [25]` asked the user to translate milliseconds into risk, and
 * nobody can do that until they have ruined one paste into a production
 * console. The slider answers the question they actually have — *how careful is
 * this?* — with three named stops at the ends and the middle, so the row can be
 * read without moving the thumb.
 *
 * Discrete stops rather than a continuous range over `DELAY_MIN..DELAY_MAX`:
 * 25 ms is 2.4% along that range, so every value anyone would choose would live
 * in the leftmost sliver of the track and the rest of it would be dead. This
 * ladder descends by roughly a third per step, which keeps a drag feeling
 * proportional, and it contains the default (25) exactly so a fresh install is
 * not already sitting between two stops.
 *
 * The numeric field beside the slider remains the way to reach anything else,
 * including everything outside 5..80 ms. A value from out there pins the thumb
 * at whichever end is nearest — what a slider does with an out-of-range value —
 * and the field, not the thumb, is what states the truth. The backend's
 * `DELAY_MIN`/`DELAY_MAX` clamping stays the authority either way.
 */
export const CADENCE_STOPS: readonly number[] = [
  80, 60, 45, 35, 25, 18, 12, 8, 5,
];

/**
 * The three stops that carry a name, keyed by their millisecond value.
 *
 * These have to stay pinned to the first, middle and last entries of
 * `CADENCE_STOPS`, because `.cadence-stops` draws the labels at 0%, 50% and
 * 100% of the track. Move a stop and the label stops pointing at it.
 */
const NAMED_STOPS: Readonly<Record<number, string>> = {
  80: "Careful",
  25: "Normal",
  5: "Fast",
};

/** Left to right under the track, matching `CADENCE_STOPS` order. */
export const CADENCE_LABELS: readonly string[] = ["Careful", "Normal", "Fast"];

/** The name of this delay, when it is one of the three named stops. */
export const cadenceName = (ms: number): string | null => NAMED_STOPS[ms] ?? null;

/**
 * The stop nearest this delay. Nearest rather than exact because the numeric
 * field can hold any value in the clamp range, and the slider still has to
 * render somewhere sensible when it does.
 */
export function cadenceIndex(ms: number): number {
  let best = 0;
  let bestGap = Number.POSITIVE_INFINITY;
  for (let i = 0; i < CADENCE_STOPS.length; i += 1) {
    const gap = Math.abs(CADENCE_STOPS[i] - ms);
    if (gap < bestGap) {
      bestGap = gap;
      best = i;
    }
  }
  return best;
}
