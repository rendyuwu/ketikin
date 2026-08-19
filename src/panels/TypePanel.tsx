import { useEffect, useRef } from "react";

import { CloseIcon } from "../components/Banner";
import { NumberInput } from "../components/NumberInput";
import {
  estimateMs,
  formatCount,
  formatDuration,
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
  // The backend's own count, so this agrees with the `state.total` the progress
  // bar is measured against rather than sitting a few characters away from it.
  const characters = typedCharCount(text);
  const estimate = formatDuration(
    estimateMs(text, typingDelayMs, startDelaySecs),
  );
  const percent =
    state.total > 0 ? Math.min(100, (state.typed / state.total) * 100) : 0;

  return (
    <div className="panel type-panel">
      <textarea
        ref={textarea}
        className="textarea"
        value={text}
        spellCheck={false}
        autoComplete="off"
        autoCorrect="off"
        autoCapitalize="off"
        aria-label="Text to type"
        placeholder="Paste the text you want Ketikin to type…"
        onChange={(e) => onTextChange(e.target.value)}
      />

      <p className="meta">
        {formatCount(characters)} character{characters === 1 ? "" : "s"} ·{" "}
        {estimate} at {typingDelayMs} ms
      </p>

      <div className="type-controls">
        <label className="inline-field" htmlFor="type-delay">
          <span>Delay (ms)</span>
          <NumberInput
            id="type-delay"
            value={typingDelayMs}
            min={DELAY_MIN}
            max={DELAY_MAX}
            onCommit={onDelayChange}
          />
        </label>
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
      </div>

      {state.phase === "countdown" ? (
        <p className="countdown" role="status">
          Starting in {state.countdown}…
        </p>
      ) : null}

      {state.phase === "typing" ? (
        <div
          className="progress"
          role="progressbar"
          aria-valuemin={0}
          aria-valuemax={state.total}
          aria-valuenow={state.typed}
          aria-label="Typing progress"
        >
          <div className="progress-fill" style={{ width: `${percent}%` }} />
        </div>
      ) : null}

      {idle ? (
        <button
          type="button"
          className="btn btn--primary btn--block"
          disabled={!canStart}
          onClick={onStart}
        >
          Start typing
        </button>
      ) : (
        <button
          type="button"
          className="btn btn--danger btn--block"
          onClick={onStop}
        >
          Stop
        </button>
      )}

      {idle && !result ? (
        <p className="helper">Click into the target window during the countdown.</p>
      ) : null}

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
            <CloseIcon />
          </button>
        </div>
      ) : null}
    </div>
  );
}
