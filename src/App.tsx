import { useCallback, useEffect, useRef, useState } from "react";

import { Banner } from "./components/Banner";
import { useSettings } from "./hooks/useSettings";
import { useTemplates } from "./hooks/useTemplates";
import { useTyping } from "./hooks/useTyping";
import { useUpdater } from "./hooks/useUpdater";
import {
  errorMessage,
  hotkeyStatus,
  onHotkeyError,
  onHotkeyStart,
  onHotkeyStop,
  onStorageWarning,
  onTrayUnavailable,
  storageInfo,
  subscribe,
  trayStatus,
} from "./lib/api";
import {
  formatAccelerator,
  formatCount,
  isStorageUnreliable,
  sameStorage,
} from "./lib/format";
import type { HotkeyError, StorageInfo, TypingState } from "./lib/types";
import { SettingsPanel } from "./panels/SettingsPanel";
import { TemplatesPanel } from "./panels/TemplatesPanel";
import { TypePanel } from "./panels/TypePanel";

type TabId = "type" | "templates" | "settings";

/** The two hotkey slots, named by the wire type rather than restated. */
type Which = HotkeyError["which"];

const TABS: ReadonlyArray<{ id: TabId; label: string }> = [
  { id: "type", label: "Type" },
  { id: "templates", label: "Templates" },
  { id: "settings", label: "Settings" },
];

/**
 * The backend guarantees a non-null message whenever the tray is unavailable,
 * so this should never render. Kept because the wire type still permits null,
 * and a null there would show no banner at all — which is precisely the silent
 * stranding (no tray, no way to quit) that the whole check exists to prevent.
 */
const TRAY_FALLBACK =
  "The system tray is unavailable on this system, so Ketikin will not minimize or close to the tray.";

function statusLabel(state: TypingState): string {
  if (state.phase === "countdown") return `Starting in ${state.countdown}…`;
  if (state.phase === "typing") {
    return `Typing ${formatCount(state.typed)} / ${formatCount(state.total)}`;
  }
  return "Ready";
}

