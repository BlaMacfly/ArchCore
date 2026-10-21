//! Client réseau ArchLord sans affichage.
//!
//! Reproduit le flux du vrai client, validé paquet par paquet contre l'émulateur ArchCore :
//!
//! 1. [`Connection`] : TCP + poignée de main Blowfish, envoi/réception de paquets typés.
//! 2. [`login`] : liste des mondes, clé de hachage, compte, personnages, création, entrée.
//! 3. [`world`] : entrée en jeu, position de chargement, PNJ en vue, keepalive, événements.
//!
//! Le moteur graphique consomme les [`world::WorldEvent`] et envoie des [`world::Command`].

pub mod combat;
pub mod connection;
pub mod ext;
pub mod guild;
pub mod items;
pub mod login;
pub mod npc;
pub mod party;
pub mod skills;
pub mod teleport;
pub mod world;

pub use connection::{Connection, NetError};
pub use ext::{ExtMessage, Extensions};
pub use login::{CharacterSummary, LoginClient, WorldTicket};
pub use world::{Command, WorldClient, WorldEvent};
