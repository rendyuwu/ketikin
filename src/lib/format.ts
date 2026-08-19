import type { NewlineMode, StorageInfo, StorageSource } from "./types";

/** Rough platform sniff — the OS plugin isn't a dependency and isn't worth one. */
const IS_MAC = /Mac|iPhone|iPad|iPod/i.test(
  typeof navigator === "undefined" ? "" : navigator.userAgent,
);

export const formatCount = (n: number): string => n.toLocaleString();

/**
 * Keystrokes the backend will actually send. Line breaks cost one keystroke in
 * `enter`/`shiftEnter` mode and nothing at all in `skip` mode.
 */
export function keystrokeCount(text: string, mode: NewlineMode): number {
  const normalized = text.replace(/\r\n/g, "\n");
  return mode === "skip"
    ? normalized.replace(/\n/g, "").length
    : normalized.length;
}

export function estimateMs(
  text: string,
  mode: NewlineMode,
  typingDelayMs: number,
  startDelaySecs: number,
): number {
  return startDelaySecs * 1000 + keystrokeCount(text, mode) * typingDelayMs;
}

export function formatDuration(ms: number): string {
  const seconds = Math.round(ms / 1000);
  if (seconds < 1) return "under a second";
  if (seconds < 60) return `about ${seconds}s`;

  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) {
    const rest = seconds % 60;
    return rest === 0 ? `about ${minutes}m` : `about ${minutes}m ${rest}s`;
  }

  const hours = Math.floor(minutes / 60);
  const restMinutes = minutes % 60;
  return restMinutes === 0
    ? `about ${hours}h`
    : `about ${hours}h ${restMinutes}m`;
}

const STORAGE_DESCRIPTIONS: Record<StorageSource, string> = {
  appData: "Stored in the standard application data folder.",
  appDataEnv: "Stored in the folder named by the APPDATA environment variable.",
  localAppDataEnv:
    "Stored in the folder named by the LOCALAPPDATA environment variable.",
  nextToExe: "Stored next to the Ketikin executable.",
  temp: "Temporary directory — data may not survive a reboot.",
  memory: "Not saved to disk. Changes will be lost when Ketikin closes.",
};

export const describeStorage = (info: StorageInfo): string =>
  STORAGE_DESCRIPTIONS[info.source] ?? "Location unknown.";

/**
 * True when the *location itself* puts settings and templates at risk of being
 * lost. This is not the same question as `StorageInfo.degraded` and must not be
 * used to gate the banner — it exists solely to gate the sentence "Settings and
 * templates may not be saved reliably", which would be false for a reset
 * templates file on a healthy appData path, and false for a portable install.
 */
export const isStorageUnreliable = (info: StorageInfo): boolean =>
  !info.writable || info.source === "temp" || info.source === "memory";

const MODIFIER_LABELS: Record<string, string> = {
  CommandOrControl: IS_MAC ? "⌘" : "Ctrl",
  CmdOrCtrl: IS_MAC ? "⌘" : "Ctrl",
  Command: "⌘",
  Cmd: "⌘",
  Super: IS_MAC ? "⌘" : "Super",
  Meta: IS_MAC ? "⌘" : "Super",
  Control: "Ctrl",
  Ctrl: "Ctrl",
  Alt: "Alt",
  Option: "Alt",
  Shift: "Shift",
};

/** Turns a Tauri accelerator into something a person reads on this platform. */
export function formatAccelerator(accelerator: string): string {
  if (!accelerator) return "Not set";
  return accelerator
    .split("+")
    .map((part) => MODIFIER_LABELS[part] ?? part)
    .join("+");
}

/** Builds a Tauri accelerator from a real keypress. `null` when only modifiers are down. */
export function acceleratorFromEvent(event: KeyboardEvent): string | null {
  const key = keyFromCode(event.code, event.key);
  if (key === null) return null;

  const parts: string[] = [];
  // The primary modifier is Cmd on macOS and Ctrl everywhere else, so the two
  // physical keys map to different accelerators per platform. Collapsing both
  // onto CommandOrControl would bind Super+K as Ctrl+K on Linux and silently
  // drop the Ctrl from Ctrl+Cmd+T on macOS.
  if (IS_MAC) {
    if (event.metaKey) parts.push("CommandOrControl");
    if (event.ctrlKey) parts.push("Control");
  } else {
    if (event.ctrlKey) parts.push("CommandOrControl");
    if (event.metaKey) parts.push("Super");
  }
  if (event.altKey) parts.push("Alt");
  if (event.shiftKey) parts.push("Shift");
  parts.push(key);
  return parts.join("+");
}

export const hasModifier = (accelerator: string): boolean =>
  accelerator.split("+").length > 1;

const PUNCTUATION: Record<string, string> = {
  Minus: "-",
  Equal: "=",
  BracketLeft: "[",
  BracketRight: "]",
  Backslash: "\\",
  Semicolon: ";",
  Quote: "'",
  Comma: ",",
  Period: ".",
  Slash: "/",
  Backquote: "`",
};

const NAMED_KEYS = new Set([
  "Space",
  "Enter",
  "Tab",
  "Backspace",
  "Delete",
  "Insert",
  "Home",
  "End",
  "PageUp",
  "PageDown",
]);

const ARROWS: Record<string, string> = {
  ArrowUp: "Up",
  ArrowDown: "Down",
  ArrowLeft: "Left",
  ArrowRight: "Right",
};

/**
 * Prefers `event.code` so the chord doesn't change with the keyboard layout or
 * with Alt producing a different `event.key`.
 */
function keyFromCode(code: string, key: string): string | null {
  if (/^Key[A-Z]$/.test(code)) return code.slice(3);
  if (/^Digit\d$/.test(code)) return code.slice(5);
  if (/^Numpad\d$/.test(code)) return `Num${code.slice(6)}`;
  if (/^F\d{1,2}$/.test(code)) return code;
  if (code in PUNCTUATION) return PUNCTUATION[code];
  if (code in ARROWS) return ARROWS[code];
  if (NAMED_KEYS.has(code)) return code;

  // Bare modifier presses carry no key of their own.
  if (/^(Control|Alt|Shift|Meta|OS)(Left|Right)?$/.test(code)) return null;
  if (key.length === 1) return key.toUpperCase();
  return null;
}
