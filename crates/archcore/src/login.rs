//! Serveur de connexion.
//!
//! Flux relevé dans le client ArchLord 64 bits :
//! ```text
//!   AGPMWORLD      op 0  → op 2 : liste des mondes « Nom=Groupe=0=Statut=Drapeau;… »
//!   AGPMLOGIN      op 0  HASHKEY     → clé de 8 caractères (chiffrement RC4 du compte/mot de passe)
//!                  op 1  USER_LOGIN  → op 1 (succès) ou op 15 LOGIN_RESULT (échec)
//!                  op 3  UNION_INFO  → op 3, puis op 4 par personnage, puis op 5
//!                  op 6  CHAR_LIST   → paquet AGPMCHARACTER par personnage, puis op 7
//!                  op 10 CHAR_CREATION_ENTER → personnages de base, puis op 10
//!                  op 11 CREATE_CHAR → op 11, paquet personnage, op 12
//!                  op 13 DELETE_CHAR → op 13
//!                  op 8  WORLD_ENTER → AGSMCHARMANAGER op 11 (clé d'auth) puis op 8 (adresse du monde)
//! ```

use crate::character::{character_packet, make_factors, CharView, CharacterRow, Templates};
use crate::config::Config;
use crate::db::{col, Databases};
use crate::net::{Connection, Service, SessionHandle};
use crate::{info, warn};
use archcore_net::rc4::capi_crypt;
use archcore_net::schema::{self, char_mgr_op, login_op, login_result, mini, ptype, world_op};
use archcore_net::{build_mini, decode_mini, Fields, Packet};
use async_trait::async_trait;
use rand::Rng;
use sha2::{Digest, Sha256};
use std::sync::{Arc, RwLock};

const MAX_CHARACTERS: usize = 3;
/// Race de la scène de création envoyée au client (1 = Humain ; le client n'accepte que 1 ou 2).
const CREATION_RACE: i64 = 1;
/// CID réservés aux modèles de l'écran de création (jamais ceux des vrais personnages).
const CREATION_CID_BASE: u32 = 0x7f00_0000;
const HASHKEY_ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";

#[derive(Clone, Debug)]
pub struct WorldEntry {
    pub name: String,
    pub address: String,
    pub status: u32,
    pub flag: u32,
    pub group_name: String,
}

pub struct LoginService {
    db: Databases,
    templates: Arc<Templates>,
    config: Arc<Config>,
    worlds: RwLock<Vec<WorldEntry>>,
}

pub fn sha256_hex(s: &[u8]) -> String {
    Sha256::digest(s).iter().map(|b| format!("{b:02x}")).collect()
}

fn make_hash_key() -> String {
    let mut rng = rand::thread_rng();
    (0..8).map(|_| HASHKEY_ALPHABET[rng.gen_range(0..HASHKEY_ALPHABET.len())] as char).collect()
}

impl LoginService {
    pub fn new(db: Databases, templates: Arc<Templates>, config: Arc<Config>) -> Self {
        LoginService { db, templates, config, worlds: RwLock::new(Vec::new()) }
    }

    pub async fn refresh_world_list(&self) -> anyhow::Result<()> {
        let rows = self
            .db
            .auth
            .rows(
                "SELECT w.id, w.name, w.address, w.status, w.flag, g.name AS group_name FROM world w JOIN server_group g ON g.id = w.group_id ORDER BY g.id, w.id",
                (),
            )
            .await?;
        let list: Vec<WorldEntry> = rows
            .iter()
            .map(|r| WorldEntry {
                name: col(r, "name"),
                address: col(r, "address"),
                status: col(r, "status"),
                flag: col(r, "flag"),
                group_name: col(r, "group_name"),
            })
            .collect();
        let desc: Vec<String> = list.iter().map(|w| format!("{}@{}", w.name, w.address)).collect();
        info!("[login] {} monde(s) : {}", list.len(), desc.join(", "));
        *self.worlds.write().unwrap() = list;
        Ok(())
    }

    fn world_list_string(&self) -> String {
        self.worlds.read().unwrap().iter().map(|w| format!("{}={}=0={}={};", w.name, w.group_name, w.status, w.flag)).collect()
    }

    fn world_address(&self, name: &str) -> Option<String> {
        let worlds = self.worlds.read().unwrap();
        worlds.iter().find(|w| w.name == name).or(worlds.first()).map(|w| w.address.clone())
    }
}

