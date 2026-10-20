//! Messages d'AddOn (extensions ArchRust `addon-msg/1`) entre deux états d'AddOns, relayés par un
//! « serveur » simulé : validation de `C_ChatInfo.SendAddonMessage`, préfixes enregistrés,
//! lissage du débit, `CHAT_MSG_ADDON`, refus (`ARCHCORE_ADDON_MESSAGE_FAILED`), AceComm en
//! plusieurs morceaux.

use archrust_addons::{AddonManager, ExtInfo, HostCommand, UnitInfo};

fn ext() -> ExtInfo {
    ExtInfo {
        software: "ArchCore 0.2.0".into(),
        major: 1,
        minor: 0,
        capabilities: [("addon-msg".to_string(), 1)].into_iter().collect(),
        max_payload: 255,
        rate_burst: 20,
        rate_per_sec: 10,
        language: None,
        languages: Vec::new(),
    }
}

fn client(name: &str, cid: i32) -> (tempfile::TempDir, AddonManager) {
    let tmp = tempfile::tempdir().unwrap();
    let mgr = AddonManager::new(tmp.path().join("sv")).unwrap();
    mgr.set_host(|h| {
        h.player = Some(UnitInfo {
            cid,
            name: name.into(),
            ..Default::default()
        })
    });
    (tmp, mgr)
}

/// Messages à envoyer : (préfixe, portée, cible, charge).
fn outgoing(mgr: &AddonManager) -> Vec<(u32, String, u8, Option<String>, Vec<u8>)> {
    mgr.drain_commands()
        .into_iter()
        .filter_map(|c| match c {
            HostCommand::AddonSend {
                request_id,
                prefix,
                channel,
                target,
                payload,
            } => Some((request_id, prefix, channel, target, payload)),
            _ => None,
        })
        .collect()
}

