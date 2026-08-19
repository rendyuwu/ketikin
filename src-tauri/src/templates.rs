//! Saved snippets the user can replay into a console.
//!
//! Templates live in a single `templates.json` array. CRUD operates on an
//! in-memory `Vec<Template>` and persists the whole list after each mutation —
//! the list is small by construction and a whole-file atomic rewrite is far
//! easier to reason about than partial updates.

use chrono::Utc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::AppError;
use crate::storage::Storage;

/// File name (without extension) inside the resolved data directory.
const FILE: &str = "templates";

/// Same ceiling as the typing engine — a template you could never type is not
/// worth storing.
pub const MAX_CONTENT_CHARS: usize = 1_000_000;
const MAX_NAME_CHARS: usize = 200;

/// A named block of text.
///
/// `created_at` / `updated_at` are RFC 3339 UTC strings, serialized as
/// `createdAt` / `updatedAt`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Template {
    pub id: String,
    pub name: String,
    pub content: String,
    pub created_at: String,
    pub updated_at: String,
}

/// Read the stored list. Never fails; see [`Storage::read`].
pub fn load(storage: &Storage) -> Vec<Template> {
    storage.read(FILE)
}

/// Persist the whole list atomically.
pub fn save(storage: &Storage, templates: &[Template]) -> Result<(), AppError> {
    storage.write(FILE, &templates)
}

/// Append a new template and return it.
pub fn create(
    templates: &mut Vec<Template>,
    name: String,
    content: String,
) -> Result<Template, AppError> {
    let name = validate_name(name)?;
    validate_content(&content)?;

    let now = Utc::now().to_rfc3339();
    let template = Template {
        id: Uuid::new_v4().to_string(),
        name,
        content,
        created_at: now.clone(),
        updated_at: now,
    };
    templates.push(template.clone());

    Ok(template)
}

/// Replace the name and content of an existing template.
pub fn update(
    templates: &mut [Template],
    id: &str,
    name: String,
    content: String,
) -> Result<Template, AppError> {
    let name = validate_name(name)?;
    validate_content(&content)?;

    let template = templates
        .iter_mut()
        .find(|t| t.id == id)
        .ok_or_else(|| AppError::NotFound(format!("no template with id {id}")))?;

    template.name = name;
    template.content = content;
    template.updated_at = Utc::now().to_rfc3339();

    Ok(template.clone())
}

/// Remove a template by id.
pub fn delete(templates: &mut Vec<Template>, id: &str) -> Result<(), AppError> {
    let before = templates.len();
    templates.retain(|t| t.id != id);

    if templates.len() == before {
        return Err(AppError::NotFound(format!("no template with id {id}")));
    }
    Ok(())
}

fn validate_name(name: String) -> Result<String, AppError> {
    let name = name.trim().to_string();

    if name.is_empty() {
        return Err(AppError::Invalid("template name cannot be empty".into()));
    }
    if name.chars().count() > MAX_NAME_CHARS {
        return Err(AppError::Invalid(format!(
            "template name is too long (max {MAX_NAME_CHARS} characters)"
        )));
    }
    Ok(name)
}