impl Service for Arc<LoginService> {
    fn handles(&self, t: u8) -> bool {
        t == ptype::WORLD || t == ptype::LOGIN || t == ptype::ARCHCORE_EXT
    }
    fn connect(&self, _session: &SessionHandle) -> Box<dyn Connection> {
        Box::new(LoginConn { svc: self.clone(), hash_key: None, account: None, characters: Vec::new(), world: String::new() })
    }
}

struct LoginConn {
    svc: Arc<LoginService>,
    hash_key: Option<String>,
    account: Option<(u32, String)>,
    characters: Vec<CharacterRow>,
    world: String,
}

fn login(map: impl FnOnce(Fields) -> Fields) -> Fields {
    map(Fields::new(&schema::LOGIN))
}

fn char_info(map: impl FnOnce(Fields) -> Fields) -> Vec<u8> {
    build_mini(&mini::LOGIN_CHAR_INFO, &map(Fields::new(&mini::LOGIN_CHAR_INFO)))
}

#[async_trait]
impl Connection for LoginConn {
    async fn on_packet(&mut self, s: &SessionHandle, p: Packet) -> anyhow::Result<()> {
        match p.header.ptype {
            ptype::WORLD => self.on_world(s, &p),
            ptype::ARCHCORE_EXT => self.on_ext(s, &p).await,
            _ => self.on_login(s, &p).await,
        }
    }
}

impl LoginConn {
    /// Extensions ArchRust sur la connexion de login (capacité `language/1`) : `SET_LANGUAGE` change le
    /// souhait de la connexion et, une fois authentifié, celui du compte. Les textes se demandent au monde.
    async fn on_ext(&mut self, s: &SessionHandle, p: &Packet) -> anyhow::Result<()> {
        use archcore_net::schema::{ext_op, ext_result};
        let request_id = p.int(1).unwrap_or(0);
        match p.int(0).unwrap_or(-1) {
            ext_op::SET_LANGUAGE => {
                let wish = crate::ext::language_field(p).filter(|_| s.has_capability(crate::ext::LANGUAGE));
                let Some(wish) = wish else {
                    s.send(&schema::ARCHCORE_EXT, crate::ext::result(request_id, ext_result::INVALID_REQUEST));
                    return Ok(());
                };
                s.set_language(Some(wish.clone()));
                let summary = crate::locale::summary();
                let applied = summary.pick(Some(&wish));
                info!("[login] #{} langue {wish} (appliquée : {applied})", s.id());
                s.send(&schema::ARCHCORE_EXT, crate::ext::language_ack(request_id, &applied, &summary.available_list()));
                if let Some((id, _)) = &self.account {
                    crate::locale::save_account_language(&self.svc.db, *id, &wish).await;
                }
            }
            ext_op::TEXT_REQUEST => s.send(&schema::ARCHCORE_EXT, crate::ext::result(request_id, ext_result::NOT_AVAILABLE)),
            op => warn!("[login] #{} opération d'extension {op} ignorée", s.id()),
        }
        Ok(())
    }

    fn on_world(&mut self, s: &SessionHandle, p: &Packet) -> anyhow::Result<()> {
        match p.int(0).unwrap_or(-1) {
            world_op::SERVER_LIST_REQ => {
                let mut list = archcore_net::text::encode(&self.svc.world_list_string());
                list.push(0);
                let ws = &schema::WORLD;
                s.send(ws, Fields::new(ws).with(0, world_op::SERVER_LIST_ANS).with(3, list));
            }
            world_op::SERVER_EXTRA => {}
            op => warn!("[login] #{} AGPMWORLD op {op} non géré", s.id()),
        }
        Ok(())
    }

    async fn on_login(&mut self, s: &SessionHandle, p: &Packet) -> anyhow::Result<()> {
        match p.int(0).unwrap_or(-1) {
            login_op::HASHKEY => {
                let key = make_hash_key();
                s.send(&schema::LOGIN, login(|f| f.with(0, login_op::HASHKEY).with(1, key.as_str())));
                self.hash_key = Some(key);
                Ok(())
            }
            login_op::USER_LOGIN => self.user_login(s, p).await,
            login_op::UNION_INFO => self.union_info(s, p),
            login_op::CHAR_LIST => self.char_list(s),
            login_op::CHAR_CREATION_ENTER => self.char_creation_enter(s, p),
            login_op::CREATE_CHAR => self.create_char(s, p).await,
            login_op::DELETE_CHAR => self.delete_char(s, p).await,
            login_op::WORLD_ENTER => self.world_enter(s, p).await,
            op => {
                warn!("[login] #{} AGPMLOGIN op {op} non géré", s.id());
                Ok(())
            }
        }
    }

