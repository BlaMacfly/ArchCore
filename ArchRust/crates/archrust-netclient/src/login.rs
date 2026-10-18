//! Serveur de connexion (port 11002) : mondes, compte, personnages, entrée dans le monde.
//!
//! Séquence relevée dans le client et validée avec ArchCore :
//!
//! ```text
//! WORLD  op 0  → WORLD op 2 : bloc "Nom=Groupe=0=Statut=Drapeau;…"
//! LOGIN  op 0  → LOGIN op 0 : clé de hachage (8 caractères, champ 1)
//! LOGIN  op 1  (compte et mot de passe chiffrés RC4 avec la clé) → LOGIN op 1, ou op 15 + résultat
//! LOGIN  op 3  (nom du monde, champ 7) → op 3, puis op 4 par personnage (mini CHAR_INFO), op 5
//! LOGIN  op 6  → un paquet CHARACTER par personnage, puis LOGIN op 7
//! LOGIN  op 10 → personnages de base de l'écran de création, puis LOGIN op 10
//! LOGIN  op 11 (mini CHAR_INFO : TID, nom, cheveux, visage) → op 11, CHARACTER, op 12 (CID champ 6)
//! LOGIN  op 8  (mini CHAR_INFO : nom) → CHAR_MANAGER op 11 (clé d'auth champ 6), LOGIN op 8 (adresse champ 9)
//! ```

use crate::connection::{Connection, Incoming, NetError};
use archrust_crypto::capi_crypt;
use archrust_protocol::packet::{build_mini, decode_mini, sparse, Value};
use archrust_protocol::schema::{char_mgr_op, login_op, main, mini, packet_type, world_op};

/// Monde annoncé par le serveur de connexion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorldInfo {
    pub name: String,
    pub group: String,
    pub status: i32,
    pub flag: i32,
}

/// Personnage tel que listé à l'écran de sélection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CharacterSummary {
    pub cid: i32,
    pub tid: i32,
    pub name: String,
    pub level: i32,
}

/// Billet d'entrée remis par le serveur de connexion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorldTicket {
    pub character: String,
    pub world: String,
    pub auth_key: i32,
    pub host: String,
    pub port: u16,
}

pub struct LoginClient {
    pub conn: Connection,
    pub worlds: Vec<WorldInfo>,
    pub hash_key: String,
    pub names: Vec<String>,
}

fn text(v: Option<&Value>) -> String {
    v.and_then(Value::as_text).unwrap_or_default()
}

/// Analyse la liste des mondes `Nom=Groupe=0=Statut=Drapeau;…`.
pub fn parse_world_list(s: &str) -> Vec<WorldInfo> {
    s.split(';')
        .filter(|e| !e.trim().is_empty())
        .filter_map(|e| {
            let f: Vec<&str> = e.split('=').collect();
            Some(WorldInfo {
                name: f.first()?.to_string(),
                group: f.get(1).unwrap_or(&"").to_string(),
                status: f.get(3).and_then(|s| s.parse().ok()).unwrap_or(0),
                flag: f.get(4).and_then(|s| s.parse().ok()).unwrap_or(0),
            })
        })
        .collect()
}

/// Niveau d'un paquet CHARACTER : mini facteurs (champ 7) → mini statut (index 1) → champ 7.
pub fn character_summary(p: &Incoming) -> Option<CharacterSummary> {
    let cid = p.get(1)?.as_i64()? as i32;
    let tid = p.get(2)?.as_i64()? as i32;
    let name = p.get(3).and_then(Value::as_text).unwrap_or_default();
    let level = p
        .get(7)
        .and_then(Value::as_bytes)
        .and_then(|f| decode_mini(mini::CHAR_FACTORS, f).ok())
        .and_then(|(_, v)| v.get(1).cloned().flatten())
        .and_then(|st| st.as_bytes().map(|b| b.to_vec()))
        .and_then(|st| decode_mini(mini::FACTOR_STATUS, &st).ok())
        .and_then(|(_, v)| v.get(7).cloned().flatten())
        .and_then(|v| v.as_i64())
        .unwrap_or(0) as i32;
    Some(CharacterSummary {
        cid,
        tid,
        name,
        level,
    })
}

