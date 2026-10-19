//! Langues des clients et textes localisés (tables `language` et `localized_text` de la base monde).
//!
//! La langue d'un joueur vient, dans l'ordre : de l'indicateur envoyé par ArchRust (capacité
//! `language/1`, champ 16 du `HELLO` ou `SET_LANGUAGE`), du dernier choix enregistré sur son compte
//! (`account.language`), sinon de `World.Language`. Un client d'origine ne déclare rien : il reçoit la
//! langue de son compte ou celle du serveur, comme avant.
//!
//! Repli d'un texte, dans l'ordre : ligne `localized_text` dans la langue demandée, dans sa langue de
//! base (`fr-CA` → `fr`), traduction des titres de PNJ (`data/i18n/npc-titles-<langue>.json`, noms de PNJ
//! et de monstres seulement), ligne en anglais, texte d'origine du client.

use crate::db::{col, Databases};
use crate::world::npcs::Translator;
use crate::{info, warn};
use archcore_net::ext_text::{base_language, normalize_language};
use std::collections::HashMap;
use std::path::Path;
use std::sync::OnceLock;

pub use archcore_net::schema::ext_text_kind as kind;

/// Langue des textes d'origine du client (dossier `Game/ini`).
pub const ORIGINAL_LANGUAGE: &str = "en";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Language {
    pub code: String,
    pub name_en: String,
    pub native_name: String,
    pub tier: u8,
    pub client_folder: Option<String>,
}

/// Ce que les deux serveurs doivent connaître sans charger les textes : langue par défaut et langues
/// proposées. Installé au démarrage (`install`) pour le `HELLO_ACK` et le serveur de connexion.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Summary {
    pub default_language: String,
    /// Codes des langues proposées, dans l'ordre d'affichage.
    pub available: Vec<String>,
}

impl Summary {
    /// Langue que le serveur applique à un souhait : le code s'il est proposé, sinon sa langue de
    /// base, sinon la langue par défaut.
    pub fn pick(&self, wish: Option<&str>) -> String {
        if let Some(w) = wish.and_then(normalize_language) {
            if self.available.iter().any(|a| *a == w) {
                return w;
            }
            let b = base_language(&w);
            if let Some(a) = self.available.iter().find(|a| base_language(a) == b) {
                return a.clone();
            }
        }
        self.default_language.clone()
    }

    /// Champ 17 : `fr;en;de`.
    pub fn available_list(&self) -> String {
        self.available.join(";")
    }
}

static SUMMARY: OnceLock<Summary> = OnceLock::new();

/// Rend le résumé disponible aux deux serveurs (une fois par processus).
pub fn install(summary: Summary) {
    let _ = SUMMARY.set(summary);
}

/// Résumé installé, sinon « anglais seulement ».
pub fn summary() -> Summary {
    SUMMARY.get().cloned().unwrap_or_else(|| Summary { default_language: ORIGINAL_LANGUAGE.into(), available: vec![ORIGINAL_LANGUAGE.into()] })
}

/// Catalogue des textes localisés du serveur monde.
#[derive(Default)]
pub struct Catalog {
    pub summary: Summary,
    pub languages: Vec<Language>,
    /// sorte → identifiant → langue → texte.
    texts: HashMap<String, HashMap<u32, HashMap<String, String>>>,
    /// sorte → identifiant → texte d'origine (tables du client).
    originals: HashMap<String, HashMap<u32, String>>,
    /// Traduction des titres de PNJ par langue de base.
    translators: HashMap<String, Translator>,
}

impl Catalog {
    pub fn new(default_language: &str) -> Self {
        let default_language = normalize_language(default_language).unwrap_or_else(|| ORIGINAL_LANGUAGE.into());
        Catalog { summary: Summary { default_language, available: Vec::new() }, ..Default::default() }
    }