    fn send_login_result(&self, s: &SessionHandle, result: i64) {
        let info = char_info(|f| f);
        s.send(&schema::LOGIN, login(|f| f.with(0, login_op::LOGIN_RESULT).with(8, info).with(10, result)));
    }

    async fn user_login(&mut self, s: &SessionHandle, p: &Packet) -> anyhow::Result<()> {
        let Some(hash_key) = self.hash_key.clone() else {
            warn!("[login] #{} login sans clé de hachage", s.id());
            self.send_login_result(s, login_result::GIBBERISH);
            return Ok(());
        };
        let acct_len = p.int(3).unwrap_or(0);
        let pw_len = p.int(5).unwrap_or(0);
        let acct_raw = p.bytes(2).unwrap_or_default().to_vec();
        let pw_raw = p.bytes(4).unwrap_or_default();
        if acct_len <= 0 || acct_len > 48 || pw_len <= 0 || pw_len > 64 {
            self.send_login_result(s, login_result::GIBBERISH);
            return Ok(());
        }
        let username = archcore_net::text::decode(&capi_crypt(hash_key.as_bytes(), &acct_raw[..(acct_len as usize).min(acct_raw.len())]));
        let password = capi_crypt(hash_key.as_bytes(), &pw_raw[..(pw_len as usize).min(pw_raw.len())]);
        info!("[login] #{} tentative de connexion : {username:?}", s.id());

        let row = self
            .svc
            .db
            .auth
            .first(
                "SELECT id, username, password_hash, (banned_until IS NOT NULL AND banned_until > NOW()) AS banned FROM account WHERE username = ?",
                (username.clone(),),
            )
            .await?;
        let ok = row.as_ref().is_some_and(|r| col::<String>(r, "password_hash").to_lowercase() == sha256_hex(&password));
        let Some(row) = row.filter(|_| ok) else {
            info!("[login] #{} identifiants refusés pour {username:?}", s.id());
            self.send_login_result(s, login_result::INCORRECT_USER);
            return Ok(());
        };
        if col::<i64>(&row, "banned") != 0 {
            self.send_login_result(s, login_result::TOO_MANY_ATTEMPTS);
            return Ok(());
        }
        let (id, name): (u32, String) = (col(&row, "id"), col(&row, "username"));
        self.svc.db.auth.exec("UPDATE account SET last_login = NOW(), last_ip = ? WHERE id = ?", (s.ip().to_string(), id)).await?;
        // Langue annoncée au HELLO par un client ArchRust : elle suit le compte (clients d'origine compris).
        if let Some(lang) = s.language() {
            crate::locale::save_account_language(&self.svc.db, id, &lang).await;
        }
        self.account = Some((id, name.clone()));
        self.load_characters().await?;
        info!("[login] #{} {name} authentifié ({} personnage(s))", s.id(), self.characters.len());
        s.send(&schema::LOGIN, login(|f| f.with(0, login_op::USER_LOGIN).with(2, acct_raw).with(14, 0).with(15, 0)));
        Ok(())
    }

    async fn load_characters(&mut self) -> anyhow::Result<()> {
        let Some((id, _)) = &self.account else { return Ok(()) };
        let rows = self
            .svc
            .db
            .characters
            .rows("SELECT * FROM characters WHERE account_id = ? AND deleted_at IS NULL ORDER BY slot", (*id,))
            .await?;
        self.characters = rows.iter().map(CharacterRow::from_row).collect();
        Ok(())
    }

    fn require_account(&self, s: &SessionHandle) -> bool {
        if self.account.is_none() {
            warn!("[login] #{} requête avant authentification", s.id());
            s.close();
            return false;
        }
        true
    }

    fn union_info(&mut self, s: &SessionHandle, p: &Packet) -> anyhow::Result<()> {
        if !self.require_account(s) {
            return Ok(());
        }
        self.world = p.text(7).unwrap_or_default();
        // Le client range ce champ comme race courante de l'écran de connexion : le bouton « Créer »
        // n'envoie la demande op 10 que pour la race 1 ou 2.
        s.send(&schema::LOGIN, login(|f| f.with(0, login_op::UNION_INFO).with(8, char_info(|i| i.with(4, CREATION_RACE)))));
        let count = self.characters.len();
        for ch in &self.characters {
            let info = char_info(|i| i.with(1, ch.name.as_str()).with(2, count).with(3, ch.slot));
            s.send(&schema::LOGIN, login(|f| f.with(0, login_op::CHAR_NAME).with(8, info)));
        }
        let user = self.account.as_ref().map(|a| a.1.clone()).unwrap_or_default();
        s.send(&schema::LOGIN, login(|f| f.with(0, login_op::CHAR_NAME_FINISH).with(2, user.as_str())));
        Ok(())
    }

