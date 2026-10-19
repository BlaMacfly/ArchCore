//! ArchCore : émulateur de serveur ArchLord et ses outils, en un seul exécutable.
//!
//! ```text
//!   archcore [login|world|all]                 serveur (par défaut : all)
//!   archcore db-init [--account nom mdp [mj]]  crée les bases et applique sql/*.sql
//!   archcore import [dossier]                  importe les tables déchiffrées du client
//!   archcore decrypt [source] [destination]    déchiffre Game/ini vers data/client-ini
//!   archcore i18n strings|coverage [Game]      mesure ce qui reste à traduire
//!   archcore i18n apply [--ascii] [--dry]      applique data/i18n/fr.json à Game/ini_fr
//!   archcore sim client|move|shop|addon [args…] client simulé (tests sans le jeu)
//!   archcore re extract-schemas <fichiers…>    relevé des schémas de paquets (sorties REA)
//! ```
//! La configuration est lue dans `config/archcore.json` (ou `ARCHCORE_CONFIG`) depuis le dossier
//! courant (ou `ARCHCORE_ROOT`).

mod character;
mod config;
mod db;
mod ext;
mod log;
mod login;
mod net;
mod server;
mod tools;
mod world;

const USAGE: &str = "usage : archcore [login|world|all] | db-init | import | decrypt | i18n | sim | re (voir le wiki, page Outils)";

#[tokio::main]
async fn main() {
    log::init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = args.first().map(String::as_str).unwrap_or("all");
    let rest = args.get(1..).unwrap_or_default().to_vec();
    let result = match cmd {
        "login" | "world" | "all" => server::run(cmd).await,
        "db-init" => tools::db_init::run(&rest).await,
        "import" => tools::import::run(&rest).await,
        "decrypt" => tools::decrypt::run(&rest),
        "i18n" => tools::i18n::run(&rest),
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
