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
 * Re-states the suspend after a settings save.
 *
 * A save does *not* expire the suspend: `hotkeys::apply` has to lift it, since
 * actually registering an accelerator is the only way to learn whether the OS
 * will accept it, but it snapshots the flag on the way in and restores it on
 * both exit paths. So the capture survives a save today — because the backend
 * puts the suspend back, and because `suspend_hotkeys` is a boolean flip rather
 * than a counted release, which is what makes re-asserting an intact suspend a
 * no-op instead of a second lease.
 *
 * Both of those are the backend's choices, and the frontend holds the suspend
 * for as long as a capture field is focused — a window a debounced save lands
 * inside of by design. Re-asserting costs one idempotent IPC call and keeps
 * that claim resting on something this side does, so if the restore is ever
 * dropped the field is still protected rather than silently re-arming the exact
 * hotkey the user is in the middle of replacing.
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
