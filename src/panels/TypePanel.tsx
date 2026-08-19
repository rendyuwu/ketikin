import { useEffect, useRef } from "react";

import { CloseIcon } from "../components/Icons";
import { NumberInput } from "../components/NumberInput";
import {
  CADENCE_LABELS,
  CADENCE_STOPS,
  cadenceIndex,
  cadenceName,
} from "../lib/cadence";
import {
  estimateMs,
  formatCount,
  formatDuration,
  formatDurationCompact,
  typedCharCount,
} from "../lib/format";
import {
  DELAY_MAX,
  DELAY_MIN,
  type TypingDone,
  type TypingState,
} from "../lib/types";

type TypePanelProps = {
  text: string;
  onTextChange: (text: string) => void;
  typingDelayMs: number;
  startDelaySecs: number;
  onDelayChange: (ms: number) => void;
  state: TypingState;
  result: TypingDone | null;
  starting: boolean;
  /**
   * Shortcuts drawn inside the buttons they fire, already formatted for this
   * platform. `null` when there is nothing honest to show — hotkeys switched
   * off, or a bind the OS refused — because a hint for a shortcut that does
   * nothing is worse than no hint. App owns that rule; the panel only renders.
   */
  startAccelerator: string | null;
  stopAccelerator: string | null;
  onStart: () => void;
  onStop: () => void;
  onDismissResult: () => void;
};

const RESULT_TEXT: Record<TypingDone["reason"], string> = {
  completed: "Finished typing.",
  stopped: "Stopped.",
  error: "Typing failed.",
};

export function TypePanel({
  text,
  onTextChange,
  typingDelayMs,
  startDelaySecs,
  onDelayChange,
  state,
  result,
  starting,
  startAccelerator,
  stopAccelerator,
  onStart,
  onStop,
  onDismissResult,
}: TypePanelProps) {
  const textarea = useRef<HTMLTextAreaElement>(null);

  useEffect(() => {
    textarea.current?.focus();
  }, []);

  const idle = state.phase === "idle";
  const canStart = text.trim().length > 0 && !starting;
  // The backend's own count, so this agrees with the `state.total` the edge rail
  // is measured against rather than sitting a few characters away from it.
  const characters = typedCharCount(text);
  const runMs = estimateMs(text, typingDelayMs, startDelaySecs);
  const cadence = cadenceName(typingDelayMs);
  const counting = state.phase === "countdown";

  return (
    <div className="panel type-panel">
      <div className={counting ? "compose compose--taken" : "compose"}>
        <textarea
          ref={textarea}
          className="compose-input"
          value={text}
          spellCheck={false}
          autoComplete="off"
          autoCorrect="off"
          autoCapitalize="off"
          aria-label="Text to type"
          placeholder="Paste what Ketikin should type."
          onChange={(e) => onTextChange(e.target.value)}
        />

        {counting ? (
          <div className="takeover">
            <div className="takeover-band">
              {/* Remounted on each tick by the key, which is what replays the
                  slide. aria-hidden because the header's live region already
                  speaks the count, and two regions counting down over each
                  other is worse than one. */}
              <span
                key={state.countdown}
                className="takeover-digit"
                aria-hidden="true"
              >
                {state.countdown}
              </span>

              {/* The footnote that used to stand permanently under the button.
                  It is only actionable while the countdown is running, which is
                  now the only time it is on screen — and role="status"
                  announces it once, at the moment that becomes true. */}
              <p className="takeover-note" role="status">
                Click into the target window.
              </p>
            </div>
          </div>
        ) : null}
      </div>

      <div className="type-footer">
        <p className="type-readout">
          {/* The visible pair is hidden from the accessibility tree and spoken
              as one sentence below: "~" and "—" are typography, and read aloud
              they are noise or silence. */}
          <span className="type-count" aria-hidden="true">
            {formatCount(characters)} character{characters === 1 ? "" : "s"}
          </span>

          {characters > 0 ? (
            <button
              type="button"
              className="btn btn--quiet btn--small"
              onClick={() => {
                onTextChange("");
                textarea.current?.focus();
              }}
            >
              Clear
            </button>
          ) : null}

          {/* An estimate for an empty box would be the countdown on its own,
              which is true and useless — and it is now the largest number on
              the screen, so it must not imply there is something to type. */}
          <span className="type-duration" aria-hidden="true">
            {characters > 0 ? `~ ${formatDurationCompact(runMs)}` : "—"}
          </span>

          <span className="visually-hidden">
            {characters > 0
              ? `${formatCount(characters)} character${
                  characters === 1 ? "" : "s"
                }, ${formatDuration(runMs)} to type.`
              : "Nothing to type yet."}
          </span>
        </p>

        <div className="cadence">
          <label className="cadence-label" htmlFor="type-cadence">
            Cadence
          </label>

          {/* Straight through to `settings.update`, which is debounced 400 ms
              and resets that timer on every call — nine discrete stops means at
              most nine calls across an entire drag, and only the last one
              saves. So no commit-on-release is needed here, and adding local
              drag state would only put a second copy of the value beside the
              optimistic one `useSettings` already holds. A continuous range
              would not have that luxury. */}
          <input
            id="type-cadence"
            type="range"
            className="cadence-slider"
            min={0}
            max={CADENCE_STOPS.length - 1}
            step={1}
            value={cadenceIndex(typingDelayMs)}
            // The slider's own value is a stop index, which means nothing aloud.
            aria-valuetext={
              cadence ? `${typingDelayMs} ms, ${cadence}` : `${typingDelayMs} ms`
            }
            onChange={(e) => onDelayChange(CADENCE_STOPS[Number(e.target.value)])}
          />

          {/* Kept, and kept editable: the slider is for people who do not know
              what 25 ms feels like, and this is for people who know exactly
              what they want. It is also the only way to reach a value off the
              ladder. */}
          <NumberInput
            id="type-delay"
            ariaLabel="Delay between characters"
            value={typingDelayMs}
            min={DELAY_MIN}
            max={DELAY_MAX}
            suffix="ms"
            suffixLabel="milliseconds"
            onCommit={onDelayChange}
          />

          {/* Decoration for the eye: `aria-valuetext` above already speaks the
              name of the stop the thumb is on. */}
          <p className="cadence-stops" aria-hidden="true">
            {CADENCE_LABELS.map((label) => (
              <span key={label}>{label}</span>
            ))}
          </p>
        </div>

        {idle ? (
          <button
            type="button"
            className="btn btn--primary btn--block btn--action"
            disabled={!canStart}
            onClick={onStart}
          >
            Start typing
            {startAccelerator ? (
              <span className="btn-accel">{startAccelerator}</span>
            ) : null}
          </button>
        ) : (
          <button
            type="button"
            className="btn btn--danger btn--block btn--action"
            onClick={onStop}
          >
            Stop
            {stopAccelerator ? (
              <span className="btn-accel">{stopAccelerator}</span>
            ) : null}
          </button>
        )}
      </div>

      {result ? (
        <div
          className={`result result--${result.reason === "error" ? "error" : "ok"}`}
          role={result.reason === "error" ? "alert" : "status"}
        >
          <span>{result.message ?? RESULT_TEXT[result.reason]}</span>
          <button
            type="button"
            className="icon-button"
            aria-label="Dismiss result"
            onClick={onDismissResult}
          >
            <CloseIcon size={12} />
          </button>
        </div>
      ) : null}
    </div>
  );
}