export default function App() {
  const [tab, setTab] = useState<TabId>("type");
  const [text, setText] = useState("");
  const [storage, setStorage] = useState<StorageInfo | null>(null);
  const [storageError, setStorageError] = useState<string | null>(null);
  const [storageDismissed, setStorageDismissed] = useState(false);
  const [trayMessage, setTrayMessage] = useState<string | null>(null);
  const [trayError, setTrayError] = useState<string | null>(null);
  const [hotkeyErrors, setHotkeyErrors] = useState<Record<Which, string | null>>(
    { start: null, stop: null },
  );

  // Which slots have had their error state decided since mount — by an event,
  // or by the user recapturing. `hotkey://error` describes a failure happening
  // now, so it always wins. `hotkey_status()` is the backfill for the errors
  // emitted during the backend's `setup`, before any listener could exist, so
  // it may only fill slots nothing has spoken for yet, and neither clobbers
  // the other.
  //
  // The backend clears a slot's failure on a successful rebind, so the poll
  // cannot carry a stale error. This guards the narrower thing the backend
  // can't see: its snapshot was taken before an event that has already landed
  // here, or before the user cleared the field by recapturing.
  const hotkeySettled = useRef<Record<Which, boolean>>({
    start: false,
    stop: false,
  });

  const settings = useSettings();
  const templates = useTemplates();
  const typing = useTyping();
  const updater = useUpdater();

  const { start: beginTyping, refresh: refreshTyping } = typing;

  // The hotkey listener is mount-scoped, so it reads the text through a ref
  // rather than capturing a value that goes stale on the first keystroke.
  const textRef = useRef(text);
  useEffect(() => {
    textRef.current = text;
  }, [text]);

  const tabRefs = useRef<Array<HTMLButtonElement | null>>([]);

  useEffect(() => {
    document.documentElement.dataset.theme = settings.settings.theme;
  }, [settings.settings.theme]);

  const noteHotkeyError = useCallback((err: HotkeyError) => {
    hotkeySettled.current[err.which] = true;
    setHotkeyErrors((prev) => ({ ...prev, [err.which]: err.message }));
  }, []);

  const backfillHotkeyErrors = useCallback((failures: HotkeyError[]) => {
    const pending = failures.filter(
      (failure) => !hotkeySettled.current[failure.which],
    );
    if (pending.length === 0) return;
    setHotkeyErrors((prev) => {
      const next = { ...prev };
      // At most one entry per slot by contract — the backend collapses to the
      // latest, so a stale release failure never sits behind a fresh one.
      for (const failure of pending) next[failure.which] = failure.message;
      return next;
    });
  }, []);

  useEffect(() => {
    let alive = true;

    // `tray_status()` is the authoritative read: the tray outcome is decided
    // synchronously in the backend's setup hook, before the event loop that
    // handles IPC even starts, so a poll can never observe a premature
    // "available". `tray://unavailable` is the redundant nudge — it fires once
    // ~1.5s in and isn't buffered, so a slow WebView2 cold start misses it.
    // The two always agree, so first writer wins and neither can clobber.
    const noteTray = (message: string) =>
      setTrayMessage((previous) => previous ?? message);

    // Latest storage payload from either channel. `storage_info()` and
    // `storage://warning` are two views of one backend state that land ~1.5s
    // apart, so identity is what tells a redundant repeat apart from a new
    // problem — and therefore whether a dismissal still applies. Resetting the
    // flag unconditionally popped the banner straight back up on anyone who
    // dismissed it inside that window.
    let seenStorage: StorageInfo | null = null;
    const noteStorage = (info: StorageInfo) => {
      if (!sameStorage(seenStorage, info)) setStorageDismissed(false);
      seenStorage = info;
      setStorage(info);
    };

    const unsubscribe = subscribe([
      onStorageWarning((info) => noteStorage(info)),
      onTrayUnavailable(({ message }) => noteTray(message)),
    ]);

    storageInfo()
      .then((info) => {
        if (alive) noteStorage(info);
      })
      .catch((err: unknown) => {
        if (alive) setStorageError(errorMessage(err));
      });

    trayStatus()
      .then(({ available, message }) => {
        if (alive && !available) noteTray(message ?? TRAY_FALLBACK);
      })
      .catch((err: unknown) => {
        if (alive) setTrayError(errorMessage(err));
      });

    hotkeyStatus()
      .then(({ failures }) => {
        if (alive) backfillHotkeyErrors(failures);
      })
      .catch(() => {
        // Deliberately silent. Nothing here is recoverable by the user, and a
        // rejection means the IPC bridge itself is down — in which case
        // `get_settings` has already failed and is showing a banner about it.
        // A second banner would only report the same outage twice.
      });

    return () => {
      alive = false;
      unsubscribe();
    };
  }, [backfillHotkeyErrors]);

  useEffect(
    () =>
      subscribe([
        onHotkeyStart(() => beginTyping(textRef.current)),
        // The backend has already stopped; just resynchronise the UI.
        onHotkeyStop(() => refreshTyping()),
        onHotkeyError(noteHotkeyError),
      ]),
    [beginTyping, noteHotkeyError, refreshTyping],
  );

  // Recapturing settles the slot: the backfill must not put a startup failure
  // back after the user has replaced the accelerator that caused it.
  const clearHotkeyError = useCallback((which: Which) => {
    hotkeySettled.current[which] = true;
    setHotkeyErrors((prev) => ({ ...prev, [which]: null }));
  }, []);

  function onTabKeyDown(event: React.KeyboardEvent, index: number) {
    let next = -1;
    if (event.key === "ArrowRight") next = (index + 1) % TABS.length;
    else if (event.key === "ArrowLeft") next = (index - 1 + TABS.length) % TABS.length;
    else if (event.key === "Home") next = 0;
    else if (event.key === "End") next = TABS.length - 1;
    if (next < 0) return;

    event.preventDefault();
    setTab(TABS[next].id);
    tabRefs.current[next]?.focus();
  }

  /**
   * The shortcut to draw inside a Start / Stop button, or `null` for none.
   *
   * Omitted rather than greyed out when hotkeys are switched off or the OS
   * refused the bind: the alternative is a button that advertises a shortcut
   * which does nothing, which is the one thing a shortcut hint must never do.
   * The rule lives here because this is the only place that can see both the
   * settings and the registration failures.
   */
  function accelerator(which: Which, value: string): string | null {
    if (!settings.settings.hotkeysEnabled || hotkeyErrors[which] !== null) {
      return null;
    }
    const trimmed = value.trim();
    return trimmed === "" ? null : formatAccelerator(trimmed);
  }

  const storageUnreliable = storage ? isStorageUnreliable(storage) : false;
  const storagePinned = storage?.source === "memory";
  const notices = storage?.notices ?? [];
  // Notices can arrive on a perfectly healthy path — a reset templates file is
  // worth saying out loud even when nothing is degraded.
  // The backend's verdict, not a re-derivation: a portable install carries
  // notices but is deliberately not degraded, so it must not raise a banner.
  const showStorage = storage !== null && storage.degraded && !storageDismissed;
  const updateInfo = updater.dismissed ? null : updater.info;

  // Zero during the countdown, when `total` is already known but nothing has
  // been typed — so the rail's track appears the moment a run is accepted and
  // starts filling when the keystrokes do.
  const runState = typing.state;
  const railPercent =
    runState.total > 0
      ? Math.min(100, (runState.typed / runState.total) * 100)
      : 0;

  return (
    <div className="app">
      {/* The progress indicator lives on the window's own top edge rather than
          inside the panel, because Ketikin is deliberately behind another
          window for the whole time it is working: the user starts a run and
          clicks into a KVM console. A bar in the content area is one that never
          gets read. This is the only element that survives the window being
          almost entirely occluded, which is the normal condition here. */}
      {runState.phase !== "idle" ? (
        <div
          className="rail"
          role="progressbar"
          aria-valuemin={0}
          aria-valuemax={runState.total}
          aria-valuenow={runState.typed}
          aria-label="Typing progress"
        >
          <div className="rail-fill" style={{ width: `${railPercent}%` }} />
        </div>
      ) : null}

      {/* The tabs and the status share one row, and there is no wordmark: the
          OS titlebar already says "Ketikin". */}
      <header className="header">
        <div className="tabs" role="tablist" aria-label="Sections">
          {TABS.map((entry, index) => (
            <button
              key={entry.id}
              type="button"
              role="tab"
              id={`tab-${entry.id}`}
              className="tab"
              aria-selected={tab === entry.id}
              // Only the selected panel is mounted, so only it can be referenced.
              aria-controls={tab === entry.id ? `panel-${entry.id}` : undefined}
              tabIndex={tab === entry.id ? 0 : -1}
              ref={(el) => {
                tabRefs.current[index] = el;
              }}
              onClick={() => setTab(entry.id)}
              onKeyDown={(event) => onTabKeyDown(event, index)}
            >
              {entry.label}
            </button>
          ))}
        </div>

        <span className="status" aria-live="polite">
          {statusLabel(typing.state)}
        </span>
      </header>

      {settings.error || showStorage || trayMessage || trayError || updateInfo ||
      updater.error ? (
        <div className="banners">
          {settings.error ? (
            <Banner
              tone="error"
              onDismiss={settings.dismissError}
              actions={
                <>
                  <button
                    type="button"
                    className="btn btn--small"
                    onClick={settings.retry}
                  >
                    Retry
                  </button>
                  {/* Only a failed save has edits worth discarding. */}
                  {settings.error.kind === "save" ? (
                    <button
                      type="button"
                      className="btn btn--quiet btn--small"
                      onClick={settings.discard}
                    >
                      Discard changes
                    </button>
                  ) : null}
                </>
              }
            >
              {settings.error.message}
            </Banner>
          ) : null}

          {showStorage ? (
            <Banner
              tone="notice"
              onDismiss={storagePinned ? undefined : () => setStorageDismissed(true)}
            >
              {storageUnreliable ? (
                <span className="banner-line">
                  {storagePinned
                    ? "Settings and templates are not being saved to disk. They will be lost when Ketikin closes."
                    : "Settings and templates may not be saved reliably. See Storage in Settings."}
                </span>
              ) : null}
              {/* Dismissing only hides the banner — Settings > Storage keeps
                  showing these so they can always be found again. */}
              {notices.map((notice) => (
                <span className="banner-line" key={notice}>
                  {notice}
                </span>
              ))}
            </Banner>
          ) : null}

          {trayMessage ? <Banner tone="notice">{trayMessage}</Banner> : null}

          {trayError ? (
            <Banner tone="error" onDismiss={() => setTrayError(null)}>
              {trayError}
            </Banner>
          ) : null}

          {updateInfo ? (
            <Banner
              tone="notice"
              onDismiss={updater.installing ? undefined : updater.dismiss}
              actions={
                /* Gated here, not in the handler: when Ketikin can't replace
                   its own binary the install button never exists. */
                updateInfo.canInstall ? (
                  <>
                    <button
                      type="button"
                      className="btn btn--primary btn--small"
                      disabled={updater.installing}
                      onClick={updater.install}
                    >
                      {updater.installing ? "Installing…" : "Restart & install"}
                    </button>
                    <button
                      type="button"
                      className="link"
                      onClick={() => updater.openNotes(updateInfo.version)}
                    >
                      Release notes
                    </button>
                  </>
                ) : (
                  <button
                    type="button"
                    className="btn btn--primary btn--small"
                    onClick={() => updater.openNotes(updateInfo.version)}
                  >
                    Download
                  </button>
                )
              }
            >
              Ketikin {updateInfo.version} is available.
              {updateInfo.canInstall ? null : (
                <span className="banner-note">
                  Ketikin was installed from a system package and can't update
                  itself. Download the new version from the releases page.
                </span>
              )}
            </Banner>
          ) : null}

          {updater.error ? (
            <Banner tone="error" onDismiss={updater.dismissError}>
              {updater.error}
            </Banner>
          ) : null}
        </div>
      ) : null}

      <main
        className="content"
        role="tabpanel"
        id={`panel-${tab}`}
        aria-labelledby={`tab-${tab}`}
      >
        {tab === "type" ? (
          <TypePanel
            text={text}
            onTextChange={setText}
            typingDelayMs={settings.settings.typingDelayMs}
            startDelaySecs={settings.settings.startDelaySecs}
            onDelayChange={(typingDelayMs) => settings.update({ typingDelayMs })}
            state={typing.state}
            result={typing.result}
            starting={typing.starting}
            startAccelerator={accelerator("start", settings.settings.startHotkey)}
            stopAccelerator={accelerator("stop", settings.settings.stopHotkey)}
            onStart={() => beginTyping(text)}
            onStop={typing.stop}
            onDismissResult={typing.dismissResult}
          />
        ) : null}

        {tab === "templates" ? (
          <TemplatesPanel
            templates={templates}
            onUse={(content) => {
              setText(content);
              setTab("type");
            }}
          />
        ) : null}

        {tab === "settings" ? (
          <SettingsPanel
            settings={settings.settings}
            justSaved={settings.justSaved}
            onChange={settings.update}
            hotkeyErrors={hotkeyErrors}
            onClearHotkeyError={clearHotkeyError}
            trayUnavailable={trayMessage !== null}
            updater={updater}
            storage={storage}
            storageError={storageError}
          />
        ) : null}
      </main>
    </div>
  );
}