#[test]
fn send_receive_throttle_and_acecomm() {
    let (_ta, a) = client("Alice", 1);
    let (_tb, b) = client("Bruno", 2);

    // Sans extensions : rien ne part.
    assert_eq!(
        a.eval_string("C_ChatInfo.SendAddonMessage('Pfx', 'x', 'SAY')").unwrap(),
        "9",
        "GeneralError tant que le serveur n'a pas accordé addon-msg"
    );
    assert_eq!(a.eval_string("tostring(C_ArchCore.IsAvailable())").unwrap(), "false");

    for m in [&a, &b] {
        m.run_script(
            r#"
            Ready = nil
            local f = CreateFrame("Frame")
            f:RegisterEvent("ARCHCORE_EXTENSIONS_READY")
            f:RegisterEvent("CHAT_MSG_ADDON")
            f:RegisterEvent("ARCHCORE_ADDON_MESSAGE_FAILED")
            Got, Failed = {}, {}
            f:SetScript("OnEvent", function(_, e, ...)
              if e == "ARCHCORE_EXTENSIONS_READY" then Ready = table.concat({tostringall(...)}, " ")
              elseif e == "CHAT_MSG_ADDON" then
                local prefix, text, channel, sender, target = ...
                table.insert(Got, prefix .. "|" .. text .. "|" .. channel .. "|" .. sender .. "|" .. target)
              else
                local prefix, result, name = ...
                table.insert(Failed, prefix .. ":" .. name)
              end
            end)
        "#,
        )
        .unwrap();
        m.set_extensions(Some(ext()));
    }
    assert_eq!(a.eval_string("Ready").unwrap(), "ArchCore 0.2.0 1 0");
    assert_eq!(a.eval_string("C_ArchCore.GetLimits().maxPayload").unwrap(), "255");
    assert_eq!(a.eval_string("C_ArchCore.GetCapabilities()['addon-msg']").unwrap(), "1");

    // Validation côté client (codes Enum.SendAddonMessageResult).
    for (expr, want) in [
        ("C_ChatInfo.SendAddonMessage('', 'x', 'SAY')", "1"),
        ("C_ChatInfo.SendAddonMessage('a b', 'x', 'SAY')", "1"),
        ("C_ChatInfo.SendAddonMessage('AC.serveur', 'x', 'SAY')", "1"),
        ("C_ChatInfo.SendAddonMessage('Pfx', string.rep('x', 256), 'SAY')", "2"),
        ("C_ChatInfo.SendAddonMessage('Pfx', 'x', 'INSTANCE_CHAT')", "4"),
        ("C_ChatInfo.SendAddonMessage('Pfx', 'x', 'WHISPER')", "6"),
        ("C_ChatInfo.RegisterAddonMessagePrefix('Pfx')", "true"),
        ("select(2, C_ChatInfo.RegisterAddonMessagePrefix('Pfx'))", "1"),
        ("C_ChatInfo.RegisterAddonMessagePrefix('trop long pour un préfixe')", "false"),
        ("C_ChatInfo.IsAddonMessagePrefixRegistered('Pfx')", "true"),
        ("#C_ChatInfo.GetRegisteredAddonMessagePrefixes()", "1"),
    ] {
        assert_eq!(a.eval_string(expr).unwrap(), want, "{expr}");
    }
    assert!(outgoing(&a).is_empty(), "aucun message invalide ne part");

    // Murmure de A à B, charge binaire intacte.
    b.run_script("C_ChatInfo.RegisterAddonMessagePrefix('Pfx')").unwrap();
    assert_eq!(
        a.eval_string("C_ChatInfo.SendAddonMessage('Pfx', 'salut\\0\\255', 'WHISPER', 'Bruno')")
            .unwrap(),
        "0"
    );
    let out = outgoing(&a);
    assert_eq!(out.len(), 1);
    let (id, prefix, channel, target, payload) = out[0].clone();
    assert_eq!((prefix.as_str(), channel, target.as_deref()), ("Pfx", 1, Some("Bruno")));
    assert_eq!(payload, b"salut\0\xff");
    assert!(b.addon_message(&prefix, channel, target.as_deref(), "Alice", 1, &payload));
    assert_eq!(
        b.eval_string("tostring(Got[1] == 'Pfx|salut\\0\\255|WHISPER|Alice|Bruno')")
            .unwrap(),
        "true",
        "charge binaire intacte"
    );
    // Préfixe non enregistré : pas d'événement.
    assert!(!b.addon_message("Autre", 2, None, "Alice", 1, b"x"));

    // Refus du serveur : l'événement nomme le préfixe de la demande.
    a.addon_message_failed(id, 8);
    assert_eq!(a.eval_string("Failed[1]").unwrap(), "Pfx:TARGET_NO_EXTENSION");

    // Rafale de 40 messages : les 18 jetons restants (le murmure en a pris un) partent tout de suite,
    // le reste au rythme du serveur.
    a.run_script("for i = 1, 40 do assert(C_ChatInfo.SendAddonMessage('Pfx', 'm' .. i, 'SAY') == 0) end")
        .unwrap();
    assert_eq!(outgoing(&a).len(), 18);
    assert_eq!(a.eval_string("C_ArchCore.GetPendingMessages()").unwrap(), "22");
    a.update(1.0);
    assert_eq!(outgoing(&a).len(), 9);
    for _ in 0..3 {
        a.update(1.0);
    }
    assert_eq!(outgoing(&a).len(), 13);
    // File pleine (256) : AddonMessageThrottle.
    a.run_script(
        "local n = 0 \
         repeat n = n + 1 Over = C_ChatInfo.SendAddonMessage('Pfx', 'x', 'SAY') \
         until Over ~= 0 or n > 1000",
    )
    .unwrap();
    assert_eq!(a.eval_string("Over").unwrap(), "3");
    assert_eq!(a.eval_string("C_ArchCore.GetPendingMessages()").unwrap(), "256");
    // Vide la file de A avant la suite.
    for _ in 0..40 {
        a.update(1.0);
    }
    outgoing(&a);

    // AceComm : un message de 700 octets est découpé puis recomposé chez B.
    b.run_script(
        r#"
        local B = LibStub("AceAddon-3.0"):NewAddon("CommB", "AceComm-3.0")
        function B:OnCommReceived(prefix, message, distribution, sender)
          CommGot = prefix .. "|" .. #message .. "|" .. distribution .. "|" .. sender .. "|" .. message:sub(1, 3) .. message:sub(-3)
          CommLog = (CommLog or "") .. #message .. ";"
          if #message == 6 then ShortOk = (message == "\001court") end
        end
        B:RegisterComm("ArchComm")
        "#,
    )
    .unwrap();
    a.run_script(
        r#"
        local A = LibStub("AceAddon-3.0"):NewAddon("CommA", "AceComm-3.0")
        local big = "abc" .. string.rep("-", 694) .. "xyz"
        Progress = {}
        A:SendCommMessage("ArchComm", big, "WHISPER", "Bruno", "BULK", function(_, sent, total) table.insert(Progress, sent .. "/" .. total) end)
        A:SendCommMessage("ArchComm", "\001court", "WHISPER", "Bruno")
        "#,
    )
    .unwrap();
    let mut relayed = 0;
    for _ in 0..10 {
        a.update(0.1);
        for (_, prefix, channel, target, payload) in outgoing(&a) {
            assert!(payload.len() <= 255);
            b.addon_message(&prefix, channel, target.as_deref(), "Alice", 1, &payload);
            relayed += 1;
        }
    }
    assert_eq!(relayed, 4, "700 octets = 3 morceaux, plus le message court");
    assert!(a.errors().is_empty() && b.errors().is_empty(), "{:?} {:?}", a.errors(), b.errors());
    // Le message court (priorité NORMAL) passe avant le gros (BULK) ; son marqueur est échappé.
    assert_eq!(b.eval_string("CommLog").unwrap(), "6;700;");
    assert_eq!(b.eval_string("tostring(ShortOk)").unwrap(), "true");
    assert_eq!(
        b.eval_string("CommGot").unwrap(),
        "ArchComm|700|WHISPER|Alice|abcxyz"
    );
    assert_eq!(a.eval_string("table.concat(Progress, ',')").unwrap(), "254/700,508/700,700/700");
}

