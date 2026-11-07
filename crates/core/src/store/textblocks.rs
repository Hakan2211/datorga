//! Textbausteine (slice 48): her library of reusable paragraphs, with the built-in set.

use rusqlite::{params, OptionalExtension};
use serde_json::json;

use super::{new_id, now, parse_col, Store, ACTOR};
use crate::audit::{self, Entry};
use crate::letter::FillContext;
use crate::textblock::{self, TextBlock, TextBlockInput};
use crate::{Error, Result};

const COLS: &str = "id, title, body, template_keys_json, section_key, origin, use_count, created_at, updated_at";
/// Built-in blocks she deleted (ids): not seeded again.
const HIDDEN_KEY: &str = "textblock.hidden_builtins";

fn row_to_block(r: &rusqlite::Row<'_>) -> rusqlite::Result<TextBlock> {
    let keys: String = r.get(3)?;
    Ok(TextBlock {
        id: r.get(0)?,
        title: r.get(1)?,
        body: r.get(2)?,
        template_keys: serde_json::from_str(&keys).unwrap_or_default(),
        section_key: r.get(4)?,
        origin: parse_col(r, 5)?,
        use_count: r.get(6)?,
        created_at: r.get(7)?,
        updated_at: r.get(8)?,
    })
}

impl Store {
    fn hidden_builtins(&self) -> Result<Vec<String>> {
        Ok(self.setting(HIDDEN_KEY)?.and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default())
    }

    /// Adds missing built-in blocks and updates unedited ones to the compiled-in wording. Blocks
    /// she edited (origin `user`) or deleted are left alone.
    pub(super) fn seed_builtin_textblocks(&self) -> Result<()> {
        let hidden = self.hidden_builtins()?;
        let ts = now();
        for b in textblock::builtin()? {
            if hidden.contains(&b.id) {
                continue;
            }
            let input = TextBlockInput { title: b.title, body: b.body, template_keys: b.templates, section_key: b.section }.clean()?;
            self.conn.execute(
                "INSERT INTO text_block (id, title, body, template_keys_json, section_key, origin, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, 'builtin', ?6, ?6)
                 ON CONFLICT (id) DO UPDATE SET title = excluded.title, body = excluded.body,
                     template_keys_json = excluded.template_keys_json, section_key = excluded.section_key
                 WHERE text_block.origin = 'builtin'
                   AND (text_block.title, text_block.body, text_block.template_keys_json, IFNULL(text_block.section_key, ''))
                       != (excluded.title, excluded.body, excluded.template_keys_json, IFNULL(excluded.section_key, ''))",
                params![b.id, input.title, input.body, serde_json::to_string(&input.template_keys)?, input.section_key, ts],
            )?;
        }
        Ok(())
    }