impl LoginClient {
    /// Se connecte, fait la poignée de main, récupère la liste des mondes et la clé de hachage.
    pub async fn connect(host: &str, port: u16) -> Result<Self, NetError> {
        let mut conn = Connection::connect("login", host, port).await?;
        conn.handshake().await?;

        conn.send(
            main::WORLD,
            &sparse(main::WORLD, &[(0, Value::I8(world_op::SERVER_LIST_REQ))]),
        )
        .await?;
        let ans = conn
            .expect(packet_type::WORLD, Some(world_op::SERVER_LIST_ANS as i64))
            .await?;
        let worlds = parse_world_list(&text(ans.get(3)));
        log::info!(
            "[login] mondes : {:?}",
            worlds.iter().map(|w| &w.name).collect::<Vec<_>>()
        );

        conn.send(
            main::LOGIN,
            &sparse(main::LOGIN, &[(0, Value::I8(login_op::HASHKEY))]),
        )
        .await?;
        let hk = conn
            .expect(packet_type::LOGIN, Some(login_op::HASHKEY as i64))
            .await?;
        let hash_key = text(hk.get(1));
        if hash_key.is_empty() {
            return Err(NetError::Server("clé de hachage vide".into()));
        }
        Ok(Self {
            conn,
            worlds,
            hash_key,
            names: Vec::new(),
        })
    }

    /// Authentifie le compte. Rend l'erreur du serveur (code de résultat) en cas de refus.
    pub async fn authenticate(&mut self, user: &str, password: &str) -> Result<(), NetError> {
        let enc_user = capi_crypt(&self.hash_key, user.as_bytes());
        let enc_pass = capi_crypt(&self.hash_key, password.as_bytes());
        let values = sparse(
            main::LOGIN,
            &[
                (0, Value::I8(login_op::USER_LOGIN)),
                (2, Value::Str(enc_user.clone())),
                (3, Value::I8(enc_user.len() as i8)),
                (4, Value::Str(enc_pass.clone())),
                (5, Value::I8(enc_pass.len() as i8)),
            ],
        );
        self.conn.send(main::LOGIN, &values).await?;
        let ans = self.conn.expect(packet_type::LOGIN, None).await?;
        if ans.op() == Some(login_op::LOGIN_RESULT as i64) {
            let code = ans.get(10).and_then(Value::as_i64).unwrap_or(-1);
            return Err(NetError::Server(format!(
                "connexion refusée (résultat {code})"
            )));
        }
        log::info!("[login] compte authentifié (op {:?})", ans.op());
        Ok(())
    }

    /// Choisit un monde et récupère les noms de personnages (op 3/4/5).
    pub async fn select_world(&mut self, world: &str) -> Result<Vec<String>, NetError> {
        self.conn
            .send(
                main::LOGIN,
                &sparse(
                    main::LOGIN,
                    &[(0, Value::I8(login_op::UNION_INFO)), (7, Value::str(world))],
                ),
            )
            .await?;
        self.conn
            .expect(packet_type::LOGIN, Some(login_op::UNION_INFO as i64))
            .await?;
        let mut names = Vec::new();
        loop {
            let p = self.conn.expect(packet_type::LOGIN, None).await?;
            match p.op().map(|o| o as i8) {
                Some(login_op::CHAR_NAME_FINISH) => break,
                Some(login_op::CHAR_NAME) => {
                    if let Some(info) = p.get(8).and_then(Value::as_bytes) {
                        let (_, v) = decode_mini(mini::LOGIN_CHAR_INFO, info)?;
                        names.push(text(v.get(1).and_then(|x| x.as_ref())));
                    }
                }
                _ => {}
            }
        }
        self.names = names.clone();
        Ok(names)
    }

    /// Liste détaillée des personnages (op 6 → paquets CHARACTER → op 7).
    pub async fn list_characters(&mut self) -> Result<Vec<CharacterSummary>, NetError> {
        self.conn
            .send(
                main::LOGIN,
                &sparse(main::LOGIN, &[(0, Value::I8(login_op::CHAR_LIST))]),
            )
            .await?;
        let mut chars = Vec::new();
        loop {
            let p = self
                .conn
                .expect_where("liste des personnages", |p| {
                    p.packet_type == packet_type::CHARACTER
                        || p.is(packet_type::LOGIN, Some(login_op::CHAR_LIST_FINISH as i64))
                })
                .await?;
            if p.packet_type == packet_type::LOGIN {
                break;
            }
            if let Some(c) = character_summary(&p) {
                chars.push(c);
            }
        }
        Ok(chars)
    }

