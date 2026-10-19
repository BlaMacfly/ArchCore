//! Charge les AddOns d'exemple du dépôt (`addons/`) et rejoue une petite session de jeu sans
//! moteur graphique : entrée en jeu, unités en vue, commandes, souris, sauvegarde.

use archrust_addons::{AddonManager, HostCommand, PointerInput, UnitInfo};
use std::path::PathBuf;

fn addons_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../addons")
}

fn unit(cid: i32, name: &str, hp: i64, x: f32) -> UnitInfo {
    UnitInfo {
        cid,
        tid: 300 + cid,
        name: name.into(),
        level: 1,
        hp: Some(hp),
        position: [x, 0.0, 1000.0],
        ..Default::default()
    }
}

#[test]
fn archace_and_helloarch_run() {
    let tmp = tempfile::tempdir().unwrap();
    let mut mgr = AddonManager::new(tmp.path().join("sv")).unwrap();
    let me = unit(1, "Testeur", 120, 0.0);
    mgr.set_host(|h| {
        h.player = Some(me.clone());
        h.screen = (1280.0, 720.0);
        // Caméra fictive : projection orthographique simple (x, y → écran).
        h.view_proj = [
            1.0 / 2000.0, 0.0, 0.0, 0.0, //
            0.0, 1.0 / 2000.0, 0.0, 0.0, //
            0.0, 0.0, 0.0, 0.0, //
            0.0, 0.0, 0.0, 1.0,
        ];
    });
    mgr.load_dir(&addons_dir()).unwrap();
    let list = mgr.addons();
    for a in &list {
        assert!(a.loaded, "{} : {:?}", a.manifest.name, a.error);
    }
    assert!(list.iter().any(|a| a.manifest.name == "ArchAce"));
    assert!(list.iter().any(|a| a.manifest.name == "HelloArch"));

    mgr.sync_world(
        Some(me.clone()),
        vec![unit(7, "Loup", 50, 300.0), unit(8, "Ours", 80, 900.0)],
        Some(vec![]),
    );
    mgr.fire_event("PLAYER_LOGIN", &[]);
    mgr.fire("PLAYER_ENTERING_WORLD", (true, false));
    assert!(mgr.run_slash("/archace demo"));
    for _ in 0..5 {
        mgr.update(0.3);
        mgr.layout(1280.0, 720.0, &|f| f.estimated_text_size());
    }
    assert!(mgr.errors().is_empty(), "{:?}", mgr.errors());

    // Démo : thème ambre (couleurs + ciel), cible la plus proche teintée, cadres remplis.
    assert_eq!(mgr.cvar("skyColor").as_deref(), Some("0.55 0.38 0.2"));
    let colors = mgr.take_ui_colors().expect("couleurs d'interface");
    assert!(colors.contains_key("windowFill"));
    assert_eq!(mgr.eval_string("UnitName('target')").unwrap(), "Loup");
    assert!(mgr.unit_tint(7).is_some());
    assert_eq!(mgr.eval_string("ArchAceTargetFrame.name:GetText()").unwrap(), "Loup");
    assert_eq!(
        mgr.eval_string("ArchAcePlayerFrame.health.text:GetText()").unwrap(),
        "120 / 120"
    );
    // Plaques de nom : deux textes visibles placés par WorldToScreen.
    let plates = mgr
        .eval_string(
            "(function() local n = 0 for _, r in ipairs({ArchAceNameplates:GetRegions()}) do if r:IsShown() then n = n + 1 end end return n end)()",
        )
        .unwrap();
    assert_eq!(plates, "2");

    // Dégâts sur la cible : UNIT_HEALTH → la barre suit.
    mgr.sync_world(
        Some(me.clone()),
        vec![unit(7, "Loup", 20, 300.0), unit(8, "Ours", 80, 900.0)],
        None,
    );
    assert_eq!(
        mgr.eval_string("ArchAceTargetFrame.health.text:GetText()").unwrap(),
        "20 / 50"
    );

    // Panneau d'options : clic sur la case « Plaques de nom ».
    assert!(mgr.run_slash("/archace options"));
    mgr.layout(1280.0, 720.0, &|f| f.estimated_text_size());
    assert_eq!(mgr.eval_string("tostring(ArchRustOptionsFrame:IsShown())").unwrap(), "true");
    let (x, y) = {
        let s = mgr.eval_string(
            "(function() local p = ArchAceOptions for _, c in ipairs({p:GetChildren()}) do if c:GetObjectType() == 'CheckButton' and c.Text:GetText() == 'Plaques de nom' then local x, y = c:GetCenter() return x .. ',' .. y end end end)()",
        )
        .unwrap();
        let (a, b) = s.split_once(',').unwrap();
        (a.parse::<f32>().unwrap(), b.parse::<f32>().unwrap())
    };
    for (pressed, released) in [(vec!["LeftButton"], vec![]), (vec![], vec!["LeftButton"])] {
        mgr.pointer(&PointerInput {
            pos: Some((x, y)),
            pressed: pressed.into_iter().map(String::from).collect(),
            released: released.into_iter().map(String::from).collect(),
            wheel: 0.0,
            native_over: false,
        });
    }
    assert_eq!(
        mgr.eval_string("tostring(ArchAce.db.profile.plates)").unwrap(),
        "false"
    );
    assert_eq!(mgr.eval_string("tostring(ArchAceNameplates:IsShown())").unwrap(), "false");

    // HelloArch : commande slash et SendChatMessage (accroché par ArchAce).
    mgr.drain_commands();
    assert!(mgr.run_slash("/hello monde"));
    let cmds = mgr.drain_commands();
    assert!(
        cmds.iter().any(|c| matches!(c, HostCommand::Chat { message, .. } if message.contains("monde"))),
        "{cmds:?}"
    );
    assert_eq!(mgr.eval_string("ArchAce.db.char.messagesSent").unwrap(), "1");

    // Raccourci clavier posé par un AddOn.
    assert!(mgr.run_slash("/archace bind F5"));
    mgr.run_script("ClearTarget()").unwrap();
    assert!(mgr.key("F5", true));
    assert_eq!(mgr.eval_string("UnitName('target')").unwrap(), "Loup");

    // Sauvegarde : profil AceDB sans les valeurs par défaut, compteur HelloArch.
    mgr.shutdown();
    let saved = std::fs::read_to_string(tmp.path().join("sv/ArchAce.lua")).unwrap();
    assert!(saved.contains("ArchAceDB = {"), "{saved}");
    assert!(saved.contains("plates = false"), "{saved}");
    assert!(saved.contains("theme = \"ambre\""), "{saved}");
    assert!(!saved.contains("fov = 55"), "valeur par défaut retirée : {saved}");
    let hello = std::fs::read_to_string(tmp.path().join("sv/HelloArch.lua")).unwrap();
    assert!(hello.contains("logins = 1"), "{hello}");
}