    /// Most used first. `template_key` keeps those fitting that letter type; `query` matches
    /// title and text.
    pub fn list_textblocks(&self, template_key: Option<&str>, query: Option<&str>) -> Result<Vec<TextBlock>> {
        let like = format!("%{}%", query.unwrap_or("").trim());
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {COLS} FROM text_block WHERE title || ' ' || body LIKE ?1
             ORDER BY use_count DESC, title COLLATE NOCASE"
        ))?;
        let rows = stmt.query_map([like], row_to_block)?;
        let all: Vec<TextBlock> = rows.collect::<std::result::Result<_, _>>()?;
        Ok(match template_key {
            Some(k) => all.into_iter().filter(|b| b.fits(k)).collect(),
            None => all,
        })
    }

    pub fn textblock(&self, id: &str) -> Result<TextBlock> {
        self.conn
            .query_row(&format!("SELECT {COLS} FROM text_block WHERE id = ?1"), [id], row_to_block)
            .optional()?
            .ok_or_else(|| Error::NotFound(format!("text block {id}")))
    }

    pub fn create_textblock(&self, input: TextBlockInput) -> Result<TextBlock> {
        let input = input.clean()?;
        let id = new_id();
        let ts = now();
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "INSERT INTO text_block (id, title, body, template_keys_json, section_key, origin, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, 'user', ?6, ?6)",
            params![id, input.title, input.body, serde_json::to_string(&input.template_keys)?, input.section_key, ts],
        )?;
        audit::append(&tx, Entry { actor: ACTOR, action: "textblock.create", entity_type: "text_block", entity_id: Some(&id), meta: json!({}) })?;
        tx.commit()?;
        self.textblock(&id)
    }

    /// An edited built-in block becomes hers: later versions of the app don't change it.
    pub fn update_textblock(&self, id: &str, input: TextBlockInput) -> Result<TextBlock> {
        let input = input.clean()?;
        let tx = self.conn.unchecked_transaction()?;
        let n = tx.execute(
            "UPDATE text_block SET title = ?2, body = ?3, template_keys_json = ?4, section_key = ?5, origin = 'user',
                 updated_at = ?6 WHERE id = ?1",
            params![id, input.title, input.body, serde_json::to_string(&input.template_keys)?, input.section_key, now()],
        )?;
        if n == 0 {
            return Err(Error::NotFound(format!("text block {id}")));
        }
        audit::append(&tx, Entry { actor: ACTOR, action: "textblock.update", entity_type: "text_block", entity_id: Some(id), meta: json!({}) })?;
        tx.commit()?;
        self.textblock(id)
    }

    /// A deleted built-in block stays hidden until "Mitgelieferte wiederherstellen".
    pub fn delete_textblock(&self, id: &str) -> Result<()> {
        let builtin = textblock::builtin()?.iter().any(|b| b.id == id);
        let tx = self.conn.unchecked_transaction()?;
        if tx.execute("DELETE FROM text_block WHERE id = ?1", [id])? == 0 {
            return Err(Error::NotFound(format!("text block {id}")));
        }
        audit::append(&tx, Entry { actor: ACTOR, action: "textblock.delete", entity_type: "text_block", entity_id: Some(id), meta: json!({}) })?;
        tx.commit()?;
        if builtin {
            let mut hidden = self.hidden_builtins()?;
            if !hidden.iter().any(|h| h == id) {
                hidden.push(id.to_string());
                self.set_setting(HIDDEN_KEY, &json!(hidden))?;
            }
        }
        Ok(())
    }

    /// Brings deleted built-in blocks back. Returns how many came back.
    pub fn restore_builtin_textblocks(&self) -> Result<usize> {
        let before: i64 = self.conn.query_row("SELECT count(*) FROM text_block WHERE id LIKE 'builtin-%'", [], |r| r.get(0))?;
        self.set_setting(HIDDEN_KEY, &json!([]))?;
        self.seed_builtin_textblocks()?;
        let after: i64 = self.conn.query_row("SELECT count(*) FROM text_block WHERE id LIKE 'builtin-%'", [], |r| r.get(0))?;
        Ok((after - before) as usize)
    }

    /// The block's HTML for this letter, placeholders filled from its patient and Behandlungsfall.
    /// Counts the use (the menu's order); the audit holds ids only.
    pub fn insert_textblock(&self, id: &str, report_id: &str) -> Result<String> {
        let block = self.textblock(id)?;
        let report = self.report(report_id)?;
        if report.signed_at.is_some() {
            return Err(Error::Invalid("Der Brief ist unterschrieben und kann nicht mehr geändert werden".into()));
        }
        let l = self.letter(report_id)?;
        let patient = self.patient(&l.patient_id)?;
        let episode = l.episode_id.as_deref().map(|e| self.episode(e)).transpose()?;
        let html = textblock::fill_html(&block.body, &FillContext { patient: &patient, episode: episode.as_ref() });
        let tx = self.conn.unchecked_transaction()?;
        tx.execute("UPDATE text_block SET use_count = use_count + 1 WHERE id = ?1", [id])?;
        audit::append(&tx, Entry {
            actor: ACTOR,
            action: "textblock.insert",
            entity_type: "report",
            entity_id: Some(report_id),
            meta: json!({ "text_block_id": id, "origin": block.origin.as_str() }),
        })?;
        tx.commit()?;
        Ok(html)
    }

    /// The blocks "Mit KI schreiben" may send for this letter type (see [`textblock::for_request`]).
    pub fn textblocks_for(&self, template_key: &str) -> Result<Vec<TextBlock>> {
        self.list_textblocks(Some(template_key), None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audit;
    use crate::domain::{AgeGroup, EpisodeInput, Gender, Method, PatientInput, Setting};
    use crate::letter::LetterInput;
    use crate::textblock::Origin;

    fn letter_fixture(s: &Store) -> String {
        let p = s
            .create_patient(PatientInput {
                first_name: "Anna".into(),
                last_name: "Beispiel".into(),
                birth_date: Some("1985-04-03".into()),
                gender: Some(Gender::W),
                ..Default::default()
            })
            .unwrap();
        s.create_episode(EpisodeInput { patient_id: p.id.clone(), method: Method::Vt, age_group: AgeGroup::Adult, setting: Setting::Einzel })
            .unwrap();
        s.create_letter(LetterInput { patient_id: p.id, template_key: "bescheinigung".into(), episode_id: None, recipient_id: None })
            .unwrap()
            .report_id
    }

    fn block(title: &str, body: &str, keys: &[&str]) -> TextBlockInput {
        TextBlockInput { title: title.into(), body: body.into(), template_keys: keys.iter().map(|k| k.to_string()).collect(), section_key: None }
    }

    #[test]
    fn builtins_are_seeded_once_and_hers_stay_hers() {
        let s = Store::open_in_memory().unwrap();
        let n = textblock::builtin().unwrap().len();
        let all = s.list_textblocks(None, None).unwrap();
        assert_eq!(all.len(), n);
        assert!(all.iter().all(|b| b.origin == Origin::Builtin));

        // Edit one: it becomes hers and a re-seed leaves it.
        let id = "builtin-schluss";
        s.update_textblock(id, block("Schluss", "<p>Bei Fragen gern melden.</p>", &[])).unwrap();
        s.seed_builtin_textblocks().unwrap();
        let b = s.textblock(id).unwrap();
        assert_eq!(b.origin, Origin::User);
        assert_eq!(b.body, "<p>Bei Fragen gern melden.</p>");

        // An unedited one that drifted is brought back to the compiled-in wording.
        s.conn.execute("UPDATE text_block SET body = '<p>alt</p>' WHERE id = 'builtin-belastbarkeit'", []).unwrap();
        s.seed_builtin_textblocks().unwrap();
        assert_ne!(s.textblock("builtin-belastbarkeit").unwrap().body, "<p>alt</p>");

        // Deleted builtins stay away until restored.
        s.delete_textblock("builtin-teilhabe").unwrap();
        s.seed_builtin_textblocks().unwrap();
        assert!(s.textblock("builtin-teilhabe").is_err());
        assert_eq!(s.restore_builtin_textblocks().unwrap(), 1);
        assert!(s.textblock("builtin-teilhabe").is_ok());
    }

    #[test]
    fn crud_filter_and_validation() {
        let s = Store::open_in_memory().unwrap();
        assert!(s.create_textblock(block(" ", "<p>x</p>", &[])).is_err());
        assert!(s.create_textblock(block("Leer", "<p> </p>", &[])).is_err());
        let b = s.create_textblock(block("  Mein   Satz ", "<p>Nur für die DRV.</p>", &["drv-befund", "drv-befund"])).unwrap();
        assert_eq!(b.title, "Mein Satz");
        assert_eq!(b.template_keys, ["drv-befund"]);
        let drv = s.list_textblocks(Some("drv-befund"), Some("Nur für")).unwrap();
        assert_eq!(drv.len(), 1);
        assert!(s.list_textblocks(Some("arztbrief"), Some("Nur für")).unwrap().is_empty());
        s.delete_textblock(&b.id).unwrap();
        assert!(s.textblock(&b.id).is_err());
        // Her own deleted block isn't recorded as a hidden builtin.
        assert!(s.hidden_builtins().unwrap().is_empty());
    }

    #[test]
    fn insert_fills_counts_and_audits_without_text() {
        let s = Store::open_in_memory().unwrap();
        let letter_id = letter_fixture(&s);
        let b = s.create_textblock(block("Seit", "<p>{Frau/Herr} {Nachname} ist seit dem {Behandlung seit} bei mir. {Krankenkasse}</p>", &[])).unwrap();
        let html = s.insert_textblock(&b.id, &letter_id).unwrap();
        assert!(html.starts_with("<p>Frau "), "{html}");
        assert!(!html.contains("{Nachname}") && !html.contains("{Behandlung seit}"), "{html}");
        assert_eq!(s.textblock(&b.id).unwrap().use_count, 1);
        let entries = audit::list(s.conn(), 50).unwrap();
        let e = entries.iter().find(|e| e.action == "textblock.insert").unwrap();
        assert!(!format!("{e:?}").contains("Beispiel") && !format!("{e:?}").contains("seit dem"));
        assert_eq!(audit::verify(s.conn()).unwrap(), None);
        // Most used comes first.
        assert_eq!(s.list_textblocks(None, None).unwrap()[0].id, b.id);
    }
}