    fn character_packet(&self, ch: &CharacterRow, status: i64, is_new: i64) -> anyhow::Result<Vec<u8>> {
        let tpl = self.svc.templates.get(ch.template_id).ok_or_else(|| anyhow::anyhow!("template {} inconnu pour {}", ch.template_id, ch.name))?;
        let mut view = CharView::from(ch);
        view.status = status;
        view.is_new = is_new;
        Ok(character_packet(&view, &make_factors(tpl, Some(ch)), 0))
    }

    fn char_list(&mut self, s: &SessionHandle) -> anyhow::Result<()> {
        if !self.require_account(s) {
            return Ok(());
        }
        for ch in &self.characters {
            s.send_raw(self.character_packet(ch, 0, 0)?);
        }
        let user = self.account.as_ref().map(|a| a.1.clone()).unwrap_or_default();
        s.send(&schema::LOGIN, login(|f| f.with(0, login_op::CHAR_LIST_FINISH).with(2, user.as_str()).with(6, 0)));
        Ok(())
    }

    fn char_creation_enter(&mut self, s: &SessionHandle, p: &Packet) -> anyhow::Result<()> {
        if !self.require_account(s) {
            return Ok(());
        }
        let mut fake_id = CREATION_CID_BASE;
        for tid in &self.svc.templates.base {
            let Some(tpl) = self.svc.templates.get(*tid) else {
                warn!("[login] template de base {tid} absent");
                continue;
            };
            let view = CharView { id: fake_id, template_id: *tid, name: tpl.name.clone(), status: 2, ..Default::default() };
            fake_id += 1;
            s.send_raw(character_packet(&view, &make_factors(tpl, None), 0));
        }
        let asked = p.bytes(8).and_then(|b| decode_mini(&mini::LOGIN_CHAR_INFO, b).ok()).and_then(|i| i.int(5));
        info!(
            "[login] #{} écran de création (race {}) : {} modèle(s) de base",
            s.id(),
            asked.map(|r| r.to_string()).unwrap_or("?".into()),
            self.svc.templates.base.len()
        );
        let race = asked.filter(|&r| r != 0).unwrap_or(CREATION_RACE);
        s.send(&schema::LOGIN, login(|f| f.with(0, login_op::CHAR_CREATION_ENTER).with(8, char_info(|i| i.with(2, 15).with(5, race)))));
        Ok(())
    }

    fn info_values(p: &Packet) -> Fields {
        p.bytes(8).and_then(|b| decode_mini(&mini::LOGIN_CHAR_INFO, b).ok()).unwrap_or_else(|| Fields::new(&mini::LOGIN_CHAR_INFO))
    }

