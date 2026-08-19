import { useState } from "react";

import { Field } from "../components/Field";
import { HotkeyInput } from "../components/HotkeyInput";
import { NumberInput } from "../components/NumberInput";
import { Toggle } from "../components/Toggle";
import type { UseUpdater } from "../hooks/useUpdater";
import { errorMessage, validateHotkey } from "../lib/api";
import { describeStorage, isStorageDegraded } from "../lib/format";
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

  async function captureHotkey(which: Which, accelerator: string) {
    setValidationErrors((prev) => ({ ...prev, [which]: null }));
    onClearHotkeyError(which);
    try {
      await validateHotkey(accelerator);
      onChange(
        which === "start"
          ? { startHotkey: accelerator }
          : { stopHotkey: accelerator },
      );
    } catch (err) {
      setValidationErrors((prev) => ({ ...prev, [which]: errorMessage(err) }));
    }
  }

  function resetHotkey(which: Which) {
    setValidationErrors((prev) => ({ ...prev, [which]: null }));
    onClearHotkeyError(which);
    onChange(
      which === "start"
        ? { startHotkey: DEFAULT_SETTINGS.startHotkey }
        : { stopHotkey: DEFAULT_SETTINGS.stopHotkey },
    );
  }

  const degraded = storage ? isStorageDegraded(storage) : false;
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
            <div className={degraded ? "storage storage--warn" : "storage"}>
              <code className="storage-path">{storage.path}</code>
              <p className="storage-note">{describeStorage(storage)}</p>
              {!storage.writable ? (
                <p className="storage-note">This location is not writable.</p>
              ) : null}
              {storage.error ? (
                <p className="storage-note">{storage.error}</p>
              ) : null}
            </div>
          ) : null}
        </section>
      </div>
    </div>
  );
}