    /// Entre dans l'écran de création (op 10) et rend le nombre de modèles de base reçus.
    pub async fn enter_creation(&mut self) -> Result<usize, NetError> {
        self.conn
            .send(
                main::LOGIN,
                &sparse(
                    main::LOGIN,
                    &[(0, Value::I8(login_op::CHAR_CREATION_ENTER))],
                ),
            )
            .await?;
        let mut bases = 0;
        loop {
            let p = self
                .conn
                .expect_where("écran de création", |p| {
                    p.packet_type == packet_type::CHARACTER
                        || p.is(
                            packet_type::LOGIN,
                            Some(login_op::CHAR_CREATION_ENTER as i64),
                        )
                })
                .await?;
            if p.packet_type == packet_type::LOGIN {
                break;
            }
            bases += 1;
        }
        Ok(bases)
    }

    /// Crée un personnage (op 11) ; rend son CID.
    pub async fn create_character(
        &mut self,
        name: &str,
        tid: i32,
        hair: i32,
        face: i32,
    ) -> Result<i32, NetError> {
        let info = build_mini(
            mini::LOGIN_CHAR_INFO,
            &sparse(
                mini::LOGIN_CHAR_INFO,
                &[
                    (0, Value::I32(tid)),
                    (1, Value::str(name)),
                    (6, Value::I32(hair)),
                    (7, Value::I32(face)),
                ],
            ),
        )?;
        self.conn
            .send(
                main::LOGIN,
                &sparse(
                    main::LOGIN,
                    &[
                        (0, Value::I8(login_op::CREATE_CHAR)),
                        (8, Value::Packet(info)),
                    ],
                ),
            )
            .await?;
        let ans = self.conn.expect(packet_type::LOGIN, None).await?;
        if ans.op() == Some(login_op::LOGIN_RESULT as i64) {
            let code = ans.get(10).and_then(Value::as_i64).unwrap_or(-1);
            return Err(NetError::Server(format!(
                "création refusée (résultat {code})"
            )));
        }
        self.conn.expect(packet_type::CHARACTER, None).await?;
        let fin = self
            .conn
            .expect(
                packet_type::LOGIN,
                Some(login_op::CREATE_CHAR_FINISH as i64),
            )
            .await?;
        let cid = fin.get(6).and_then(Value::as_i64).unwrap_or(0) as i32;
        self.names.push(name.to_string());
        Ok(cid)
    }

    /// Demande l'entrée dans le monde avec un personnage ; rend le billet (clé d'auth + adresse).
    pub async fn enter_world(
        &mut self,
        character: &str,
        world: &str,
    ) -> Result<WorldTicket, NetError> {
        let info = build_mini(
            mini::LOGIN_CHAR_INFO,
            &sparse(mini::LOGIN_CHAR_INFO, &[(1, Value::str(character))]),
        )?;
        self.conn
            .send(
                main::LOGIN,
                &sparse(
                    main::LOGIN,
                    &[
                        (0, Value::I8(login_op::WORLD_ENTER)),
                        (8, Value::Packet(info)),
                    ],
                ),
            )
            .await?;
        let auth = self
            .conn
            .expect(
                packet_type::CHAR_MANAGER,
                Some(char_mgr_op::AUTH_KEY as i64),
            )
            .await?;
        let auth_key = auth.get(6).and_then(Value::as_i64).unwrap_or(0) as i32;
        let enter = self
            .conn
            .expect(packet_type::LOGIN, Some(login_op::WORLD_ENTER as i64))
            .await?;
        let addr_mini = enter
            .get(9)
            .and_then(Value::as_bytes)
            .ok_or_else(|| NetError::Server("adresse du monde absente".into()))?;
        let (_, v) = decode_mini(mini::LOGIN_SERVER_INFO, addr_mini)?;
        let address = text(v.first().and_then(|x| x.as_ref()));
        let (host, port) = address
            .rsplit_once(':')
            .and_then(|(h, p)| p.parse::<u16>().ok().map(|p| (h.to_string(), p)))
            .ok_or_else(|| NetError::Server(format!("adresse du monde invalide {address:?}")))?;
        log::info!("[login] billet pour {character}@{world} : clé {auth_key}, monde {host}:{port}");
        Ok(WorldTicket {
            character: character.to_string(),
            world: world.to_string(),
            auth_key,
            host,
            port,
        })
    }

    pub async fn close(self) {
        self.conn.close().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn world_list() {
        let w = parse_world_list("ArchCore=ArchCore=0=4=4;Autre=G=0=1=2;");
        assert_eq!(w.len(), 2);
        assert_eq!(w[0].name, "ArchCore");
        assert_eq!(w[1].status, 1);
    }
}
