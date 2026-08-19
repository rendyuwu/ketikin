import { useCallback, useEffect, useRef, useState } from "react";

import {
  createTemplate,
  deleteTemplate,
  errorMessage,
  listTemplates,
  updateTemplate,
} from "../lib/api";
import type { Template } from "../lib/types";

export type UseTemplates = {
  templates: Template[];
  loading: boolean;
  error: string | null;
  create: (name: string, content: string) => Promise<boolean>;
  save: (id: string, name: string, content: string) => Promise<boolean>;
  remove: (id: string) => Promise<boolean>;
  dismissError: () => void;
};

const byName = (a: Template, b: Template) =>
  a.name.localeCompare(b.name, undefined, { sensitivity: "base" });

export function useTemplates(): UseTemplates {
  const [templates, setTemplates] = useState<Template[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const mounted = useRef(true);

  useEffect(() => {
    mounted.current = true;
    listTemplates()
      .then((list) => {
        if (mounted.current) setTemplates([...list].sort(byName));
      })
      .catch((err: unknown) => {
        if (mounted.current) setError(errorMessage(err));
      })
      .finally(() => {
        if (mounted.current) setLoading(false);
      });
    return () => {
      mounted.current = false;
    };
  }, []);

  const create = useCallback(async (name: string, content: string) => {
    try {
      const created = await createTemplate(name, content);
      setTemplates((prev) => [...prev, created].sort(byName));
      setError(null);
      return true;
    } catch (err) {
      setError(errorMessage(err));
      return false;
    }
  }, []);

  const save = useCallback(
    async (id: string, name: string, content: string) => {
      try {
        const saved = await updateTemplate(id, name, content);
        setTemplates((prev) =>
          prev.map((t) => (t.id === saved.id ? saved : t)).sort(byName),
        );
        setError(null);
        return true;
      } catch (err) {
        setError(errorMessage(err));
        return false;
      }
    },
    [],
  );

  const remove = useCallback(async (id: string) => {
    try {
      await deleteTemplate(id);
      setTemplates((prev) => prev.filter((t) => t.id !== id));
      setError(null);
      return true;
    } catch (err) {
      setError(errorMessage(err));
      return false;
    }
  }, []);

  const dismissError = useCallback(() => setError(null), []);

  return { templates, loading, error, create, save, remove, dismissError };
}
