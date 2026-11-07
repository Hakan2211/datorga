//! Style exemplars (slice 49): her redacted old letters and Berichte per document type.

use rusqlite::{params, OptionalExtension};
use serde_json::json;

use super::{new_id, now, Store, ACTOR};
use crate::audit::{self, Entry};
use crate::style::{StyleExemplar, StyleExemplarInput};
use crate::{Error, Result};

const COLS: &str = "id, template_key, title, redacted_text, active, source_sha256, created_at, updated_at";

fn row_to_exemplar(r: &rusqlite::Row<'_>) -> rusqlite::Result<StyleExemplar> {
    Ok(StyleExemplar {
        id: r.get(0)?,
        template_key: r.get(1)?,
        title: r.get(2)?,
        text: r.get(3)?,
        active: r.get(4)?,
        source_sha256: r.get(5)?,
        created_at: r.get(6)?,
        updated_at: r.get(7)?,
    })
}

impl Store {
    /// Newest first; `template_key` keeps one document type.
    pub fn list_style_exemplars(&self, template_key: Option<&str>) -> Result<Vec<StyleExemplar>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {COLS} FROM style_exemplar WHERE ?1 IS NULL OR template_key = ?1 ORDER BY created_at DESC, id"
        ))?;
        let rows = stmt.query_map([template_key], row_to_exemplar)?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    pub fn style_exemplar(&self, id: &str) -> Result<StyleExemplar> {
        self.conn
            .query_row(&format!("SELECT {COLS} FROM style_exemplar WHERE id = ?1"), [id], row_to_exemplar)
            .optional()?
            .ok_or_else(|| Error::NotFound(format!("style exemplar {id}")))
    }

    /// An exemplar from the same file of the same type, if she uploaded it before.
    pub fn style_exemplar_by_source(&self, template_key: &str, sha256: &str) -> Result<Option<StyleExemplar>> {
        Ok(self
            .conn
            .query_row(
                &format!("SELECT {COLS} FROM style_exemplar WHERE template_key = ?1 AND source_sha256 = ?2"),
                [template_key, sha256],
                row_to_exemplar,
            )
            .optional()?)
    }

    pub fn create_style_exemplar(&self, input: StyleExemplarInput) -> Result<StyleExemplar> {
        let input = input.clean()?;
        if !self.template_offered(&input.template_key)? {
            return Err(Error::Invalid(format!("Unbekannte Art des Schreibens: {}", input.template_key)));
        }
        let (id, ts) = (new_id(), now());
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "INSERT INTO style_exemplar (id, template_key, title, redacted_text, source_sha256, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
            params![id, input.template_key, input.title, input.text, input.source_sha256, ts],
        )?;
        audit::append(
            &tx,
            Entry {
                actor: ACTOR,
                action: "style.create",
                entity_type: "style_exemplar",
                entity_id: Some(&id),
                meta: json!({ "template": input.template_key, "chars": input.text.chars().count() }),
            },
        )?;
        tx.commit()?;
        self.style_exemplar(&id)
    }

    /// Title, type, text (still redacted: she edits the stored text) and whether drafts use it.
    pub fn update_style_exemplar(&self, id: &str, input: StyleExemplarInput, active: bool) -> Result<StyleExemplar> {
        let old = self.style_exemplar(id)?;
        let input = StyleExemplarInput { source_sha256: old.source_sha256.clone(), ..input }.clean()?;
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "UPDATE style_exemplar SET template_key = ?2, title = ?3, redacted_text = ?4, active = ?5, updated_at = ?6 WHERE id = ?1",
            params![id, input.template_key, input.title, input.text, active, now()],
        )?;
        audit::append(
            &tx,
            Entry {
                actor: ACTOR,
                action: "style.update",
                entity_type: "style_exemplar",
                entity_id: Some(id),
                meta: json!({ "template": input.template_key, "chars": input.text.chars().count(), "active": active }),
            },
        )?;
        tx.commit()?;
        self.style_exemplar(id)
    }

    pub fn delete_style_exemplar(&self, id: &str) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        if tx.execute("DELETE FROM style_exemplar WHERE id = ?1", [id])? == 0 {
            return Err(Error::NotFound(format!("style exemplar {id}")));
        }
        audit::append(&tx, Entry { actor: ACTOR, action: "style.delete", entity_type: "style_exemplar", entity_id: Some(id), meta: json!({}) })?;
        tx.commit()?;
        Ok(())
    }

    /// A document was read for a request or an exemplar (method and size only: no file name,
    /// it may hold a patient's name).
    pub fn log_document_read(&self, purpose: &str, imported: &crate::ingest::Imported) -> Result<()> {
        audit::append(
            &self.conn,
            Entry {
                actor: ACTOR,
                action: "document.read",
                entity_type: "document",
                entity_id: None,
                meta: json!({ "purpose": purpose, "method": imported.method, "pages": imported.pages, "chars": imported.text.chars().count() }),
            },
        )
    }

    /// The text of a signed Bericht or letter, to offer it as a style exemplar: each section's
    /// heading and text (the redaction review comes next).
    pub fn report_style_text(&self, report_id: &str) -> Result<(String, String)> {
        let report = self.report(report_id)?;
        if report.signed_at.is_none() {
            return Err(Error::Invalid("Nur unterschriebene Berichte und Briefe können Stilbeispiele werden".into()));
        }
        let template = self.template_at(&report.template_key, &report.template_version)?;
        let mut out = String::new();
        // Section order as stored (a letter's request sections have their own headings).
        for sec in &report.sections {
            let def = template.sections.iter().find(|d| d.key == sec.key);
            let field = def.map(|d| d.field).unwrap_or(crate::templates::FieldKind::Prose);
            // Diagnoses are hers to fill in each time: no style in them.
            if field == crate::templates::FieldKind::Diagnoses {
                continue;
            }
            let text = crate::pii::section_text(field, &sec.body);
            if text.trim().is_empty() {
                continue;
            }
            let heading = sec.heading.clone().or_else(|| def.map(|d| d.heading.clone())).unwrap_or_default();
            if !heading.trim().is_empty() {
                out.push_str(heading.trim());
                out.push('\n');
            }
            out.push_str(text.trim());
            out.push_str("\n\n");
        }
        Ok((report.template_key.clone(), out.trim().to_string()))
    }

    /// What goes along with a draft of `template_key`: (title, excerpt) of the newest active ones.
    pub fn style_for(&self, template_key: &str) -> Result<Vec<(String, String)>> {
        Ok(crate::style::for_request(&self.list_style_exemplars(Some(template_key))?, template_key))
    }
}

