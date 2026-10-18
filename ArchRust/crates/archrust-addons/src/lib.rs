//! AddOns Lua façon World of Warcraft pour ArchRust.
//!
//! Un AddOn est un dossier `addons/<Nom>/` contenant un fichier `<Nom>.toc` qui décrit l'AddOn
//! (`## Title`, `## Version`, `## Interface`, `## SavedVariables`, `## Dependencies`…) puis la liste
//! des fichiers Lua à charger dans l'ordre. Chaque fichier reçoit, comme sous WoW, les arguments
//! `local addonName, ns = ...`.
//!
//! L'API exposée reprend les noms de WoW pour que les habitudes se transposent :
//! `CreateFrame`, `frame:SetScript("OnEvent", …)`, `frame:RegisterEvent("PLAYER_LOGIN")`,
//! `SlashCmdList`/`SLASH_X1`, `print`, `GetTime`, `UnitName`, `SendChatMessage`, `C_Timer.After`…
//! Les frames sont de simples nœuds décrits dans [`ui::UiTree`] ; c'est le moteur qui les dessine.
//!
//! Le bac à sable Lua n'a ni `io`, ni `os`, ni `package`, ni `dofile`/`loadfile` : un AddOn ne
//! peut toucher que le jeu et ses propres SavedVariables.

pub mod api;
pub mod frame;
pub mod manager;
pub mod manifest;
pub mod saved;
pub mod ui;

pub use manager::{AddonError, AddonManager, HostCommand, HostState, LoadedAddon};
pub use manifest::Manifest;
pub use ui::{Anchor, FrameKind, FrameNode, UiTree};

/// Version d'interface annoncée aux AddOns (`## Interface`).
pub const INTERFACE_VERSION: u32 = 10000;