    /// Charge les langues, les textes et les textes d'origine. Une table absente (base pas encore mise à
    /// jour par `archcore db-init`) laisse le catalogue vide, avec un avertissement.
    pub async fn load(db: &Databases, root: &Path, default_language: &str) -> Self {
        let mut c = Catalog::new(default_language);
        match db.world.rows("SELECT code, name_en, native_name, tier, client_folder FROM language WHERE enabled <> 0 ORDER BY sort_order, code", ()).await {
            Ok(rows) => {
                for r in rows {
                    c.add_language(Language {
                        code: col(&r, "code"),
                        name_en: col(&r, "name_en"),
                        native_name: col(&r, "native_name"),
                        tier: col::<u8>(&r, "tier"),
                        client_folder: col::<Option<String>>(&r, "client_folder"),
                    });
                }
            }
            Err(e) => warn!("langues non chargées ({e:#}) : lancer `archcore db-init`"),
        }
        if c.languages.is_empty() {
            c.add_language(Language { code: ORIGINAL_LANGUAGE.into(), name_en: "English".into(), native_name: "English".into(), tier: 1, client_folder: Some("ini".into()) });
        }
        let mut n = 0usize;
        match db.world.rows("SELECT kind, ref_id, lang, text FROM localized_text", ()).await {
            Ok(rows) => {
                for r in rows {
                    c.add_text(&col::<String>(&r, "kind"), col(&r, "ref_id"), &col::<String>(&r, "lang"), col::<String>(&r, "text"));
                    n += 1;
                }
            }
            Err(e) => warn!("textes localisés non chargés ({e:#})"),
        }
        for (k, sql) in [
            (kind::NPC_NAME, "SELECT id AS ref_id, name FROM npc_spawn"),
            (kind::MONSTER_NAME, "SELECT DISTINCT tid AS ref_id, display_name AS name FROM monster_spawn"),
            (kind::ITEM_NAME, "SELECT tid AS ref_id, name FROM item_data"),
            (kind::QUEST_NAME, "SELECT tid AS ref_id, name FROM quest_template"),
            (kind::SKILL_NAME, "SELECT tid AS ref_id, name FROM skill_template"),
            (kind::REGION_NAME, "SELECT id AS ref_id, name FROM region_template"),
        ] {
            match db.world.rows(sql, ()).await {
                Ok(rows) => {
                    for r in rows {
                        c.add_original(k, col(&r, "ref_id"), col::<String>(&r, "name"));
                    }
                }
                Err(e) => warn!("textes d'origine {k} non chargés ({e:#})"),
            }
        }
        // Titres de PNJ traduits : un fichier par langue de base.
        for lang in c.languages.iter().map(|l| base_language(&l.code).to_string()).collect::<Vec<_>>() {
            if lang != ORIGINAL_LANGUAGE && root.join("data").join("i18n").join(format!("npc-titles-{lang}.json")).exists() {
                c.translators.insert(lang.clone(), Translator::load(root, &lang));
            }
        }
        info!(
            "langues : {} (défaut {}) ; {n} texte(s) localisé(s), titres de PNJ traduits en {}",
            c.summary.available_list(),
            c.summary.default_language,
            if c.translators.is_empty() { "—".to_string() } else { c.translators.keys().cloned().collect::<Vec<_>>().join(", ") }
        );
        c
    }

    pub fn add_language(&mut self, l: Language) {
        if let Some(code) = normalize_language(&l.code) {
            if !self.summary.available.contains(&code) {
                self.summary.available.push(code.clone());
            }
            self.languages.push(Language { code, ..l });
        }
    }

    pub fn add_text(&mut self, kind: &str, id: u32, lang: &str, text: String) {
        if let Some(lang) = normalize_language(lang) {
            self.texts.entry(kind.to_string()).or_default().entry(id).or_default().insert(lang, text);
        }
    }

    pub fn add_original(&mut self, kind: &str, id: u32, text: String) {
        self.originals.entry(kind.to_string()).or_default().insert(id, text);
    }

    #[cfg(test)]
    pub fn add_translator(&mut self, lang: &str, t: Translator) {
        self.translators.insert(base_language(lang).to_string(), t);
    }

    /// Langue appliquée à un souhait (voir `Summary::pick`).
    pub fn pick(&self, wish: Option<&str>) -> String {
        self.summary.pick(wish)
    }

    /// Texte d'origine du client, s'il est connu.
    pub fn original(&self, kind: &str, id: u32) -> Option<&str> {
        self.originals.get(kind)?.get(&id).map(String::as_str)
    }

    fn db_text(&self, kind: &str, id: u32, lang: &str) -> Option<&str> {
        self.texts.get(kind)?.get(&id)?.get(lang).map(String::as_str)
    }

    /// Texte dans la langue `lang`, avec repli (voir l'en-tête du module). `None` si rien n'est connu.
    pub fn text(&self, kind: &str, id: u32, lang: &str) -> Option<String> {
        let base = base_language(lang);
        if let Some(t) = self.db_text(kind, id, lang).or_else(|| (base != lang).then(|| self.db_text(kind, id, base)).flatten()) {
            return Some(t.to_string());
        }
        let original = self.original(kind, id);
        if matches!(kind, kind::NPC_NAME | kind::MONSTER_NAME) {
            if let (Some(t), Some(o)) = (self.translators.get(base), original) {
                let translated = t.translate(o);
                if translated != o {
                    return Some(translated);
                }
            }
        }
        if base != ORIGINAL_LANGUAGE {
            if let Some(t) = self.db_text(kind, id, ORIGINAL_LANGUAGE) {
                return Some(t.to_string());
            }
        }
        original.map(str::to_string)
    }

    /// Comme `text`, avec `fallback` (le texte que le serveur enverrait sans localisation) en dernier recours.
    pub fn resolve(&self, kind: &str, id: u32, lang: &str, fallback: &str) -> String {
        self.text(kind, id, lang).unwrap_or_else(|| fallback.to_string())
    }
}

