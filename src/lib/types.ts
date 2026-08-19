/**
 * The wire contract with the Rust backend. JSON is camelCase on both sides.
 * Nothing here is inferred — if the backend changes, this file changes first.
 */

export type Template = {
  id: string;
  name: string;
  content: string;
  createdAt: string;
  updatedAt: string;
};

export type Theme = "system" | "dark" | "light";

export type NewlineMode = "enter" | "shiftEnter" | "skip";

export type Settings = {
  typingDelayMs: number;
  startDelaySecs: number;
  theme: Theme;
  minimizeToTray: boolean;
  closeToTray: boolean;
  alwaysOnTop: boolean;
  hotkeysEnabled: boolean;
  startHotkey: string;
  stopHotkey: string;
  newlineMode: NewlineMode;
  autoCheckUpdates: boolean;
};

export type StorageSource =
  | "appData"
  | "appDataEnv"
  | "localAppDataEnv"
  | "nextToExe"
  | "temp"
  | "memory";

export type StorageInfo = {
  path: string;
  source: StorageSource;
  writable: boolean;
  error: string | null;
  /**
   * Recoverable-but-important conditions: a corrupt templates file that was
   * reset, or data landing somewhere other users of the machine can write.
   * Full sentences, rendered verbatim. Empty when there's nothing to say.
   */
  notices: string[];
  /**
   * The backend's own verdict, and the same value it gates `storage://warning`
   * on. Gate the banner on this rather than re-deriving it: a deliberate
   * portable install (`nextToExe`) carries notices but is *not* degraded, and
   * that policy belongs to one owner.
   */
  degraded: boolean;
};

/** Result of `tray_status()` — the pollable form of `tray://unavailable`. */
export type TrayStatus = { available: boolean; message: string | null };

export type TypingPhase = "idle" | "countdown" | "typing";

export type TypingState = {
  phase: TypingPhase;
  typed: number;
  total: number;
  countdown: number;
};

export type UpdateInfo = {
  version: string;
  currentVersion: string;
  notes: string | null;
  date: string | null;
  /**
   * Whether Ketikin can replace its own binary. Always true on Windows and
   * macOS; on Linux only under the AppImage, since the updater plugin needs
   * the APPIMAGE env var. False for .deb / .rpm installs, where the update is
   * real but the package manager owns it.
   */
  canInstall: boolean;
};

/** Payload of `typing://done`. */
export type TypingDone = {
  reason: "completed" | "stopped" | "error";
  message: string | null;
};

/** Payload of `hotkey://error` — saved, but the OS refused to register it. */
export type HotkeyError = {
  which: "start" | "stop";
  accelerator: string;
  message: string;
};

/**
 * Result of `hotkey_status()` — the pollable form of `hotkey://error`.
 *
 * Needed because the startup registration happens in the backend's `setup`
 * hook, so its `hotkey://error` is emitted before any listener exists to hear
 * it. Without a poll, a shortcut another application already owns is rendered
 * in Settings as though it were bound and simply does nothing when pressed.
 *
 * At most one entry per slot, cleared once that slot rebinds successfully — the
 * backend collapses to the latest, so this never accumulates and a poll cannot
 * hand back an error the user has already fixed.
 *
 * `failures` also carries release failures, where a shortcut Ketikin no longer
 * wants may still be grabbed until it restarts. Nothing distinguishes them in
 * the shape: the `message` says so, and it is rendered verbatim. Note that on
 * one of those the `accelerator` is the *old* value, not the one now shown in
 * the field it renders under — which is why only `message` is displayed.
 */
export type HotkeyStatus = { failures: HotkeyError[] };

/**
 * Payload of `tray://unavailable` — the tray icon could not be constructed, so
 * the backend ignores `minimizeToTray` / `closeToTray` for this run. The stored
 * preference is left alone, so it returns on a system with a working tray.
 */
export type TrayUnavailable = { message: string };

/**
 * Mirrors the backend defaults. Used as the pre-load render value and as the
 * target of "Reset to default" on the hotkey fields; the backend remains the
 * authority and its normalized response always wins.
 */
export const DEFAULT_SETTINGS: Settings = {
  typingDelayMs: 25,
  startDelaySecs: 3,
  theme: "system",
  minimizeToTray: true,
  closeToTray: true,
  alwaysOnTop: false,
  hotkeysEnabled: true,
  startHotkey: "CommandOrControl+Alt+T",
  stopHotkey: "CommandOrControl+Alt+X",
  newlineMode: "enter",
  autoCheckUpdates: true,
};

export const IDLE_TYPING_STATE: TypingState = {
  phase: "idle",
  typed: 0,
  total: 0,
  countdown: 0,
};

export const DELAY_MIN = 1;
export const DELAY_MAX = 1000;
export const COUNTDOWN_MIN = 0;
export const COUNTDOWN_MAX = 10;
