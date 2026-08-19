import { useCallback, useEffect, useRef, useState } from "react";

import { Banner } from "./components/Banner";
import { useSettings } from "./hooks/useSettings";
import { useTemplates } from "./hooks/useTemplates";
import { useTyping } from "./hooks/useTyping";
import { useUpdater } from "./hooks/useUpdater";
import {
  errorMessage,
  onHotkeyError,
  onHotkeyStart,
  onHotkeyStop,
  onStorageWarning,
  onTrayUnavailable,
  storageInfo,
  subscribe,
} from "./lib/api";
import { formatCount, isStorageDegraded } from "./lib/format";
import type { StorageInfo, TypingState } from "./lib/types";
import { SettingsPanel } from "./panels/SettingsPanel";
import { TemplatesPanel } from "./panels/TemplatesPanel";
import { TypePanel } from "./panels/TypePanel";

type TabId = "type" | "templates" | "settings";

const TABS: ReadonlyArray<{ id: TabId; label: string }> = [
  { id: "type", label: "Type" },
  { id: "templates", label: "Templates" },
  { id: "settings", label: "Settings" },
];

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
  const [hotkeyErrors, setHotkeyErrors] = useState<{
    start: string | null;
    stop: string | null;
  }>({ start: null, stop: null });

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

  useEffect(() => {
    let alive = true;

    const unsubscribe = subscribe([
      onStorageWarning((info) => {
        setStorage(info);
        setStorageDismissed(false);
      }),
      onTrayUnavailable(({ message }) => setTrayMessage(message)),
    ]);

    storageInfo()
      .then((info) => {
        if (alive) setStorage(info);
      })
      .catch((err: unknown) => {
        if (alive) setStorageError(errorMessage(err));
      });

    return () => {
      alive = false;
      unsubscribe();
    };
  }, []);

  useEffect(
    () =>
      subscribe([
        onHotkeyStart(() => beginTyping(textRef.current)),
        // The backend has already stopped; just resynchronise the UI.
        onHotkeyStop(() => refreshTyping()),
        onHotkeyError((err) =>
          setHotkeyErrors((prev) => ({ ...prev, [err.which]: err.message })),
        ),
      ]),
    [beginTyping, refreshTyping],
  );

  const clearHotkeyError = useCallback(
    (which: "start" | "stop") =>
      setHotkeyErrors((prev) => ({ ...prev, [which]: null })),
    [],
  );

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

  const storageDegraded = storage ? isStorageDegraded(storage) : false;
  const storagePinned = storage?.source === "memory";
  const updateInfo = updater.dismissed ? null : updater.info;

  return (
    <div className="app">
      <header className="header">
        <span className="wordmark">Ketikin</span>
        <span className="status" aria-live="polite">
          {statusLabel(typing.state)}
        </span>
      </header>

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

      {settings.error || (storageDegraded && !storageDismissed) || trayMessage ||
      updateInfo || updater.error ? (
        <div className="banners">
          {settings.error ? (
            <Banner
              tone="error"
              onDismiss={settings.dismissError}
              actions={
                <button type="button" className="btn btn--small" onClick={settings.retry}>
                  Retry
                </button>
              }
            >
              {settings.error}
            </Banner>
          ) : null}

          {storage && storageDegraded && !storageDismissed ? (
            <Banner
              tone="warn"
              onDismiss={storagePinned ? undefined : () => setStorageDismissed(true)}
            >
              {storagePinned
                ? "Settings and templates are not being saved to disk. They will be lost when Ketikin closes."
                : "Settings and templates may not be saved reliably. See Storage in Settings."}
            </Banner>
          ) : null}

          {trayMessage ? <Banner tone="warn">{trayMessage}</Banner> : null}

          {updateInfo ? (
            <Banner
              tone="info"
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
            newlineMode={settings.settings.newlineMode}
            onDelayChange={(typingDelayMs) => settings.update({ typingDelayMs })}
            state={typing.state}
            result={typing.result}
            starting={typing.starting}
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