/// Dernier souhait de langue du compte (`account.language`). Une colonne absente (base pas encore passée
/// par `archcore db-init`) vaut « aucun choix », avec un avertissement.
pub async fn account_language(db: &Databases, account_id: u32) -> Option<String> {
    match db.auth.first("SELECT language FROM account WHERE id = ?", (account_id,)).await {
        Ok(row) => row.and_then(|r| col::<Option<String>>(&r, "language")).and_then(|l| normalize_language(&l)),
        Err(e) => {
            warn!("langue du compte {account_id} non lue ({e:#}) : lancer `archcore db-init`");
            None
        }
    }
}

/// Enregistre le souhait de langue d'un compte (tel que demandé, normalisé : un serveur qui proposera
/// cette langue plus tard l'appliquera).
pub async fn save_account_language(db: &Databases, account_id: u32, language: &str) {
    if let Err(e) = db.auth.exec("UPDATE account SET language = ? WHERE id = ?", (language.to_string(), account_id)).await {
        warn!("langue du compte {account_id} non enregistrée ({e:#}) : lancer `archcore db-init`");
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    pub fn catalog() -> Catalog {
        let mut c = Catalog::new("fr");
        for code in ["en", "fr", "de", "pt-BR"] {
            c.add_language(Language { code: code.into(), name_en: code.into(), native_name: code.into(), tier: 1, client_folder: None });
        }
        c.add_original(kind::NPC_NAME, 3, "<Merchant> Darryl".into());
        c.add_original(kind::QUEST_NAME, 100, "First Steps".into());
        c.add_original(kind::ITEM_NAME, 190, "Short Sword".into());
        c.add_text(kind::QUEST_NAME, 100, "fr", "Premiers pas".into());
        c.add_text(kind::QUEST_NAME, 100, "de", "Erste Schritte".into());
        c.add_text(kind::ITEM_NAME, 190, "en", "Short sword (fixed)".into());
        c.add_text(kind::NPC_NAME, 3, "pt-BR", "<Mercador> Darryl".into());
        let mut t = Translator::default();
        t.add_title("Merchant", "Marchand");
        c.add_translator("fr", t);
        c
    }

    #[test]
    fn choix_de_la_langue() {
        let c = catalog();
        assert_eq!(c.summary.available_list(), "en;fr;de;pt-BR");
        assert_eq!(c.pick(Some("de")), "de");
        assert_eq!(c.pick(Some("FR-ca")), "fr", "langue de base");
        assert_eq!(c.pick(Some("pt")), "pt-BR", "première variante proposée");
        assert_eq!(c.pick(Some("xx")), "fr", "inconnue : défaut du serveur");
        assert_eq!(c.pick(Some("n'importe quoi")), "fr");
        assert_eq!(c.pick(None), "fr");
        assert_eq!(summary().pick(Some("fr")), "en", "sans installation : anglais seulement");
    }

    #[test]
    fn repli_des_textes() {
        let c = catalog();
        // Ligne dans la langue, puis langue de base.
        assert_eq!(c.text(kind::QUEST_NAME, 100, "fr").as_deref(), Some("Premiers pas"));
        assert_eq!(c.text(kind::QUEST_NAME, 100, "fr-CA").as_deref(), Some("Premiers pas"));
        assert_eq!(c.text(kind::QUEST_NAME, 100, "de").as_deref(), Some("Erste Schritte"));
        // Pas de ligne : texte d'origine.
        assert_eq!(c.text(kind::QUEST_NAME, 100, "pt-BR").as_deref(), Some("First Steps"));
        assert_eq!(c.text(kind::QUEST_NAME, 100, "en").as_deref(), Some("First Steps"));
        // Ligne « en » corrigée, préférée au texte d'origine pour les autres langues comme pour l'anglais.
        assert_eq!(c.text(kind::ITEM_NAME, 190, "de").as_deref(), Some("Short sword (fixed)"));
        assert_eq!(c.text(kind::ITEM_NAME, 190, "en").as_deref(), Some("Short sword (fixed)"));
        // Titres de PNJ : traduction du fichier JSON quand la base n'a rien, d'origine en anglais.
        assert_eq!(c.text(kind::NPC_NAME, 3, "fr").as_deref(), Some("<Marchand> Darryl"));
        assert_eq!(c.text(kind::NPC_NAME, 3, "en").as_deref(), Some("<Merchant> Darryl"));
        assert_eq!(c.text(kind::NPC_NAME, 3, "pt-BR").as_deref(), Some("<Mercador> Darryl"));
        assert_eq!(c.text(kind::NPC_NAME, 3, "de").as_deref(), Some("<Merchant> Darryl"));
        // Inconnu partout : repli de l'appelant.
        assert_eq!(c.text(kind::SKILL_NAME, 1, "fr"), None);
        assert_eq!(c.resolve(kind::SKILL_NAME, 1, "fr", "Normal Attack"), "Normal Attack");
    }
}
