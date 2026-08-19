import { useCallback, useEffect, useRef, useState } from "react";

import {
  errorMessage,
  onTypingDone,
  onTypingState,
  startTyping,
  stopTyping,
  subscribe,
  typingStatus,
} from "../lib/api";
import { IDLE_TYPING_STATE, type TypingDone, type TypingState } from "../lib/types";

const RESULT_DISMISS_MS = 4000;

export type UseTyping = {
  state: TypingState;
  result: TypingDone | null;
  starting: boolean;
  start: (text: string) => void;
  stop: () => void;
  refresh: () => void;
  dismissResult: () => void;
};

export function useTyping(): UseTyping {
  const [state, setState] = useState<TypingState>(IDLE_TYPING_STATE);
  const [result, setResult] = useState<TypingDone | null>(null);
  const [starting, setStarting] = useState(false);

  const phase = useRef(state.phase);
  const inFlight = useRef(false);
  /**
   * Bumped by every event-driven state change so an in-flight `typing_status`
   * can tell it has been superseded. Without this a status snapshot taken
   * before a run ended can land after `typing://done` and put the UI back into
   * `typing`, stranding it on the Stop button. Same guard, and same reason, as
   * the revision counter in `useSettings`.
   */
  const revision = useRef(0);

  const applyState = useCallback((next: TypingState) => {
    phase.current = next.phase;
    setState(next);
  }, []);

  /** Apply state that came from an event; supersedes any in-flight refresh. */
  const applyEventState = useCallback(
    (next: TypingState) => {
      revision.current += 1;
      applyState(next);
    },
    [applyState],
  );

  const refresh = useCallback(() => {
    const at = revision.current;
    typingStatus()
      .then((next) => {
        // An event landed while this was in flight, so it is newer than we are.
        if (revision.current === at) applyState(next);
      })
      .catch((err: unknown) =>
        setResult({ reason: "error", message: errorMessage(err) }),
      );
  }, [applyState]);

  useEffect(() => {
    const unsubscribe = subscribe([
      onTypingState((next) => {
        applyEventState(next);
        if (next.phase !== "idle") setResult(null);
      }),
      onTypingDone((done) => {
        applyEventState(IDLE_TYPING_STATE);
        setResult(done);
      }),
    ]);

    // The window may have been hidden to tray mid-run; recover the real state.
    refresh();

    return unsubscribe;
  }, [applyEventState, refresh]);

  // Success is transient; an error stays until the user dismisses it.
  useEffect(() => {
    if (!result || result.reason === "error") return;
    const timer = window.setTimeout(() => setResult(null), RESULT_DISMISS_MS);
    return () => clearTimeout(timer);
  }, [result]);

  const start = useCallback((text: string) => {
    // Guards a hotkey press landing on top of a click, and a StrictMode replay.
    if (inFlight.current || phase.current !== "idle" || !text.trim()) return;
    inFlight.current = true;
    setStarting(true);
    setResult(null);

    startTyping(text)
      .catch((err: unknown) =>
        setResult({ reason: "error", message: errorMessage(err) }),
      )
      .finally(() => {
        inFlight.current = false;
        setStarting(false);
      });
  }, []);

  const stop = useCallback(() => {
    stopTyping().catch((err: unknown) =>
      setResult({ reason: "error", message: errorMessage(err) }),
    );
  }, []);

  const dismissResult = useCallback(() => setResult(null), []);

  return { state, result, starting, start, stop, refresh, dismissResult };
}
