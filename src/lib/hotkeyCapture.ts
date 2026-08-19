import { errorMessage, resumeHotkeys, suspendHotkeys } from "./api";

/*
 * A registered global hotkey is grabbed by the OS, so its keydown never reaches
 * the WebView. Without suspending, a user who clicks a capture field and presses
 * their existing shortcut out of muscle memory doesn't re-record it — they fire
 * it, and a real typing run starts into the settings panel they're editing.
 *
 * The count is module-scope because both fields share one global registration,
 * and the resume is deferred a tick so tabbing from one field to the other
 * (blur then focus) doesn't re-arm the hotkeys in between.
 */
let captureCount = 0;
let pendingResume: number | null = null;
let reportError: ((message: string) => void) | null = null;

/*
 * The backend also auto-resumes on window blur. A webview doesn't reliably fire
 * a DOM blur on the focused element when the *window* loses focus, so the field
 * can still be focused and prompting for a chord while the hotkeys have been
 * re-armed underneath it. Re-assert whenever the window comes back.
 */
const onWindowFocus = () => reassertCapture();

export function acquireCapture(onError: (message: string) => void): void {
  reportError = onError;
  if (pendingResume !== null) {
    clearTimeout(pendingResume);
    pendingResume = null;
  }
  captureCount += 1;
  if (captureCount > 1) return;
  window.addEventListener("focus", onWindowFocus);
  suspendHotkeys().catch((err: unknown) => onError(errorMessage(err)));
}

export function releaseCapture(onError: (message: string) => void): void {
  captureCount = Math.max(0, captureCount - 1);
  if (captureCount > 0) return;
  reportError = null;
  if (pendingResume !== null) clearTimeout(pendingResume);
  pendingResume = window.setTimeout(() => {
    pendingResume = null;
    if (captureCount === 0) {
      window.removeEventListener("focus", onWindowFocus);
      resumeHotkeys().catch((err: unknown) => onError(errorMessage(err)));
    }
  }, 0);
}

/**
 * `save_settings` expires any outstanding suspend on the backend. A debounced
 * save landing while a capture field is still focused would therefore re-arm
 * the exact hotkey the user is in the middle of replacing — the original bug,
 * reintroduced a few hundred milliseconds later. Re-assert after every save.
 *
 * Safe to call unconditionally; it's a no-op when nothing is capturing.
 */
export function reassertCapture(): void {
  if (captureCount === 0) return;
  const onError = reportError;
  suspendHotkeys().catch((err: unknown) => {
    if (onError) onError(errorMessage(err));
  });
}
