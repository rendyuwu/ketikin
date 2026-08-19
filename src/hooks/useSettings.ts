import { useCallback, useEffect, useRef, useState } from "react";

import { errorMessage, getSettings, saveSettings } from "../lib/api";
import { DEFAULT_SETTINGS, type Settings } from "../lib/types";

const SAVE_DEBOUNCE_MS = 400;
const SAVED_FLASH_MS = 1800;

export type UseSettings = {
  settings: Settings;
  loaded: boolean;
  error: string | null;
  justSaved: boolean;
  update: (patch: Partial<Settings>) => void;
  retry: () => void;
  dismissError: () => void;
};

/**
 * Optimistic local edits, debounced save, and always re-render from whatever
 * the backend hands back (it clamps). A revision counter keeps a slow response
 * from clobbering edits the user made while it was in flight.
 */
export function useSettings(): UseSettings {
  const [settings, setSettings] = useState<Settings>(DEFAULT_SETTINGS);
  const [loaded, setLoaded] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [justSaved, setJustSaved] = useState(false);

  const latest = useRef<Settings>(settings);
  const revision = useRef(0);
  const saveTimer = useRef<number | null>(null);
  const flashTimer = useRef<number | null>(null);
  const mounted = useRef(true);

  const apply = useCallback((next: Settings) => {
    latest.current = next;
    setSettings(next);
  }, []);

  const flush = useCallback(async () => {
    saveTimer.current = null;
    const at = revision.current;
    const payload = latest.current;

    try {
      const normalized = await saveSettings(payload);
      if (!mounted.current) return;
      // Only adopt the normalized copy if nothing was edited meanwhile.
      if (revision.current === at) apply(normalized);
      setError(null);
      setJustSaved(true);
      if (flashTimer.current !== null) clearTimeout(flashTimer.current);
      flashTimer.current = window.setTimeout(() => {
        if (mounted.current) setJustSaved(false);
      }, SAVED_FLASH_MS);
    } catch (err) {
      if (mounted.current) setError(errorMessage(err));
    }
  }, [apply]);

  const load = useCallback(async () => {
    const at = revision.current;
    try {
      const loadedSettings = await getSettings();
      if (!mounted.current) return;
      if (revision.current === at) apply(loadedSettings);
      setError(null);
    } catch (err) {
      if (mounted.current) setError(errorMessage(err));
    } finally {
      if (mounted.current) setLoaded(true);
    }
  }, [apply]);

  useEffect(() => {
    mounted.current = true;
    void load();
    return () => {
      mounted.current = false;
      if (saveTimer.current !== null) clearTimeout(saveTimer.current);
      if (flashTimer.current !== null) clearTimeout(flashTimer.current);
    };
  }, [load]);

  const update = useCallback(
    (patch: Partial<Settings>) => {
      revision.current += 1;
      apply({ ...latest.current, ...patch });
      setJustSaved(false);
      if (saveTimer.current !== null) clearTimeout(saveTimer.current);
      saveTimer.current = window.setTimeout(() => void flush(), SAVE_DEBOUNCE_MS);
    },
    [apply, flush],
  );

  const retry = useCallback(() => {
    setError(null);
    void load();
  }, [load]);

  const dismissError = useCallback(() => setError(null), []);

  return { settings, loaded, error, justSaved, update, retry, dismissError };
}