#[test]
fn acecomm_multipart_alone() {
    let (_ta, a) = client("Alice", 1);
    let (_tb, b) = client("Bruno", 2);
    a.set_extensions(Some(ext()));
    b.set_extensions(Some(ext()));
    b.run_script(
        r#"
        local B = LibStub("AceAddon-3.0"):NewAddon("CommB", "AceComm-3.0", "AceSerializer-3.0")
        B:RegisterComm("ArchComm", function(prefix, message, distribution, sender)
          local ok, t = B:Deserialize(message)
          Received = ok and (#t.list .. " " .. t.name .. " " .. sender .. " " .. distribution)
        end)
        "#,
    )
    .unwrap();
    a.run_script(
        r#"
        local A = LibStub("AceAddon-3.0"):NewAddon("CommA", "AceComm-3.0", "AceSerializer-3.0")
        local list = {}
        for i = 1, 200 do list[i] = i * 3 end
        A:SendCommMessage("ArchComm", A:Serialize({ name = "Élan", list = list }), "SAY")
        "#,
    )
    .unwrap();
    for _ in 0..10 {
        a.update(0.1);
        for (_, prefix, channel, target, payload) in outgoing(&a) {
            b.addon_message(&prefix, channel, target.as_deref(), "Alice", 1, &payload);
        }
    }
    assert!(b.errors().is_empty(), "{:?}", b.errors());
    assert_eq!(b.eval_string("Received").unwrap(), "200 Élan Alice SAY");
}
