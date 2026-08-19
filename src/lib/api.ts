/**
 * The only module in the app that talks to Tauri. Everything else imports from
 * here, so the backend contract lives in exactly one place.
 */
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

import type {
  HotkeyError,
  Settings,
  StorageInfo,
  Template,
  TrayStatus,
  TrayUnavailable,
  TypingDone,
  TypingState,
  UpdateInfo,
} from "./types";

/**
 * Backend commands reject with a plain human-readable string, but a transport
 * failure can surface as an Error. Normalize both into something showable.
 */
export function errorMessage(err: unknown): string {
  if (typeof err === "string") return err;
  if (err instanceof Error) return err.message;
  if (err && typeof err === "object" && "message" in err) {
    const message = (err as { message: unknown }).message;
    if (typeof message === "string") return message;
  }
  return "Something went wrong.";
}

/* -------------------------------------------------------------- commands -- */

export const getSettings = () => invoke<Settings>("get_settings");

export const saveSettings = (settings: Settings) =>
  invoke<Settings>("save_settings", { settings });

export const listTemplates = () => invoke<Template[]>("list_templates");

export const createTemplate = (name: string, content: string) =>
  invoke<Template>("create_template", { name, content });

export const updateTemplate = (id: string, name: string, content: string) =>
  invoke<Template>("update_template", { id, name, content });

export const deleteTemplate = (id: string) =>
  invoke<void>("delete_template", { id });

export const startTyping = (text: string) =>
  invoke<void>("start_typing", { text });

export const stopTyping = () => invoke<void>("stop_typing");

export const typingStatus = () => invoke<TypingState>("typing_status");

export const storageInfo = () => invoke<StorageInfo>("storage_info");

export const openDataFolder = () => invoke<void>("open_data_folder");

export const trayStatus = () => invoke<TrayStatus>("tray_status");

export const suspendHotkeys = () => invoke<void>("suspend_hotkeys");

export const resumeHotkeys = () => invoke<void>("resume_hotkeys");

export const checkForUpdates = () =>
  invoke<UpdateInfo | null>("check_for_updates");

export const installUpdate = () => invoke<void>("install_update");

export const openReleaseNotes = (version: string) =>
  invoke<void>("open_release_notes", { version });

export const appVersion = () => invoke<string>("app_version");

export const validateHotkey = (accelerator: string) =>
  invoke<void>("validate_hotkey", { accelerator });

/* ---------------------------------------------------------------- events -- */

const on = <T>(event: string, handler: (payload: T) => void) =>
  listen<T>(event, (e) => handler(e.payload));

export const onTypingState = (h: (s: TypingState) => void) =>
  on<TypingState>("typing://state", h);

export const onTypingDone = (h: (d: TypingDone) => void) =>
  on<TypingDone>("typing://done", h);

export const onUpdateAvailable = (h: (u: UpdateInfo) => void) =>
  on<UpdateInfo>("update://available", h);

export const onStorageWarning = (h: (s: StorageInfo) => void) =>
  on<StorageInfo>("storage://warning", h);

export const onHotkeyStart = (h: () => void) =>
  on<unknown>("hotkey://start", () => h());

export const onHotkeyStop = (h: () => void) =>
  on<unknown>("hotkey://stop", () => h());

export const onHotkeyError = (h: (e: HotkeyError) => void) =>
  on<HotkeyError>("hotkey://error", h);

export const onTrayUnavailable = (h: (t: TrayUnavailable) => void) =>
  on<TrayUnavailable>("tray://unavailable", h);

/**
 * Registers a batch of listeners and returns a synchronous cleanup function.
 *
 * `listen` resolves asynchronously, so a StrictMode double-mount can tear down
 * before registration completes. The `cancelled` flag makes any late arrival
 * unlisten itself, which is what keeps dev from ending up with two of every
 * subscription (and therefore two `start_typing` calls per hotkey press).
 */
export function subscribe(pending: Array<Promise<UnlistenFn>>): () => void {
  let cancelled = false;
  const active: UnlistenFn[] = [];

  for (const p of pending) {
    p.then(
      (un) => {
        if (cancelled) un();
        else active.push(un);
      },
      () => {
        /* Registration only fails outside a Tauri webview (e.g. plain `vite`). */
      },
    );
  }

  return () => {
    cancelled = true;
    for (const un of active) un();
    active.length = 0;
  };
}