    async fn create_char(&mut self, s: &SessionHandle, p: &Packet) -> anyhow::Result<()> {
        if !self.require_account(s) {
            return Ok(());
        }
        let info = Self::info_values(p);
        let tid = info.int(0).unwrap_or(0) as u32;
        let name = info.text(1).unwrap_or_default().trim().to_string();
        let hair = info.int(6).unwrap_or(0);
        let face = info.int(7).unwrap_or(0);
        info!("[login] #{} création : {name:?} tid={tid} hair={hair} face={face}", s.id());
        let tpl = self.svc.templates.get(tid).cloned();
        let Some(tpl) = tpl.filter(|_| self.characters.len() < MAX_CHARACTERS && !name.is_empty() && name.chars().count() <= 48) else {
            self.send_login_result(s, login_result::NAME_TAKEN);
            return Ok(());
        };
        if self.svc.db.characters.first("SELECT id FROM characters WHERE name = ?", (name.clone(),)).await?.is_some() {
            self.send_login_result(s, login_result::NAME_TAKEN);
            return Ok(());
        }
        let mut slot = 0;
        while self.characters.iter().any(|c| c.slot == slot) {
            slot += 1;
        }
        let starts = &self.svc.config.world.start_positions;
        let start = starts.get(&tpl.race.to_string()).or_else(|| starts.get("1")).cloned().unwrap_or_default();
        info!("[login] #{} {name} apparaîtra à {} (race {})", s.id(), start.village.as_deref().unwrap_or("?"), tpl.race);
        let account_id = self.account.as_ref().unwrap().0;
        let level = if tpl.level != 0 { tpl.level } else { 1 };
        // Points pleins au niveau de départ (croissance du modèle).
        let (hp, mp, sp) = tpl.max_points(level);
        let new_id = self
            .svc
            .db
            .characters
            .exec(
                "INSERT INTO characters (account_id, slot, name, template_id, level, hp, mp, sp, pos_x, pos_y, pos_z, hair, face) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?)",
                vec![
                    mysql_async::Value::from(account_id),
                    slot.into(),
                    name.clone().into(),
                    tid.into(),
                    level.into(),
                    hp.into(),
                    mp.into(),
                    sp.into(),
                    start.x.into(),
                    start.y.into(),
                    start.z.into(),
                    hair.into(),
                    face.into(),
                ],
            )
            .await? as u32;
        self.load_characters().await?;
        let Some(ch) = self.characters.iter().find(|c| c.id == new_id).cloned() else {
            anyhow::bail!("personnage {name} introuvable après création");
        };
        let count = self.characters.len();
        s.send(&schema::LOGIN, login(|f| f.with(0, login_op::CREATE_CHAR).with(8, char_info(|i| i.with(1, name.as_str()).with(2, count).with(3, slot)))));
        s.send_raw(self.character_packet(&ch, 1, 1)?);
        s.send(&schema::LOGIN, login(|f| f.with(0, login_op::CREATE_CHAR_FINISH).with(6, ch.id)));
        Ok(())
    }

    async fn delete_char(&mut self, s: &SessionHandle, p: &Packet) -> anyhow::Result<()> {
        if !self.require_account(s) {
            return Ok(());
        }
        let name = Self::info_values(p).text(1).unwrap_or_default().trim().to_string();
        let Some(ch) = self.characters.iter().find(|c| c.name == name).cloned() else { return Ok(()) };
        self.svc
            .db
            .characters
            .exec("UPDATE characters SET deleted_at = NOW(), name = CONCAT(name, '#', id) WHERE id = ?", (ch.id,))
            .await?;
        self.load_characters().await?;
        info!("[login] #{} personnage supprimé : {name}", s.id());
        s.send(&schema::LOGIN, login(|f| f.with(0, login_op::DELETE_CHAR).with(8, char_info(|i| i.with(1, name.as_str())))));
        Ok(())
    }

    async fn world_enter(&mut self, s: &SessionHandle, p: &Packet) -> anyhow::Result<()> {
        if !self.require_account(s) {
            return Ok(());
        }
        let name = Self::info_values(p).text(1).unwrap_or_default().trim().to_string();
        let Some(ch) = self.characters.iter().find(|c| c.name == name).cloned() else {
            warn!("[login] #{} entrée monde pour un personnage inconnu {name:?}", s.id());
            return Ok(());
        };
        let Some(address) = self.svc.world_address(&self.world) else {
            crate::error!("[login] aucune adresse de monde pour {:?}", self.world);
            return Ok(());
        };
        let auth_key: i32 = rand::thread_rng().gen_range(1..i32::MAX);
        let (account_id, username) = self.account.clone().unwrap();
        self.svc.db.auth.exec("UPDATE account SET auth_token = ? WHERE id = ?", (auth_key, account_id)).await?;
        let full_name = format!("{}#{}", ch.name, self.world);
        let cm = &schema::CHAR_MANAGER;
        s.send(cm, Fields::new(cm).with(0, char_mgr_op::AUTH_KEY).with(3, full_name.as_str()).with(6, auth_key));
        let info = char_info(|i| i.with(0, ch.template_id).with(1, full_name.as_str()));
        let server_info = build_mini(&mini::LOGIN_SERVER_INFO, &Fields::new(&mini::LOGIN_SERVER_INFO).with(0, address.as_str()));
        // Le client : CHAR_INFO au champ 8, SERVER_INFO au champ 9, CID du personnage au champ 6.
        s.send(&schema::LOGIN, login(|f| f.with(0, login_op::WORLD_ENTER).with(6, ch.id).with(8, info).with(9, server_info)));
        info!("[login] #{} {username} → monde {} ({address}) avec {}", s.id(), self.world, ch.name);
        Ok(())
    }
}


#[cfg(test)]
mod tests {
    #[test]
    fn empreinte_sha256() {
        assert_eq!(super::sha256_hex(b"test"), "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08");
    }
}
