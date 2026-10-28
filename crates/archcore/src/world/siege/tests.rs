use super::*;
use super::war::allows;
use crate::world::monsters::{MonsterRegistry, SpawnPoint};
use archcore_net::{decode_mini, decode_packet, parse_header};
use chrono::NaiveDate;

fn day(y: i32, m: u32, d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, d).unwrap()
}

#[test]
fn calendrier_des_sieges() {
    // 2026-10-28 est un mercredi ; sans siège passé, base = samedi de la semaine de « aujourd'hui + 7 − 7 jours ».
    assert_eq!(base_day(None, day(2026, 10, 28), 7), day(2026, 10, 31));
    // Rensphere (30 jours) : samedi de la semaine du 2026-11-20.
    assert_eq!(base_day(None, day(2026, 10, 28), 30), day(2026, 11, 21));
    // Après un siège : + 7 jours, reculé d'un jour si c'est un dimanche.
    assert_eq!(base_day(Some(day(2026, 10, 24)), day(2026, 10, 28), 7), day(2026, 10, 31));
    assert_eq!(base_day(Some(day(2026, 10, 25)), day(2026, 10, 28), 7), day(2026, 10, 31), "dimanche 1er novembre → samedi");
    let s = slots(day(2026, 10, 31));
    assert_eq!(s.len(), 6);
    assert_eq!((s[0], s[5]), (day(2026, 10, 31).and_hms_opt(20, 0, 0).unwrap(), day(2026, 11, 1).and_hms_opt(22, 0, 0).unwrap()));
    let now = day(2026, 10, 28).and_hms_opt(12, 0, 0).unwrap();
    let last = Some(day(2026, 10, 24));
    assert!(declare_ok(last, day(2026, 10, 31).and_hms_opt(21, 0, 0).unwrap(), now, 7));
    assert!(declare_ok(last, day(2026, 11, 1).and_hms_opt(22, 0, 0).unwrap(), now, 7), "base + 26 h");
    assert!(!declare_ok(last, day(2026, 11, 1).and_hms_opt(22, 30, 0).unwrap(), now, 7));
    assert!(!declare_ok(last, day(2026, 10, 30).and_hms_opt(21, 0, 0).unwrap(), now, 7));
    assert!(declare_ok(None, day(2026, 12, 25).and_hms_opt(3, 0, 0).unwrap(), now, 7), "jamais de siège : toute date à venir");
    assert!(!declare_ok(None, day(2026, 10, 27).and_hms_opt(20, 0, 0).unwrap(), now, 7));
    assert_eq!(first_slot(last, now, 7), day(2026, 10, 31).and_hms_opt(20, 0, 0).unwrap());
    // Créneaux passés : période suivante.
    let late = day(2026, 11, 1).and_hms_opt(22, 30, 0).unwrap();
    assert_eq!(first_slot(last, late, 7), day(2026, 11, 7).and_hms_opt(20, 0, 0).unwrap());
}

#[test]
fn dates_filetime() {
    let tz = crate::config::TimeZone::Utc;
    // 2026-10-31 20:00:00 UTC.
    let unix = 1_793_476_800;
    let ft = filetime(tz, unix);
    assert_eq!(ft, (unix as u64 + 11_644_473_600) * 10_000_000);
    assert_eq!(from_filetime(tz, ft), Some(unix));
    assert_eq!(filetime(tz, 0), 0);
    assert_eq!(to_local(tz, unix), day(2026, 10, 31).and_hms_opt(20, 0, 0).unwrap());
}

#[test]
fn camps_et_participants() {
    let mut c = Castle::new(1);
    c.owner = Some(10);
    c.applications = vec![
        Application { guild: 20, side: Side::Attack, selected: true },
        Application { guild: 21, side: Side::Attack, selected: false },
        Application { guild: 30, side: Side::Defense, selected: true },
        Application { guild: 31, side: Side::Defense, selected: false },
    ];
    assert_eq!((c.side_of(10), c.side_of(20), c.side_of(21), c.side_of(30), c.side_of(31)), (Some(Side::Defense), Some(Side::Attack), None, Some(Side::Defense), None));
    assert_eq!(c.defenders(), vec![10, 30]);
    assert_eq!(c.participants(), vec![10, 30, 20]);
    assert_eq!((c.applicants(Side::Attack), c.selected(Side::Defense)), (vec![20, 21], vec![30]));
    assert_eq!(variants(1269), Some((1287, 1288)));
    assert_eq!(variants(1266), Some((1364, 1364)));
    assert_eq!(variants(1272), None);
}

