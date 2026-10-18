//! Moteur du client ArchRust (x86-64, wgpu).
//!
//! * [`assets`] : packs du client, textures, modèles, tables, patchs HD.
//! * [`render`] : contexte wgpu, maillages, textures GPU, pipeline éclairé.
//! * [`camera`] : caméra orbitale (clic droit + molette) façon MMO.
//! * [`scene`] : conversion des clumps RenderWare en maillages GPU, entités du monde.
//! * [`gltf_model`] : modèles glTF/GLB des patchs (`[[models]]`) en maillages GPU.
//! * [`net`] : liaison avec `archrust-netclient` sur un fil tokio, événements/commandes.
//! * [`ui`] : interface egui (connexion, discussion, console) et rendu des frames Lua.
//! * [`app`] : boucle winit, états (connexion → personnages → monde).

pub mod app;
pub mod assets;
pub mod camera;
pub mod gltf_model;
pub mod net;
pub mod render;
pub mod scene;
pub mod ui;
pub mod world;

pub use app::{run, Config};
pub use assets::Assets;
