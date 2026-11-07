//! Her own templates (slice 51): versioned letter and Bericht templates from the template builder.
//!
//! Each save is a new version (`1`, `2`, …) of the same key; older versions stay in the table,
//! inactive, so a letter written with one keeps its sections and headings
//! ([`Store::template_at`]). Deleting hides all versions from the pickers; existing letters
//! still open and export.

use rusqlite::{params, OptionalExtension};
use serde_json::json;

use super::{new_id, now, Store, ACTOR};
use crate::audit::{self, Entry};
use crate::builder;
use crate::templates::{Category, Layout, Template};
use crate::{Error, Result};

impl Store {
    /// The template a report was written with: for her own templates that exact version (also
    /// after later edits or deleting); built-in templates follow the shipped definition.
    pub fn template_at(&self, key: &str, version: &str) -> Result<Template> {
        if builder::is_own(key) {
            let def: Option<String> = self
                .conn
                .query_row("SELECT definition FROM template WHERE key = ?1 AND version = ?2", [key, version], |r| r.get(0))
                .optional()?;
            if let Some(def) = def {
                return Ok(serde_json::from_str(&def)?);
            }
        }
        self.template(key)
    }

    /// The newest version of every template, deleted own ones included: for titles and types of
    /// documents already written (lists, the board).
    pub fn known_templates(&self) -> Result<Vec<Template>> {
        let mut stmt = self.conn.prepare(
            "SELECT definition FROM template t
             WHERE created_at = (SELECT MAX(created_at) FROM template WHERE key = t.key) ORDER BY key",
        )?;
        let defs = stmt.query_map([], |r| r.get::<_, String>(0))?;
        defs.map(|d| Ok(serde_json::from_str(&d?)?)).collect()
    }

    /// Whether `key` is offered for new documents (a deleted own template isn't).
    pub fn template_offered(&self, key: &str) -> Result<bool> {
        Ok(self.conn.query_row("SELECT EXISTS (SELECT 1 FROM template WHERE key = ?1 AND active = 1)", [key], |r| r.get(0))?)
    }

    /// Saves her template as the next version of its key. Only letter-layout Briefe and Berichte
    /// with an own key; the validator's rules (Diagnosen hers only) apply as to built-in packs.
    pub fn save_own_template(&self, def: Template) -> Result<Template> {
        if !builder::is_own(&def.key) {
            return Err(Error::Invalid("Mitgelieferte Vorlagen lassen sich nicht ändern, nur als eigene Vorlage anpassen".into()));
        }
        if def.layout != Layout::Letter || !matches!(def.category, Category::Bericht | Category::Brief) {
            return Err(Error::Invalid("Eigene Vorlagen sind Briefe oder Berichte im Briefformat".into()));
        }
        let mut def = def;
        def.title = def.title.trim().to_string();
        if def.title.is_empty() {
            return Err(Error::Invalid("Die Vorlage braucht einen Namen".into()));
        }
        for s in &mut def.sections {
            s.heading = s.heading.trim().to_string();
            s.hints.retain(|h| !h.text.trim().is_empty());
            s.asks.retain(|a| !a.question.trim().is_empty());
            for a in &mut s.asks {
                a.chips.retain(|c| !c.trim().is_empty());
            }
            if s.starter.as_deref().is_some_and(|x| x.trim().is_empty()) {
                s.starter = None;
            }
        }
        if def.subject.as_deref().is_some_and(|x| x.trim().is_empty()) {
            def.subject = None;
        }
        if def.closing.as_deref().is_some_and(|x| x.trim().is_empty()) {
            def.closing = None;
        }
        if def.description.as_deref().is_some_and(|x| x.trim().is_empty()) {
            def.description = None;
        }
        def.refs.retain(|r| !r.label.trim().is_empty());
        def.report_kinds.clear();
        def.validate()?;
        let last: Option<i64> =
            self.conn.query_row("SELECT MAX(CAST(version AS INTEGER)) FROM template WHERE key = ?1", [&def.key], |r| r.get(0))?;
        def.version = (last.unwrap_or(0) + 1).to_string();
        let tx = self.conn.unchecked_transaction()?;
        tx.execute("UPDATE template SET active = 0 WHERE key = ?1", [&def.key])?;
        tx.execute(
            "INSERT INTO template (id, key, version, origin, definition, active, created_at) VALUES (?1, ?2, ?3, 'user', ?4, 1, ?5)",
            params![new_id(), def.key, def.version, serde_json::to_string(&def)?, now()],
        )?;
        audit::append(
            &tx,
            Entry {
                actor: ACTOR,
                action: "template.save",
                entity_type: "template",
                entity_id: Some(&def.key),
                meta: json!({ "version": def.version, "sections": def.sections.len() }),
            },
        )?;
        tx.commit()?;
        Ok(def)
    }