#[test]
fn paquets_du_siege() {
    let p = result_packet("Inflame", siege_result::ATTACK_APPLIED, 7);
    assert_eq!(parse_header(&p).map(|h| h.ptype), Some(0x49));
    let d = decode_packet(&schema::SIEGE_WAR, &p).unwrap();
    assert_eq!((d.int(0), d.int(1), d.int(3)), (Some(siege_op::RESULT), Some(9), Some(7)));
    let castle = decode_mini(&mini::SIEGE_CASTLE, d.bytes(2).unwrap()).unwrap();
    assert_eq!(castle.text(0).as_deref(), Some("Inflame"));
    // Page : 17 guildes → 2 pages, la deuxième en montre 2.
    let rows: Vec<(String, String, i64)> = (0..17).map(|i| (format!("G{i}"), format!("M{i}"), 100 - i)).collect();
    let p = page_packet(siege_op::ATTACK_PAGE, "Inflame", 7, &rows, 2);
    let d = decode_packet(&schema::SIEGE_WAR, &p).unwrap();
    let page = decode_mini(&mini::SIEGE_GUILD_PAGE, d.bytes(4).unwrap()).unwrap();
    assert_eq!((page.int(0), page.int(1)), (Some(2), Some(2)));
    let entries = page.list(2).unwrap();
    assert_eq!(entries.len(), 2);
    let Value::Bytes(first) = &entries[0] else { panic!("entrée") };
    let e = decode_mini(&mini::SIEGE_GUILD_ENTRY, first).unwrap();
    assert_eq!((e.text(0).as_deref(), e.text(1).as_deref(), e.int(2)), (Some("G15"), Some("M15"), Some(85)));
    let p = status_msg_packet(archcore_net::schema::siege_msg::EYE_OPENED, "Inflame", "SimGuild", "GmTest", kind::EYE, 1);
    let d = decode_packet(&schema::SIEGE_WAR, &p).unwrap();
    let m = decode_mini(&mini::SIEGE_STATUS_MSG, d.bytes(8).unwrap()).unwrap();
    assert_eq!((m.int(0), m.text(1).as_deref(), m.text(2).as_deref(), m.int(4), m.int(6)), (Some(6), Some("Inflame"), Some("SimGuild"), Some(7), Some(1)));
    let d = decode_packet(&schema::SIEGE_WAR, &time_packet(2, 70_000)).unwrap();
    let t = decode_mini(&mini::SIEGE_TIME, d.bytes(10).unwrap()).unwrap();
    assert_eq!((t.int(0), t.int(1)), (Some(2), Some(65_535)));
    // Texte vide : un zéro seul, pour effacer l'ancienne valeur chez le client.
    assert_eq!(mb(""), vec![0]);
}

/// Château 1 en guerre : porte extérieure, tour de garde, porte intérieure, tour de résurrection, trône, Œil, camp ; un
/// garde.
fn war_state() -> (WorldState, Vec<u32>) {
    let tids = [1269, 1268, 1265, 1264, 1312, 1266, 1263, 1272];
    let kinds = [kind::OUTER_GATE, kind::GUARD_TOWER, kind::INNER_GATE, kind::REVIVAL_TOWER, kind::THRONE, kind::EYE, kind::CAMP, 0];
    let points: Vec<SpawnPoint> = tids
        .iter()
        .enumerate()
        .map(|(i, tid)| SpawnPoint {
            id: i as u32 + 1,
            tid: *tid,
            name: format!("objet {tid}"),
            level: 1,
            count: 1,
            pos: Vec3::new(i as f32 * 100.0, 0.0, 0.0),
            degree_x: 0.0,
            radius: 0.0,
            max_hp: 1000,
            respawn_sec: 5,
            respawn_max_sec: 0,
            ai_tid: 0,
        })
        .collect();
    let mut st = WorldState { monsters: MonsterRegistry::from_points(&points, &|_, _| false), ..Default::default() };
    let cids: Vec<u32> = st.monsters.monsters.iter().map(|m| m.cid).collect();
    for (i, cid) in cids.iter().enumerate() {
        st.siege.objects.insert(*cid, SiegeObject { castle: 1, kind: kinds[i], base_tid: tids[i], guard: kinds[i] == 0, ..Default::default() });
    }
    let mut c = Castle::new(1);
    c.status = status::WAR;
    st.siege.castles.insert(1, c);
    (st, cids)
}

#[test]
fn ordre_d_attaque_des_objets() {
    let (mut st, cids) = war_state();
    let [outer, tower, inner, revival, throne, eye, camp, guard] = cids[..] else { panic!() };
    let a = Some(Side::Attack);
    let d = Some(Side::Defense);
    assert!(allows(&st, a, outer) && allows(&st, a, tower));
    assert!(!allows(&st, a, inner) && !allows(&st, a, revival) && !allows(&st, a, throne) && !allows(&st, a, eye) && !allows(&st, a, camp));
    assert!(!allows(&st, d, outer) && allows(&st, d, camp), "le défenseur ne frappe que camps et catapultes");
    assert!(!allows(&st, None, outer), "hors du siège : rien");
    assert!(allows(&st, a, guard) && allows(&st, None, guard) && !allows(&st, d, guard), "gardes : tous sauf les défenseurs");
    // Porte extérieure tombée : porte intérieure et tour de résurrection.
    st.siege.objects.get_mut(&outer).unwrap().destroyed = true;
    assert!(!allows(&st, a, outer), "objet détruit");
    assert!(allows(&st, a, inner) && allows(&st, a, revival) && !allows(&st, a, throne));
    // Porte intérieure tombée : trône et Œil.
    st.siege.objects.get_mut(&inner).unwrap().destroyed = true;
    assert!(allows(&st, a, throne) && allows(&st, a, eye));
    // Œil éveillé (autre modèle) : plus de coups.
    st.monsters.get_mut(eye).unwrap().tid = 1364;
    assert!(!allows(&st, a, eye));
    // Hors de la guerre : rien.
    st.siege.castles.get_mut(&1).unwrap().status = status::ENDED;
    assert!(!allows(&st, a, tower) && !allows(&st, a, guard));
    assert!(war::may_hit_auto(&st, guard) && !war::may_hit_auto(&st, tower));
    assert!(allows(&st, a, 0x1234), "monstre ordinaire");
}