#[cfg(test)]
mod tests {
    use crate::crypto::MasterKey;
    use crate::store::Store;
    use crate::style::StyleExemplarInput;

    #[test]
    fn exemplars_round_trip_and_audit_without_text() {
        let dir = tempfile::tempdir().unwrap();
        let s = Store::open(&dir.path().join("t.db"), &MasterKey::generate().unwrap()).unwrap();
        let input = |key: &str| StyleExemplarInput {
            template_key: key.into(),
            title: "DRV 2023".into(),
            text: "Die Patientin befindet sich seit [Datum] in Behandlung. ".repeat(8),
            source_sha256: Some("a".repeat(64)),
        };
        assert!(s.create_style_exemplar(input("gibt-es-nicht")).is_err());
        let e = s.create_style_exemplar(input("drv-befund")).unwrap();
        assert!(e.active);
        assert_eq!(s.style_exemplar_by_source("drv-befund", &"a".repeat(64)).unwrap().unwrap().id, e.id);
        assert_eq!(s.style_for("drv-befund").unwrap().len(), 1);
        assert!(s.style_for("arztbrief").unwrap().is_empty());
        let e = s.update_style_exemplar(&e.id, StyleExemplarInput { title: "DRV alt".into(), ..input("drv-befund") }, false).unwrap();
        assert_eq!((e.title.as_str(), e.active), ("DRV alt", false));
        assert_eq!(e.source_sha256, Some("a".repeat(64)), "the source stays");
        assert!(s.style_for("drv-befund").unwrap().is_empty(), "paused");
        let log: String = s.conn().query_row("SELECT group_concat(meta_json, ' ') FROM audit_event WHERE action LIKE 'style.%'", [], |r| r.get(0)).unwrap();
        assert!(!log.contains("Behandlung") && !log.contains("DRV"), "{log}");
        s.delete_style_exemplar(&e.id).unwrap();
        assert!(s.list_style_exemplars(None).unwrap().is_empty());
        assert!(s.delete_style_exemplar(&e.id).is_err());
    }
}
