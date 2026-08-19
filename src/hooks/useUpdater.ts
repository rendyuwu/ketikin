import { useCallback, useEffect, useRef, useState } from "react";

import {
  appVersion,
  checkForUpdates,
  errorMessage,
  installUpdate,
  onUpdateAvailable,
  openReleaseNotes,
  subscribe,
} from "../lib/api";
import type { UpdateInfo } from "../lib/types";

export type CheckResult = { ok: boolean; text: string };

export type UseUpdater = {
  version: string | null;
  info: UpdateInfo | null;
  dismissed: boolean;
  checking: boolean;
  checkResult: CheckResult | null;
  installing: boolean;
  /** Failure of install / release-notes / version lookup, shown in the banner. */
  error: string | null;
  check: () => void;
  install: () => void;
  openNotes: (version: string) => void;
  dismiss: () => void;
  dismissError: () => void;
};

export function useUpdater(): UseUpdater {
  const [version, setVersion] = useState<string | null>(null);
  const [info, setInfo] = useState<UpdateInfo | null>(null);
  const [dismissed, setDismissed] = useState(false);
  const [checking, setChecking] = useState(false);
  const [checkResult, setCheckResult] = useState<CheckResult | null>(null);
  const [installing, setInstalling] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const mounted = useRef(true);

  useEffect(() => {
    mounted.current = true;

    const unsubscribe = subscribe([
      onUpdateAvailable((next) => {
        setInfo(next);
        setDismissed(false);
      }),
    ]);

    appVersion()
      .then((v) => {
        if (mounted.current) setVersion(v);
      })
      .catch((err: unknown) => {
        if (mounted.current) setError(errorMessage(err));
      });

    return () => {
      mounted.current = false;
      unsubscribe();
    };
  }, []);

  const check = useCallback(() => {
    setChecking(true);
    setCheckResult(null);

    checkForUpdates()
      .then((next) => {
        if (!mounted.current) return;
        if (next) {
          setInfo(next);
          setDismissed(false);
          setCheckResult({ ok: true, text: `Ketikin ${next.version} is available.` });
        } else {
          // `null` authoritatively means up to date — not an error, and not
          // "unknown". Drop any earlier `update://available` info so the banner
          // can't keep advertising a version this check just ruled out.
          setInfo(null);
          setCheckResult({ ok: true, text: "You're on the latest version." });
        }
      })
      .catch((err: unknown) => {
        if (mounted.current) setCheckResult({ ok: false, text: errorMessage(err) });
      })
      .finally(() => {
        if (mounted.current) setChecking(false);
      });
  }, []);

  const install = useCallback(() => {
    setInstalling(true);
    setError(null);

    // On success the app restarts, so the in-flight state simply never clears.
    installUpdate().catch((err: unknown) => {
      if (!mounted.current) return;
      setError(errorMessage(err));
      setInstalling(false);
    });
  }, []);

  const openNotes = useCallback((forVersion: string) => {
    openReleaseNotes(forVersion).catch((err: unknown) => {
      if (mounted.current) setError(errorMessage(err));
    });
  }, []);

  const dismiss = useCallback(() => setDismissed(true), []);
  const dismissError = useCallback(() => setError(null), []);

  return {
    version,
    info,
    dismissed,
    checking,
    checkResult,
    installing,
    error,
    check,
    install,
    openNotes,
    dismiss,
    dismissError,
  };
}
