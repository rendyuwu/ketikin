import { useState } from "react";

import { Field } from "../components/Field";
import { HotkeyInput } from "../components/HotkeyInput";
import { NumberInput } from "../components/NumberInput";
import { Toggle } from "../components/Toggle";
import type { UseUpdater } from "../hooks/useUpdater";
import { errorMessage, openDataFolder, validateHotkey } from "../lib/api";
import { describeStorage } from "../lib/format";
import {
  COUNTDOWN_MAX,
  COUNTDOWN_MIN,
  DEFAULT_SETTINGS,
  DELAY_MAX,
  DELAY_MIN,
  type NewlineMode,
  type Settings,
  type StorageInfo,
  type Theme,
} from "../lib/types";

type Which = "start" | "stop";

type SettingsPanelProps = {
  settings: Settings;
  justSaved: boolean;
  onChange: (patch: Partial<Settings>) => void;
  hotkeyErrors: Record<Which, string | null>;
  onClearHotkeyError: (which: Which) => void;
  /** `tray://unavailable` fired: the tray toggles still save, but do nothing. */
  trayUnavailable: boolean;
  updater: UseUpdater;
  storage: StorageInfo | null;
  storageError: string | null;
};

const NEWLINE_HELP: Record<NewlineMode, string> = {
  enter: "Line breaks press Enter.",
  shiftEnter:
    "Line breaks press Shift+Enter — for chat boxes and consoles where Enter submits.",
  skip: "Line breaks are ignored; the text is typed as one continuous line.",
};

