import { useEffect, useRef, useState } from "react";

import { Banner } from "../components/Banner";
import { ConfirmDialog } from "../components/ConfirmDialog";
import { PencilIcon, PlusIcon, TrashIcon } from "../components/Icons";
import type { UseTemplates } from "../hooks/useTemplates";
import type { Template } from "../lib/types";

type TemplatesPanelProps = {
  templates: UseTemplates;
  onUse: (content: string) => void;
};

type Draft = { id: string | null; name: string; content: string };

const EMPTY_DRAFT: Draft = { id: null, name: "", content: "" };

/**
 * Slice first: the row is a single ellipsised line, so collapsing whitespace
 * across a whole large template on every render is wasted work.
 */
const PREVIEW_CHARS = 140;
const preview = (content: string) =>
  content.slice(0, PREVIEW_CHARS).replace(/\s+/g, " ").trim();

export function TemplatesPanel({ templates, onUse }: TemplatesPanelProps) {
  const { templates: items, loading, error, create, save, remove, dismissError } =
    templates;

  const [draft, setDraft] = useState<Draft | null>(null);
  const [validation, setValidation] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [confirming, setConfirming] = useState<Template | null>(null);
  const nameInput = useRef<HTMLInputElement>(null);

  /**
   * Counts form *openings*, and is the only thing the focus effect below may
   * depend on.
   *
   * `draft` is not usable as that dependency, however natural it looks: both
   * `onChange` handlers replace the whole draft object on every keystroke, so
   * its identity changes per character. Depending on it re-ran the focus effect
   * mid-typing and pulled the caret out of the content textarea and back into
   * the name field on each character. Nor is `draft?.id` enough — it is `null`
   * both for a new template and for no form at all, so opening the new-template
   * form would never focus anything.
   *
   * Starts at 0, which is the "no form has been opened yet" state, so a mount
   * with the list showing focuses nothing.
   */
  const [opened, setOpened] = useState(0);

  useEffect(() => {
    if (opened > 0) nameInput.current?.focus();
  }, [opened]);

  function openNew() {
    setValidation(null);
    setDraft(EMPTY_DRAFT);
    setOpened((count) => count + 1);
  }

  function openEdit(template: Template) {
    setValidation(null);
    setDraft({ id: template.id, name: template.name, content: template.content });
    setOpened((count) => count + 1);
  }

  async function submit(event: React.FormEvent) {
    event.preventDefault();
    if (!draft || busy) return;

    const name = draft.name.trim();
    if (!name) {
      setValidation("Give the template a name.");
      return;
    }
    if (!draft.content) {
      setValidation("A template needs some content.");
      return;
    }

    setValidation(null);
    setBusy(true);
    const ok = draft.id
      ? await save(draft.id, name, draft.content)
      : await create(name, draft.content);
    setBusy(false);
    if (ok) setDraft(null);
  }

  async function confirmDelete() {
    if (!confirming || busy) return;
    setBusy(true);
    const ok = await remove(confirming.id);
    setBusy(false);
    if (ok) {
      if (draft?.id === confirming.id) setDraft(null);
      setConfirming(null);
    }
  }

  return (
    <div className="panel templates-panel">
      <div className="templates-head">
        <h2 className="section-title">Templates</h2>
        <button
          type="button"
          className="btn btn--small btn--icon"
          onClick={openNew}
        >
          <PlusIcon size={12} />
          New
        </button>
      </div>

      {error ? (
        <Banner tone="error" onDismiss={dismissError}>
          {error}
        </Banner>
      ) : null}

      <div className="templates-scroll">
        {loading ? <p className="meta">Loading…</p> : null}

        {draft ? (
          <form className="template-form" onSubmit={submit}>
            <h2 className="section-title">
              {draft.id ? "Edit template" : "New template"}
            </h2>
            <input
              ref={nameInput}
              className="input"
              value={draft.name}
              placeholder="Name"
              aria-label="Template name"
              maxLength={200}
              onChange={(e) => setDraft({ ...draft, name: e.target.value })}
            />
            <textarea
              className="textarea textarea--form"
              value={draft.content}
              placeholder="Template content"
              aria-label="Template content"
              spellCheck={false}
              onChange={(e) => setDraft({ ...draft, content: e.target.value })}
            />
            {validation ? (
              <p className="field-error" role="alert">
                {validation}
              </p>
            ) : null}
            <div className="template-form-actions">
              <button
                type="button"
                className="btn btn--quiet btn--small"
                onClick={() => setDraft(null)}
                disabled={busy}
              >
                Cancel
              </button>
              <button
                type="submit"
                className="btn btn--primary btn--small"
                disabled={busy}
              >
                {busy ? "Saving…" : "Save"}
              </button>
            </div>
          </form>
        ) : null}

        {!loading && items.length === 0 && !draft ? (
          <div className="empty">
            <p>
              Save text you paste often. Templates load straight into the Type
              tab.
            </p>
            <button
              type="button"
              className="btn btn--primary btn--icon"
              onClick={openNew}
            >
              <PlusIcon />
              New template
            </button>
          </div>
        ) : null}

        {items.length > 0 ? (
          <ul className="templates-list">
            {items.map((template) => (
              <li className="template" key={template.id}>
                {/*
                  A real button filling the row rather than a handler on the
                  <li>: Enter and Space, the disabled state and the focus ring
                  all come for free, and none of them do on a div. It is also
                  why the row cannot simply *wrap* the two actions — a button
                  inside a button is invalid and browsers unnest it.

                  The label is the verb, not the row's text: "Use “Bootstrap”"
                  says what pressing it does, where the name and the preview run
                  together into a sentence that starts with a fragment of shell
                  script.
                */}
                <button
                  type="button"
                  className="template-use"
                  aria-label={`Use “${template.name}”`}
                  onClick={() => onUse(template.content)}
                >
                  <span className="template-name">{template.name}</span>
                  <span className="template-preview">
                    {preview(template.content)}
                  </span>
                </button>
                <div className="template-actions">
                  <button
                    type="button"
                    className="icon-button"
                    aria-label={`Edit “${template.name}”`}
                    onClick={() => openEdit(template)}
                  >
                    <PencilIcon />
                  </button>
                  <button
                    type="button"
                    className="icon-button icon-button--danger"
                    aria-label={`Delete “${template.name}”`}
                    onClick={() => setConfirming(template)}
                  >
                    <TrashIcon />
                  </button>
                </div>
              </li>
            ))}
          </ul>
        ) : null}
      </div>

      {confirming ? (
        <ConfirmDialog
          title="Delete template"
          message={`“${confirming.name}” will be removed permanently.`}
          confirmLabel="Delete"
          busy={busy}
          onConfirm={() => void confirmDelete()}
          onCancel={() => setConfirming(null)}
        />
      ) : null}
    </div>
  );
}