fn validate_content(content: &str) -> Result<(), AppError> {
    if content.chars().count() > MAX_CONTENT_CHARS {
        return Err(AppError::Invalid(format!(
            "template content is too long (max {MAX_CONTENT_CHARS} characters)"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::Storage;
    use std::path::Path;

    fn storage_in(dir: &Path) -> Storage {
        Storage::resolve(vec![("appData", dir.to_path_buf())])
    }

    #[test]
    fn create_assigns_an_id_and_timestamps() {
        let mut templates = Vec::new();
        let created = create(&mut templates, "  Login  ".into(), "root\n".into()).expect("create");

        assert_eq!(created.name, "Login", "name should be trimmed");
        assert_eq!(created.content, "root\n");
        assert!(!created.id.is_empty());
        assert_eq!(created.created_at, created.updated_at);
        assert!(chrono::DateTime::parse_from_rfc3339(&created.created_at).is_ok());
        assert_eq!(templates.len(), 1);
    }

    #[test]
    fn create_rejects_a_blank_name() {
        let mut templates = Vec::new();
        let err = create(&mut templates, "   ".into(), "x".into()).expect_err("must reject");

        assert!(err.to_string().contains("cannot be empty"));
        assert!(templates.is_empty());
    }

    #[test]
    fn create_rejects_oversized_content() {
        let mut templates = Vec::new();
        let huge = "a".repeat(MAX_CONTENT_CHARS + 1);
        let err = create(&mut templates, "big".into(), huge).expect_err("must reject");

        assert!(err.to_string().contains("too long"));
        assert!(templates.is_empty());
    }

    #[test]
    fn update_changes_content_and_bumps_updated_at() {
        let mut templates = Vec::new();
        let created = create(&mut templates, "one".into(), "a".into()).expect("create");

        // RFC 3339 has sub-second precision, but sleep a hair so the comparison
        // is meaningful on coarse clocks.
        std::thread::sleep(std::time::Duration::from_millis(5));
        let updated =
            update(&mut templates, &created.id, "two".into(), "b".into()).expect("update");

        assert_eq!(updated.id, created.id);
        assert_eq!(updated.name, "two");
        assert_eq!(updated.content, "b");
        assert_eq!(updated.created_at, created.created_at);
        assert!(updated.updated_at >= created.updated_at);
        assert_eq!(templates[0], updated);
    }

    #[test]
    fn update_and_delete_report_a_missing_id() {
        let mut templates = Vec::new();

        let err = update(&mut templates, "nope", "n".into(), "c".into()).expect_err("must fail");
        assert!(err.to_string().contains("no template with id nope"));

        let err = delete(&mut templates, "nope").expect_err("must fail");
        assert!(err.to_string().contains("no template with id nope"));
    }

    #[test]
    fn delete_removes_only_the_named_template() {
        let mut templates = Vec::new();
        let first = create(&mut templates, "one".into(), "a".into()).expect("create");
        let second = create(&mut templates, "two".into(), "b".into()).expect("create");

        delete(&mut templates, &first.id).expect("delete");

        assert_eq!(templates.len(), 1);
        assert_eq!(templates[0].id, second.id);
    }

    #[test]
    fn crud_round_trips_through_storage() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let storage = storage_in(tmp.path());

        assert!(load(&storage).is_empty());

        let mut templates = Vec::new();
        let created =
            create(&mut templates, "Login".into(), "root\npass\n".into()).expect("create");
        save(&storage, &templates).expect("save");
        assert_eq!(load(&storage), templates);

        let mut templates = load(&storage);
        update(
            &mut templates,
            &created.id,
            "Login v2".into(),
            "admin\n".into(),
        )
        .expect("update");
        save(&storage, &templates).expect("save");

        let reloaded = load(&storage);
        assert_eq!(reloaded.len(), 1);
        assert_eq!(reloaded[0].name, "Login v2");
        assert_eq!(reloaded[0].content, "admin\n");

        let mut templates = reloaded;
        delete(&mut templates, &created.id).expect("delete");
        save(&storage, &templates).expect("save");
        assert!(load(&storage).is_empty());
    }

    #[test]
    fn corrupt_templates_file_recovers_to_an_empty_list() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let storage = storage_in(tmp.path());

        std::fs::write(tmp.path().join("templates.json"), b"[ {broken").expect("write");

        assert!(load(&storage).is_empty());
        assert!(tmp.path().join("templates.json.bak").exists());
    }

    #[test]
    fn serializes_timestamps_as_camel_case() {
        let mut templates = Vec::new();
        let created = create(&mut templates, "n".into(), "c".into()).expect("create");
        let json = serde_json::to_string(&created).expect("serialize");

        assert!(json.contains("\"createdAt\""));
        assert!(json.contains("\"updatedAt\""));
        assert!(!json.contains("created_at"));
    }
}