export function SettingsPanel({
  settings,
  justSaved,
  onChange,
  hotkeyErrors,
  onClearHotkeyError,
  trayUnavailable,
  updater,
  storage,
  storageError,
}: SettingsPanelProps) {
  const [validationErrors, setValidationErrors] = useState<
    Record<Which, string | null>
  >({ start: null, stop: null });
  const [openFolderError, setOpenFolderError] = useState<string | null>(null);

  function openFolder() {
    setOpenFolderError(null);
    openDataFolder().catch((err: unknown) => setOpenFolderError(errorMessage(err)));
  }

  /**
   * `validate_hotkey` is per-field, so it can't see that Start and Stop have
   * been set to the same chord. `save_settings` does reject it — but only after
   * the debounce, and the rejected value stays in the pending settings, so
   * every subsequent save fails too and the user is stuck behind a recurring
   * error banner. Catching it here keeps the bad value from being committed.
   */
  function commitHotkey(which: Which, accelerator: string): boolean {
    const other = which === "start" ? settings.stopHotkey : settings.startHotkey;
    if (accelerator === other) {
      setValidationErrors((prev) => ({
        ...prev,
        [which]: "Start and stop must use different shortcuts.",
      }));
      return false;
    }
    onChange(
      which === "start"
        ? { startHotkey: accelerator }
        : { stopHotkey: accelerator },
    );
    return true;
  }

  async function captureHotkey(which: Which, accelerator: string) {
    setValidationErrors((prev) => ({ ...prev, [which]: null }));
    onClearHotkeyError(which);
    try {
      await validateHotkey(accelerator);
      commitHotkey(which, accelerator);
    } catch (err) {
      setValidationErrors((prev) => ({ ...prev, [which]: errorMessage(err) }));
    }
  }

  function resetHotkey(which: Which) {
    setValidationErrors((prev) => ({ ...prev, [which]: null }));
    onClearHotkeyError(which);
    // The two defaults differ, but the *other* field may already hold this
    // one's default, so resetting can conflict just like capturing can.
    commitHotkey(
      which,
      which === "start"
        ? DEFAULT_SETTINGS.startHotkey
        : DEFAULT_SETTINGS.stopHotkey,
    );
  }

  const notices = storage?.notices ?? [];
  // Styling only, inside a section the user chose to open — so notices count
  // here even when they don't warrant a banner. This is "is there something to
  // read", not "is this degraded".
  const storageWarn = storage ? storage.degraded || notices.length > 0 : false;
  const updateInfo = updater.info;

  return (
    <div className="panel settings-panel">
      <div className="settings-status" aria-live="polite">
        <span className="meta">Changes save automatically.</span>
        {justSaved ? <span className="saved-flash">Saved</span> : null}
      </div>

      <div className="settings-scroll">
        <section className="section">
          <h2 className="section-title">Typing</h2>

          <Field label="Delay (ms)" htmlFor="settings-delay">
            <NumberInput
              id="settings-delay"
              value={settings.typingDelayMs}
              min={DELAY_MIN}
              max={DELAY_MAX}
              onCommit={(typingDelayMs) => onChange({ typingDelayMs })}
            />
          </Field>

          <Field label="Countdown (seconds)" htmlFor="settings-countdown">
            <NumberInput
              id="settings-countdown"
              value={settings.startDelaySecs}
              min={COUNTDOWN_MIN}
              max={COUNTDOWN_MAX}
              onCommit={(startDelaySecs) => onChange({ startDelaySecs })}
            />
          </Field>

          <Field
            label="Newline handling"
            htmlFor="settings-newline"
            hint={NEWLINE_HELP[settings.newlineMode]}
          >
            <select
              id="settings-newline"
              className="input select"
              value={settings.newlineMode}
              onChange={(e) =>
                onChange({ newlineMode: e.target.value as NewlineMode })
              }
            >
              <option value="enter">Press Enter</option>
              <option value="shiftEnter">Press Shift+Enter</option>
              <option value="skip">Skip line breaks</option>
            </select>
          </Field>
        </section>

        <section className="section">
          <h2 className="section-title">Window</h2>

          <Field label="Theme" htmlFor="settings-theme">
            <select
              id="settings-theme"
              className="input select"
              value={settings.theme}
              onChange={(e) => onChange({ theme: e.target.value as Theme })}
            >
              <option value="system">System</option>
              <option value="dark">Dark</option>
              <option value="light">Light</option>
            </select>
          </Field>

          <Toggle
            label="Minimize to tray"
            checked={settings.minimizeToTray}
            onChange={(minimizeToTray) => onChange({ minimizeToTray })}
          />
          <Toggle
            label="Close to tray"
            checked={settings.closeToTray}
            onChange={(closeToTray) => onChange({ closeToTray })}
          />
          {trayUnavailable ? (
            <p className="group-note">
              The system tray is unavailable on this system, so these have no
              effect. Your preference is saved and will apply where a tray is
              available.
            </p>
          ) : null}
          <Toggle
            label="Always on top"
            checked={settings.alwaysOnTop}
            onChange={(alwaysOnTop) => onChange({ alwaysOnTop })}
          />
        </section>

        <section className="section">
          <h2 className="section-title">Hotkeys</h2>

          <Toggle
            label="Global hotkeys"
            description="Start and stop Ketikin without focusing its window."
            checked={settings.hotkeysEnabled}
            onChange={(hotkeysEnabled) => onChange({ hotkeysEnabled })}
          />

          <Field label="Start typing" htmlFor="hotkey-start">
            <HotkeyInput
              id="hotkey-start"
              label="Start typing hotkey"
              value={settings.startHotkey}
              defaultValue={DEFAULT_SETTINGS.startHotkey}
              disabled={!settings.hotkeysEnabled}
              error={validationErrors.start ?? hotkeyErrors.start}
              onCapture={(accelerator) => void captureHotkey("start", accelerator)}
              onReset={() => resetHotkey("start")}
            />
          </Field>

          <Field label="Stop typing" htmlFor="hotkey-stop">
            <HotkeyInput
              id="hotkey-stop"
              label="Stop typing hotkey"
              value={settings.stopHotkey}
              defaultValue={DEFAULT_SETTINGS.stopHotkey}
              disabled={!settings.hotkeysEnabled}
              error={validationErrors.stop ?? hotkeyErrors.stop}
              onCapture={(accelerator) => void captureHotkey("stop", accelerator)}
              onReset={() => resetHotkey("stop")}
            />
          </Field>
        </section>

        <section className="section">
          <h2 className="section-title">Updates</h2>

          <Toggle
            label="Check for updates automatically"
            checked={settings.autoCheckUpdates}
            onChange={(autoCheckUpdates) => onChange({ autoCheckUpdates })}
          />

          <p className="meta">
            Current version {updater.version ?? "unknown"}
          </p>

          <div className="row">
            <button
              type="button"
              className="btn btn--small"
              disabled={updater.checking}
              onClick={updater.check}
            >
              {updater.checking ? "Checking…" : "Check for updates"}
            </button>
            {updateInfo ? (
              <button
                type="button"
                className="link"
                onClick={() => updater.openNotes(updateInfo.version)}
              >
                Release notes
              </button>
            ) : null}
          </div>

          {updater.checkResult ? (
            <p
              className={updater.checkResult.ok ? "meta" : "field-error"}
              role="status"
            >
              {updater.checkResult.text}
            </p>
          ) : null}
        </section>

        <section className="section">
          <h2 className="section-title">Storage</h2>

          {storageError ? (
            <p className="field-error" role="alert">
              {storageError}
            </p>
          ) : null}

          {storage ? (
            <div className={storageWarn ? "storage storage--warn" : "storage"}>
              <code className="storage-path">{storage.path}</code>
              <p className="storage-note">{describeStorage(storage)}</p>
              {!storage.writable ? (
                <p className="storage-note">This location is not writable.</p>
              ) : null}
              {storage.error ? (
                <p className="storage-note">{storage.error}</p>
              ) : null}
              {/* Always rendered, regardless of whether the startup banner was
                  dismissed — this is where a user comes back to find them. */}
              {notices.length > 0 ? (
                <ul className="storage-notices">
                  {notices.map((notice) => (
                    <li key={notice}>{notice}</li>
                  ))}
                </ul>
              ) : null}
              <div className="storage-actions">
                {/* An addition, not a replacement: on a locked-down host the
                    button is likelier to fail than the path is to be unreadable.
                    Disabled only for in-memory storage, where no folder exists —
                    the reason is already spelled out in the line above. */}
                <button
                  type="button"
                  className="btn btn--small"
                  disabled={storage.source === "memory"}
                  onClick={openFolder}
                >
                  Open data folder
                </button>
              </div>
              {/* The log lives one level down, in logs/. Without this, someone
                  told "send me your log" opens the folder and sees only JSON. */}
              <p className="storage-note">
                Settings, templates, and a logs folder are stored here.
              </p>
              {openFolderError ? (
                <p className="field-error" role="alert">
                  {openFolderError}
                </p>
              ) : null}
            </div>
          ) : null}
        </section>
      </div>
    </div>
  );
}
