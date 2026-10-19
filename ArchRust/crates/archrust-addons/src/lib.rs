//! AddOns Lua façon World of Warcraft pour ArchRust.
//!
//! Un AddOn est un dossier `addons/<Nom>/` contenant un fichier `<Nom>.toc` qui décrit l'AddOn
//! (`## Title`, `## Version`, `## Interface`, `## SavedVariables`, `## Dependencies`…) puis la liste
//! des fichiers à charger dans l'ordre (`.lua`, et `.xml` pour leurs `<Script>`/`<Include>`, comme
//! les `embeds.xml` des bibliothèques). Chaque fichier reçoit, comme sous WoW, les arguments
//! `local addonName, ns = ...`.
//!
//! L'API reprend les noms de WoW pour que les habitudes et une bonne partie du code existant se
//! transposent : `CreateFrame` (frames = tables Lua extensibles), scripts et événements, `Unit*`,
//! `C_Timer`, `SlashCmdList`, `hooksecurefunc`, `GameTooltip`, `StaticPopup`, CVars… et les
//! bibliothèques LibStub, CallbackHandler, Ace3 (AceAddon, AceEvent, AceTimer, AceConsole, AceDB,
//! AceHook, AceLocale, AceSerializer), LibDataBroker et LibSharedMedia sont intégrées. L'espace
//! `ArchRust` expose ce qui est propre au client (caméra, rendu, couleurs, raccourcis…).
//! Voir `docs/ADDONS.md`.
//!
//! Le bac à sable n'a ni `io`, ni `os.execute`, ni `dofile`/`loadfile`, ni bytecode : un AddOn ne
//! touche qu'au jeu, à ses fichiers et à ses SavedVariables.

pub mod api;
pub mod frame;
pub mod libs;
pub mod lua_env;
pub mod manager;
pub mod manifest;
pub mod saved;
pub mod ui;

pub use manager::{
    AddonError, AddonManager, CameraInfo, EditEvent, HostCommand, HostState, ItemInfo,
    LoadedAddon, PointerInput, UnitInfo,
};
pub use manifest::Manifest;
pub use ui::{Anchor, FrameKind, FrameNode, TextureSource, UiTree};

/// Version d'interface annoncée aux AddOns (`## Interface`).
pub const INTERFACE_VERSION: u32 = 10000;