    /// Hides her template from the pickers. Letters written with it keep working.
    pub fn delete_own_template(&self, key: &str) -> Result<()> {
        if !builder::is_own(key) {
            return Err(Error::Invalid("Mitgelieferte Vorlagen lassen sich nicht löschen".into()));
        }
        let tx = self.conn.unchecked_transaction()?;
        if tx.execute("UPDATE template SET active = 0 WHERE key = ?1 AND active = 1", [key])? == 0 {
            return Err(Error::NotFound(format!("template {key}")));
        }
        audit::append(&tx, Entry { actor: ACTOR, action: "template.delete", entity_type: "template", entity_id: Some(key), meta: json!({}) })?;
        tx.commit()?;
        Ok(())
    }

    /// How many documents were written with template `key` (shown before deleting it).
    pub fn template_uses(&self, key: &str) -> Result<i64> {
        Ok(self.conn.query_row("SELECT COUNT(*) FROM report WHERE template_key = ?1", [key], |r| r.get(0))?)
    }
}

#[cfg(test)]
mod tests {
    use crate::builder;
    use crate::crypto::MasterKey;
    use crate::store::Store;

    fn store() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let s = Store::open(&dir.path().join("t.db"), &MasterKey::generate().unwrap()).unwrap();
        (dir, s)
    }

    #[test]
    fn own_templates_are_versioned_and_hidden_on_delete() {
        let (_d, s) = store();
        let drv = s.template("drv-befund").unwrap();
        assert!(s.save_own_template(drv.clone()).is_err(), "built-in keys stay");
        let mut own = builder::copy_of(&drv).unwrap();
        let v1 = s.save_own_template(own.clone()).unwrap();
        assert_eq!(v1.version, "1");
        assert!(s.list_templates().unwrap().iter().any(|t| t.key == own.key));
        own.sections.retain(|x| x.key != "somatik");
        own.title = "  DRV kurz ".into();
        let v2 = s.save_own_template(own.clone()).unwrap();
        assert_eq!((v2.version.as_str(), v2.title.as_str()), ("2", "DRV kurz"));
        assert_eq!(s.list_templates().unwrap().iter().filter(|t| t.key == own.key).count(), 1, "one version offered");
        assert_eq!(s.template(&own.key).unwrap().version, "2");
        assert!(s.template_at(&own.key, "1").unwrap().sections.iter().any(|x| x.key == "somatik"), "old letters keep their sections");
        assert_eq!(s.template_at("drv-befund", "1999").unwrap().key, "drv-befund", "built-in: the shipped one");
        // MDR boundary as for built-in packs.
        let mut bad = own.clone();
        bad.sections.iter_mut().find(|x| x.field == crate::templates::FieldKind::Diagnoses).unwrap().ai_allowed = true;
        assert!(s.save_own_template(bad).is_err());
        let mut ptv3 = own.clone();
        ptv3.category = crate::templates::Category::Ptv3;
        assert!(s.save_own_template(ptv3).is_err());
        assert!(s.template_offered(&own.key).unwrap());
        s.delete_own_template(&own.key).unwrap();
        assert!(!s.template_offered(&own.key).unwrap());
        assert!(!s.list_templates().unwrap().iter().any(|t| t.key == own.key));
        assert_eq!(s.template_at(&own.key, "2").unwrap().title, "DRV kurz", "still opens");
        assert_eq!(s.template(&own.key).unwrap().title, "DRV kurz", "by key too");
        assert_eq!(s.known_templates().unwrap().iter().filter(|t| t.key == own.key).count(), 1, "still known for its letters");
        assert!(s.delete_own_template(&own.key).is_err());
        assert!(s.delete_own_template("drv-befund").is_err());
        let log: String = s.conn().query_row("SELECT group_concat(meta_json, ' ') FROM audit_event WHERE action LIKE 'template.%'", [], |r| r.get(0)).unwrap();
        assert!(!log.contains("DRV"), "no titles in the audit log: {log}");
    }
}
