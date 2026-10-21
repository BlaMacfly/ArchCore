//! ArchCore : émulateur de serveur ArchLord et ses outils, en un seul exécutable.
//!
//! ```text
//!   archcore [login|world|all]                 serveur (par défaut : all)
//!   archcore config migrate <archcore.json>   convertit une ancienne configuration JSON en .conf
//!   archcore db-init [--account nom mdp [mj]]  crée les bases et applique sql/*.sql et sql/migrations/
//!   archcore db-check                          contrôle de cohérence des bases (lecture seule)
//!   archcore import [dossier]                  importe les tables déchiffrées du client, puis content/*.sql
//!   archcore content                           réapplique la couche de contenu content/*.sql
//!   archcore decrypt [source] [destination]    déchiffre Game/ini vers data/client-ini
//!   archcore i18n strings|coverage [Game]      mesure ce qui reste à traduire
//!   archcore i18n apply [--ascii] [--dry]      applique data/i18n/fr.json à Game/ini_fr
//!   archcore i18n db [langue]                  remplit localized_text (titres de PNJ, mémoire de traduction)
//!   archcore sim client|move|shop|addon|lang [args…] client simulé (tests sans le jeu)
//!   archcore re extract-schemas <fichiers…>    relevé des schémas de paquets (sorties REA)
//! ```
//! La configuration est lue dans `config/auth.conf` et `config/world.conf` (créés depuis leurs modèles
//! `.dist`, ou dans le dossier `ARCHCORE_CONF_DIR`), depuis le dossier courant (ou `ARCHCORE_ROOT`).

mod character;
mod check;
mod config;
mod db;
mod ext;
mod locale;
mod log;
mod login;
mod migrations;
mod net;
mod probe;
mod server;
mod tools;
mod world;

const USAGE: &str = "usage : archcore [login|world|all] | config | db-init | db-check | import | content | decrypt | i18n | sim | re (voir le wiki, page Outils)";

#[tokio::main]
async fn main() {
    log::init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = args.first().map(String::as_str).unwrap_or("all");
    let rest = args.get(1..).unwrap_or_default().to_vec();
    let result = match cmd {
        "login" | "world" | "all" => server::run(cmd).await,
        "db-init" => tools::db_init::run(&rest).await,
        "db-check" => tools::db_check::run(&rest).await,
        "config" => tools::conf::run(&rest),
        "import" => tools::import::run(&rest).await,
        "content" => tools::content::run(&rest).await,
        "decrypt" => tools::decrypt::run(&rest),
        "i18n" => tools::i18n::run(&rest).await,
        "sim" => tools::sim::run(&rest).await,
        "re" => tools::re::run(&rest),
        "-h" | "--help" | "help" => {
            println!("{USAGE}");
            Ok(())
        }
        other => Err(anyhow::anyhow!("commande inconnue {other:?}\n{USAGE}")),
    };
    if let Err(e) = result {
        error!("{e:#}");
        std::process::exit(1);
    }
}
