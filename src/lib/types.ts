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
};

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
