//! PNJ côté moteur : dialogue, boutique, banque, quêtes. Protocole : `archrust_netclient::npc` ;
//! fenêtres d'Archonia : `hud_npc.rs`.
//!
//! * Clic droit sur un PNJ (ou clic gauche sur le PNJ déjà ciblé) : le joueur s'en approche et le
//!   dialogue s'ouvre (`npcdialog.txt`), avec un bouton par fonction : boutique, banque, quêtes.
//! * Boutique : clic droit sur un objet pour l'acheter (Maj : quantité) ; vendre un objet du sac en
//!   le lâchant sur la boutique ou d'un clic droit, après accord.
//! * Banque : glisser entre le sac et la banque, ou clic droit ; boutons « Déposer » / « Retirer »
//!   pour l'argent ; « Agrandir » (Archonia `Item_RequestBuyBankSlot`) ouvre `UI_BuyBank` : prix de
//!   `bankslotprice.txt` selon les pages déjà achetées (-1 : pas en vente contre de l'or), or porté
//!   vérifié avant l'envoi (`Item_BuyBankSlot`, sinon `Bank_NotEnoughBuyCost`), AGPMITEM op 22 ;
//!   réponse : champ 23 du personnage (onglets ouverts) et message système 30 ou 29, affiché avec
//!   `sysstr.txt` 718 ou 717 (« L'entrepot a ete agrandi avec succes. » / « … ne peut plus etre
//!   agrandi. » : même sens que les codes d'ArchCore, correspondance non retrouvée dans l'exe).
//! * Quêtes : celles du groupe du PNJ (`questgroup.ini`), texte de `questtemplate.ini` (le client
//!   français n'en décrit qu'une soixantaine) ; journal par `L` ou `/quetes`.
//! * Commandes : `/parler [nom]`, `/quetes`, `/prendrequete TID`, `/abandonner TID`.

use super::App;
use crate::assets::Assets;
use crate::hud::{BankView, BuyBankView, DialogView, HudState, ItemUi, NpcUi, PortalUi, QuestEntry, QuestState, QuestView, ShopEntry, ShopView};
use crate::net::NetLink;
use crate::scene::item_name_for_tid;
use crate::ui::UiState;
use archrust_data::ini::CodePage;
use archrust_netclient::npc::{function, trade_result, NpcCommand, NpcEvent, NpcFunction, QuestProgress, STATUS_BANK};
use archrust_netclient::world::ItemRecord;
use archrust_netclient::{Command, WorldEvent};
use glam::Vec3;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::time::{Duration, Instant};

/// Distance à laquelle le serveur accepte de parler (1500) ; le client s'approche un peu plus.
const TALK_RANGE: f32 = 1200.0;
/// Au-delà, les fenêtres du PNJ se ferment.
const CLOSE_RANGE: f32 = 2200.0;
/// Banque : 4 pages de 7 lignes × 4 colonnes.
const BANK_ROWS: i16 = 7;
const BANK_COLS: i16 = 4;

/// Texte d'une quête (`questtemplate.ini`).
#[derive(Debug, Clone, Default)]
struct QuestText {
    name: String,
    level: (Option<i32>, Option<i32>),
    start: String,
    progress: String,
    finish: String,
    summary: Vec<String>,
    monsters: Vec<(String, i32)>,
    money: i64,
    exp: i64,
    gifts: Vec<(String, i32)>,
}

/// Données des PNJ lues dans le client.
#[derive(Default)]
struct NpcData {
    /// Répliques par modèle de dialogue (`npcdialog.txt`).
    dialogs: HashMap<i32, Vec<String>>,
    /// Quêtes proposées et reçues par groupe (`questgroup.ini`).
    groups: HashMap<i32, (Vec<i32>, Vec<i32>)>,
    quests: HashMap<i32, QuestText>,
    /// Prix des pages de banque en plus (`bankslotprice.txt`, indice = pages déjà achetées).
    bank_prices: Vec<i64>,
}

struct Shop {
    npc: i32,
    items: Vec<ShopEntry>,
    loading: bool,
    first_row: i16,
}

#[derive(Default)]
pub struct NpcGlue {
    data: Option<NpcData>,
    /// Fonctions de chaque PNJ vu.
    functions: HashMap<i32, Vec<NpcFunction>>,
    /// PNJ vers lequel le joueur marche pour lui parler.
    approach: Option<(i32, Instant)>,
    dialog: Option<i32>,
    /// Réplique affichée (change à chaque ouverture).
    line: usize,
    shop: Option<Shop>,
    bank: Option<i32>,
    bank_page: i16,
    /// PNJ dont la fenêtre des quêtes est ouverte.
    quest_npc: Option<i32>,
    pub show_journal: bool,
    selected_quest: Option<i32>,
    journal: BTreeMap<i32, QuestProgress>,
    done: HashSet<u32>,
    /// Fenêtres de saisie : achat (IID, quantité), vente (IID), argent (dépôt, montant).
    buy: Option<(i32, i32)>,
    sell: Option<i32>,
    money: Option<(bool, i64)>,
    /// Fenêtre d'achat d'une page de banque (`UI_BuyBank`) ouverte.
    buy_bank: bool,
}

/// Ce que les fenêtres dessinent, préparé avant la passe egui.
#[derive(Default)]
pub struct NpcViews {
    dialog: Option<DialogView>,
    shop_name: String,
    bank_items: Vec<(ItemRecord, String)>,
    quests: Option<(bool, String, Vec<QuestEntry>, String)>,
}

/// Texte d'un fichier du client, version française d'abord (`ini_fr/`), déchiffré.
pub(super) fn client_text(assets: &Assets, file: &str) -> String {
    ["ini_fr", "ini"].iter().map(|d| client_text_in(assets, d, file)).find(|t| !t.is_empty()).unwrap_or_default()
}

/// Texte d'un fichier d'un dossier de tables (`ini`, `ini_fr`…), déchiffré s'il le faut.
pub(super) fn client_text_in(assets: &Assets, dir: &str, file: &str) -> String {
    let Ok(raw) = assets.read_file(&format!("{dir}/{file}")) else { return String::new() };
    let cp = CodePage::for_dir(dir);
    let plain = raw.iter().take(256).filter(|&&b| b < 0x20 && b != b'\r' && b != b'\n' && b != b'\t').count() * 20 < raw.len().min(256);
    if plain {
        archrust_data::ini::decode_text(&raw, cp)
    } else {
        archrust_data::ini::decrypt_ini_with(&raw, cp)
    }
}

/// Prix des pages de banque en plus : `bankslotprice.txt` (`Slot<TAB>Price`, la page 1 achetée en
/// premier).
fn bank_slot_prices(text: &str) -> Vec<i64> {
    let mut prices = Vec::new();
    for line in text.lines().skip(1) {
        let mut cols = line.split('\t');
        if let (Some(Ok(slot)), Some(Ok(price))) = (cols.next().map(|s| s.trim().parse::<usize>()), cols.next().map(|s| s.trim().parse::<i64>())) {
            if slot >= 1 {
                prices.resize(prices.len().max(slot), -1);
                prices[slot - 1] = price;
            }
        }
    }
    prices
}

/// Texte d'une ligne `id=texte` de `sysstr.txt`.
fn sysstr(assets: &Assets, id: u32) -> Option<String> {
    let text = client_text(assets, "sysstr.txt");
    let key = format!("{id}=");
    text.lines().find_map(|l| l.strip_prefix(&key)).map(|t| t.trim().to_string())
}

/// Lignes d'un fichier tabulé ; un champ entre guillemets peut contenir des retours à la ligne.
fn tsv(text: &str) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    let (mut row, mut field, mut quoted) = (Vec::new(), String::new(), false);
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' if quoted && chars.peek() == Some(&'"') => {
                field.push('"');
                chars.next();
            }
            '"' if quoted => quoted = false,
            '"' if field.is_empty() => quoted = true,
            '\t' if !quoted => row.push(std::mem::take(&mut field)),
            '\r' if !quoted => {}
            '\n' if !quoted => {
                row.push(std::mem::take(&mut field));
                rows.push(std::mem::take(&mut row));
            }
            c => field.push(c),
        }
    }
    if !field.is_empty() || !row.is_empty() {
        row.push(field);
        rows.push(row);
    }
    rows
}

/// Enlève les codes de couleur d'Archonia (`<HFFFA7272>`) et les crochets de nom (`<[…]>`).
fn clean(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find('<') {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        let color = tail.len() >= 11 && tail.as_bytes()[1] == b'H' && tail[2..10].chars().all(|c| c.is_ascii_hexdigit()) && tail.as_bytes()[10] == b'>';
        if color {
            rest = &tail[11..];
        } else if let Some(t) = tail.strip_prefix("<[") {
            rest = t;
        } else if let (true, Some(open)) = (tail.starts_with("<POS:"), tail.find('[')) {
            // Lien de position « <POS::carte::x::z::[Nom]> » : le nom seul.
            rest = &tail[open + 1..];
        } else {
            out.push('<');
            rest = &tail[1..];
        }
    }
    out.push_str(rest);
    out.replace("]>", "").replace("\\n", "\n").trim().trim_matches('"').trim().to_string()
}

fn load_data(assets: &Assets) -> NpcData {
    let mut data = NpcData::default();
    // npcdialog.txt : ID, nom, …, réplique (colonne 5) ; lignes de suite sans ID ; « - » sépare.
    // Lu ligne à ligne : le fichier contient des guillemets orphelins en début de ligne (suite d'une
    // réplique de « méchant »), qui décaleraient une lecture avec guillemets.
    let mut current: Option<i32> = None;
    let text = client_text(assets, "npcdialog.txt");
    for row in text.lines().skip(1).map(|l| l.split('\t').map(str::to_string).collect::<Vec<_>>()) {
        let first = row.first().map(|s| s.trim()).unwrap_or("");
        if first == "-" {
            current = None;
            continue;
        }
        if let Ok(id) = first.parse::<i32>() {
            current = Some(id);
        }
        let (Some(id), Some(line)) = (current, row.get(5).map(|s| clean(s)).filter(|s| !s.is_empty() && s != "-")) else { continue };
        data.dialogs.entry(id).or_default().push(line);
    }
    // questgroup.ini : ID, nom, quêtes proposées « a:b: », quêtes reçues.
    let list = |s: Option<&String>| s.map(|s| s.split(':').filter_map(|t| t.trim().parse().ok()).collect()).unwrap_or_default();
    for row in tsv(&client_text(assets, "questgroup.ini")) {
        if let Some(id) = row.first().and_then(|s| s.trim().parse::<i32>().ok()) {
            data.groups.insert(id, (list(row.get(2)), list(row.get(3))));
        }
    }
    // questtemplate.ini : colonnes nommées par l'en-tête.
    let rows = tsv(&client_text(assets, "questtemplate.ini"));
    if let Some((header, body)) = rows.split_first() {
        let col = |name: &str| header.iter().position(|h| h.trim().eq_ignore_ascii_case(name));
        for row in body {
            let get = |name: &str| col(name).and_then(|i| row.get(i)).map(|s| clean(s)).filter(|s| !s.is_empty());
            let num = |name: &str| get(name).and_then(|s| s.parse::<i64>().ok());
            let Some(tid) = num("Tid") else { continue };
            let mut q = QuestText {
                name: get("QuestName").unwrap_or_else(|| format!("Quête n° {tid}")),
                level: (num("Need_Level_Min").map(|v| v as i32), num("Need_Level_Max").map(|v| v as i32)),
                start: get("Dialogue_Start").unwrap_or_default(),
                progress: get("Dialogue_Progress").unwrap_or_default(),
                finish: get("Dialogue_Finish").unwrap_or_default(),
                money: num("Gift_Money").unwrap_or(0),
                exp: num("Gift_Exp").unwrap_or(0),
                ..Default::default()
            };
            q.summary = (1..=5).filter_map(|i| get(&format!("Complete_Summary{i}"))).collect();
            for m in ["A", "B"] {
                if let (Some(name), Some(n)) = (get(&format!("Complete_Monster{m}_Name")), num(&format!("Complete_Monster{m}_Count"))) {
                    q.monsters.push((name, n as i32));
                }
            }
            for g in ["A", "B", "C", "D"] {
                if let Some(name) = get(&format!("Gift_Item{g}_Name")) {
                    q.gifts.push((name, num(&format!("Gift_Item{g}_Count")).unwrap_or(1) as i32));
                }
            }
            data.quests.insert(tid as i32, q);
        }
    }
    log::info!(
        "PNJ : {} dialogue(s), {} groupe(s) de quêtes, {} quête(s) décrite(s) par le client",
        data.dialogs.len(),
        data.groups.len(),
        data.quests.len()
    );
    data.bank_prices = bank_slot_prices(&client_text(assets, "bankslotprice.txt"));
    data
}

impl NpcGlue {
    fn data(&mut self, assets: &Assets) -> &NpcData {
        self.data.get_or_insert_with(|| load_data(assets))
    }

    fn quest_name(&mut self, assets: &Assets, tid: i32) -> String {
        self.data(assets).quests.get(&tid).map(|q| q.name.clone()).unwrap_or_else(|| format!("Quête n° {tid}"))
    }

    fn state(&self, tid: i32) -> QuestState {
        match self.journal.get(&tid) {
            Some(p) if p.done => QuestState::Ready,
            Some(_) => QuestState::InProgress,
            None if self.done.contains(&(tid as u32)) => QuestState::Done,
            None => QuestState::Available,
        }
    }

    /// Texte d'une quête : niveaux, dialogue selon l'état, objectifs et progression, récompenses.
    fn quest_text(&mut self, assets: &Assets, tid: i32) -> String {
        let state = self.state(tid);
        let progress = self.journal.get(&tid).copied();
        let Some(q) = self.data(assets).quests.get(&tid).cloned() else {
            let mut t = format!("Quête n° {tid}\n\nLe client ne décrit pas cette quête (le client français n'en connaît qu'une soixantaine).");
            if let Some(p) = progress {
                t.push_str(&format!("\n\nProgression : {} et {} monstre(s){}", p.monster_a, p.monster_b, if p.done { " — objectifs remplis" } else { "" }));
            }
            return t;
        };
        let mut t = q.name.clone();
        match q.level {
            (Some(a), Some(b)) => t.push_str(&format!("\nNiveau {a} à {b}")),
            (Some(a), None) => t.push_str(&format!("\nNiveau {a} et plus")),
            _ => {}
        }
        let talk = match state {
            QuestState::Ready => &q.finish,
            QuestState::InProgress => &q.progress,
            _ => &q.start,
        };
        if !talk.is_empty() {
            t.push_str(&format!("\n\n{talk}"));
        }
        if !q.summary.is_empty() {
            t.push_str("\n\nObjectifs :");
            for s in &q.summary {
                t.push_str(&format!("\n{s}"));
            }
        }
        if let Some(p) = progress {
            for (i, (name, n)) in q.monsters.iter().enumerate() {
                let have = if i == 0 { p.monster_a } else { p.monster_b };
                t.push_str(&format!("\n• {name} : {}/{n}", have.min(*n)));
            }
        }
        let mut gifts: Vec<String> = q.gifts.iter().map(|(n, c)| if *c > 1 { format!("{n} × {c}") } else { n.clone() }).collect();
        if q.money > 0 {
            gifts.push(format!("{} pièce(s) d'or", q.money));
        }
        if q.exp > 0 {
            gifts.push(format!("{} point(s) d'expérience", q.exp));
        }
        if !gifts.is_empty() {
            t.push_str(&format!("\n\nRécompenses : {}", gifts.join(", ")));
        }
        t
    }

    fn function_data(&self, npc: i32, f: i32) -> Option<i32> {
        self.functions.get(&npc)?.iter().find(|x| x.function == f)?.data
    }
}

/// Première case libre d'une banque (pages ouvertes) en commençant par la page affichée.
fn free_bank_cell(items: &HashMap<i32, ItemRecord>, first_page: i16, pages: i16) -> Option<[i16; 3]> {
    let taken: Vec<[i16; 3]> = items.values().filter(|i| i.status == Some(STATUS_BANK)).filter_map(|i| i.grid).collect();
    (0..pages)
        .map(|p| (first_page + p) % pages)
        .flat_map(|p| (0..BANK_ROWS).flat_map(move |r| (0..BANK_COLS).map(move |c| [p, r, c])))
        .find(|g| !taken.contains(g))
}

/// Ouvre une fonction d'un PNJ (bouton du dialogue ou commande) : boutique, banque, quêtes.
fn start_function(glue: &mut NpcGlue, npc: i32, f: i32, net: Option<&NetLink>, ui: &mut UiState) {
    match f {
        function::TRADE => {
            glue.shop = Some(Shop { npc, items: Vec::new(), loading: true, first_row: 0 });
            ui.show_inventory = true;
            glue.dialog = None;
            send(net, NpcCommand::ShopList(npc));
        }
        function::BANK => {
            glue.dialog = None;
            send(net, NpcCommand::BankOpen(npc));
        }
        function::QUEST => {
            glue.dialog = None;
            send(net, NpcCommand::QuestWindow(npc));
        }
        function::SKILL_MASTER => {
            glue.dialog = None;
            if let Some(net) = net {
                net.send(Command::Skill(archrust_netclient::skills::SkillCommand::MasterOpen(npc)));
            }
        }
        _ => {}
    }
}

/// Prix de la page de banque suivante (Archonia FUN_140314970 : pages achetées < 3 et prix ≥ 0).
fn next_bank_price(glue: &mut NpcGlue, hud: &HudState, assets: &Assets) -> Option<i64> {
    let bought = hud.stats.bank_pages.unwrap_or(0).max(0) as usize;
    glue.data(assets).bank_prices.get(bought).copied().filter(|p| *p >= 0 && bought < 3)
}

/// « Acheter » d'`UI_BuyBank` (`Item_BuyBankSlot`, FUN_1406f6c80) : l'or porté est vérifié avant
/// l'envoi (sinon `Bank_NotEnoughBuyCost`).
fn confirm_buy_bank(glue: &mut NpcGlue, hud: &mut HudState, assets: &Assets, net: Option<&NetLink>, ui: &mut UiState) {
    glue.buy_bank = false;
    let Some(price) = next_bank_price(glue, hud, assets) else { return };
    if hud.stats.money.unwrap_or(0) < price {
        let text = super::portal_glue::ui_event_text(assets, "Bank_NotEnoughBuyCost");
        hud.push_system(&text);
        ui.push_chat(format!("[banque] {text}"));
    } else {
        send(net, NpcCommand::BuyBankSlot);
    }
}

fn send(net: Option<&NetLink>, cmd: NpcCommand) {
    if let Some(net) = net {
        net.send(Command::Npc(cmd));
    }
}

/// Fenêtres des PNJ et leurs demandes, ainsi que les demandes du sac détournées par la boutique
/// (vente) et la banque (dépôt, retrait). À appeler après l'inventaire, avant `app_items::panel`.
#[allow(clippy::too_many_arguments)]
pub fn panel(
    ctx: &egui::Context,
    glue: &mut NpcGlue,
    views: &NpcViews,
    item_requests: &mut Vec<ItemUi>,
    items: &HashMap<i32, ItemRecord>,
    hud: &mut HudState,
    assets: &Assets,
    net: Option<&NetLink>,
    ui: &mut UiState,
) {
    let mut out: Vec<NpcUi> = Vec::new();
    if let Some(view) = &views.dialog {
        crate::hud::draw_npc_dialog(ctx, hud, assets, view, &mut out);
    }
    if let Some(shop) = glue.shop.as_mut() {
        let view = ShopView { npc_name: &views.shop_name, items: &shop.items, loading: shop.loading, first_row: &mut shop.first_row };
        crate::hud::draw_shop(ctx, hud, assets, view, &mut out);
    }
    let pages = 1 + hud.stats.bank_pages.unwrap_or(0).clamp(0, 3) as i16;
    let bought = hud.stats.bank_pages.unwrap_or(0).max(0) as usize;
    let price = next_bank_price(glue, hud, assets);
    if glue.bank.is_some() {
        let money = hud.stats.bank_money;
        crate::hud::draw_bank(ctx, hud, assets, BankView { items: &views.bank_items, money, pages, page: &mut glue.bank_page, can_buy: price.is_some() }, &mut out);
    }
    if glue.bank.is_none() || price.is_none() {
        glue.buy_bank = false;
    }
    if let (true, Some(price)) = (glue.buy_bank, price) {
        let messages: HashMap<String, String> = client_text(assets, "uimessage.txt").lines().filter_map(|l| l.split_once('\t')).map(|(k, v)| (k.trim().to_string(), v.trim().to_string())).collect();
        let msg = |k: &str| messages.get(k).cloned().unwrap_or_default();
        let money = hud.stats.money.unwrap_or(0);
        let view = BuyBankView {
            title: msg("Item_BuyBankSlotTitle").replace("%d", &(bought + 1).to_string()),
            price_label: msg("Item_BuyBankSlotPrice"),
            price,
            money_name: msg("Money_Name"),
            affordable: price <= money,
        };
        let mut answer = Vec::new();
        crate::hud::draw_buy_bank(ctx, hud, assets, &view, &mut answer);
        for a in answer {
            glue.buy_bank = false;
            if a == PortalUi::BuyBank {
                confirm_buy_bank(glue, hud, assets, net, ui);
            }
        }
    }
    if let Some((journal, title, entries, text)) = &views.quests {
        let view = QuestView { journal: *journal, title: title.clone(), entries, selected: glue.selected_quest, text: text.clone() };
        crate::hud::draw_quests(ctx, hud, assets, view, &mut out);
    }

    // Demandes du sac pendant qu'une boutique ou la banque est ouverte.
    item_requests.retain(|r| match *r {
        ItemUi::Activate { iid, worn: false } if glue.shop.is_some() => {
            glue.sell = Some(iid);
            false
        }
        ItemUi::Activate { iid, worn: false } if glue.bank.is_some() => {
            match free_bank_cell(items, glue.bank_page, pages) {
                Some(grid) => send(net, NpcCommand::BankStore { iid, grid }),
                None => ui.push_chat("[banque] La banque est pleine."),
            }
            false
        }
        ItemUi::DropOnBag { drag, grid } if drag.bank => {
            send(net, NpcCommand::BankTake { iid: drag.iid, grid: Some(grid) });
            false
        }
        _ => true,
    });

    for r in out {
        match r {
            NpcUi::CloseDialog => glue.dialog = None,
            NpcUi::Function(f) => {
                if let Some(npc) = glue.dialog {
                    start_function(glue, npc, f, net, ui);
                }
            }
            NpcUi::CloseShop => glue.shop = None,
            NpcUi::Buy { iid, ask_count: true } => glue.buy = Some((iid, 1)),
            NpcUi::Buy { iid, ask_count: false } => {
                if let Some(s) = &glue.shop {
                    send(net, NpcCommand::Buy { npc: s.npc, iid, count: 1 });
                }
            }
            NpcUi::Sell(iid) => glue.sell = Some(iid),
            NpcUi::CloseBank => glue.bank = None,
            NpcUi::BankDrop { drag, grid } => send(net, NpcCommand::BankStore { iid: drag.iid, grid }),
            NpcUi::BankTake(iid) => send(net, NpcCommand::BankTake { iid, grid: None }),
            NpcUi::BankMoney { deposit } => glue.money = Some((deposit, 0)),
            NpcUi::BankBuyRequest => glue.buy_bank = true,
            NpcUi::CloseQuests => {
                glue.quest_npc = None;
                glue.show_journal = false;
            }
            NpcUi::SelectQuest(tid) => glue.selected_quest = Some(tid),
            NpcUi::QuestAccept(tid) => send(net, NpcCommand::QuestAccept { npc: glue.quest_npc, tid }),
            NpcUi::QuestComplete(tid) => send(net, NpcCommand::QuestComplete { npc: glue.quest_npc, tid, reward: 0 }),
            NpcUi::QuestAbandon(tid) => send(net, NpcCommand::QuestAbandon(tid)),
        }
    }

    // Quantité à acheter.
    if let Some((iid, mut count)) = glue.buy {
        let entry = glue.shop.as_ref().and_then(|s| s.items.iter().find(|e| e.iid == iid)).map(|e| (e.name.clone(), e.price));
        let mut answer = None;
        egui::Window::new("Acheter").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO).show(ctx, |w| {
            let (name, price) = entry.clone().unwrap_or_default();
            w.label(name);
            w.horizontal(|w| {
                w.label("Quantité :");
                w.add(egui::DragValue::new(&mut count).range(1..=99));
                w.label(format!("= {} pièce(s)", price * count as i64));
            });
            w.horizontal(|w| {
                if w.button("Acheter").clicked() {
                    answer = Some(true);
                }
                if w.button("Annuler").clicked() {
                    answer = Some(false);
                }
            });
        });
        glue.buy = Some((iid, count));
        match (answer, &glue.shop) {
            (Some(true), Some(s)) => {
                send(net, NpcCommand::Buy { npc: s.npc, iid, count });
                glue.buy = None;
            }
            (Some(_), _) | (None, None) => glue.buy = None,
            _ => {}
        }
    }

    // Vente, après accord.
    if let Some(iid) = glue.sell {
        let name = items.get(&iid).and_then(|i| i.template_id).and_then(|t| item_name_for_tid(assets, t));
        let mut answer = None;
        egui::Window::new("Vendre").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO).show(ctx, |w| {
            w.label(format!("Vendre « {} » au marchand ?", name.as_deref().unwrap_or("?")));
            w.horizontal(|w| {
                if w.button("Vendre").clicked() {
                    answer = Some(true);
                }
                if w.button("Annuler").clicked() {
                    answer = Some(false);
                }
            });
        });
        match (answer, &glue.shop) {
            (Some(true), Some(s)) => {
                send(net, NpcCommand::Sell { npc: s.npc, iid });
                glue.sell = None;
            }
            (Some(_), _) | (None, None) => glue.sell = None,
            _ => {}
        }
    }

    // Argent de la banque.
    if let Some((deposit, mut amount)) = glue.money {
        let max = if deposit { hud.stats.money } else { hud.stats.bank_money }.unwrap_or(0).max(0);
        let mut answer = None;
        egui::Window::new(if deposit { "Déposer de l'or" } else { "Retirer de l'or" })
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |w| {
                w.horizontal(|w| {
                    w.label("Montant :");
                    w.add(egui::DragValue::new(&mut amount).range(0..=max));
                    w.label(format!("sur {max}"));
                });
                w.horizontal(|w| {
                    if w.button("Valider").clicked() {
                        answer = Some(true);
                    }
                    if w.button("Annuler").clicked() {
                        answer = Some(false);
                    }
                });
            });
        glue.money = Some((deposit, amount));
        match answer {
            Some(true) if amount > 0 => {
                send(net, NpcCommand::BankMoney(if deposit { amount } else { -amount }));
                glue.money = None;
            }
            Some(_) => glue.money = None,
            None => {}
        }
    }
}

impl App {
    /// Fonctions des PNJ en vue (champ 4 de leur vue).
    pub(super) fn npc_observe(&mut self, ev: &WorldEvent) {
        if let WorldEvent::CharacterSeen(c) = ev {
            if !c.npc_functions.is_empty() {
                self.npc.functions.insert(c.cid, c.npc_functions.clone());
            }
        }
    }

    /// Message système sans texte (0x44) : codes de la banque 29 (au maximum) et 30 (page ajoutée).
    pub(super) fn npc_system_code(&mut self, code: i32) {
        let id = match code {
            29 => 717,
            30 => 718,
            _ => {
                log::debug!("message système {code} sans texte connu");
                return;
            }
        };
        if let Some(text) = sysstr(&self.assets, id) {
            self.hud.push_system(&text);
            self.ui.push_chat(format!("[banque] {text}"));
        }
    }

    /// Fonctions connues d'un PNJ.
    pub(super) fn npc_functions(&self, npc: i32) -> &[NpcFunction] {
        self.npc.functions.get(&npc).map(Vec::as_slice).unwrap_or(&[])
    }

    fn npc_position(&self, cid: i32) -> Option<Vec3> {
        self.entities.iter().find(|e| e.cid == cid).map(|e| e.position)
    }

    fn npc_name(&self, cid: i32) -> String {
        self.entities.iter().find(|e| e.cid == cid).map(|e| e.name.clone()).unwrap_or_else(|| "PNJ".into())
    }

    /// Parler à un PNJ qui a des fonctions : il devient la cible, le joueur s'en approche et le
    /// dialogue est demandé. Rend `false` pour un personnage sans fonctions.
    pub(super) fn npc_talk(&mut self, cid: i32) -> bool {
        if !self.npc.functions.contains_key(&cid) {
            return false;
        }
        self.addons.set_target(Some(cid));
        let (Some(npc), Some(me)) = (self.npc_position(cid), self.player.as_ref().map(|p| p.position)) else { return true };
        let gap = Vec3::new(npc.x - me.x, 0.0, npc.z - me.z);
        if gap.length() <= TALK_RANGE {
            self.npc_request_dialog(cid);
            return true;
        }
        // S'approcher jusqu'à 600 unités du PNJ.
        let dest = npc - gap.normalize_or_zero() * 600.0;
        if let (Some(net), Some(p)) = (&self.net, &mut self.player) {
            net.send(Command::MoveTo { position: p.position, destination: dest });
            p.destination = dest;
            self.walking = true;
        }
        self.npc.approach = Some((cid, Instant::now()));
        true
    }

    /// Demande le dialogue. Le serveur juge la distance sur la position connue au dernier arrêt :
    /// un joueur qui marche s'arrête d'abord là où il est.
    fn npc_request_dialog(&mut self, cid: i32) {
        if self.walking {
            self.walking = false;
            if let (Some(net), Some(p)) = (&self.net, &mut self.player) {
                p.destination = p.position;
                net.send(Command::Stop { position: p.position });
            }
        }
        send(self.net.as_ref(), NpcCommand::Dialog(cid));
    }

    /// À chaque image : dialogue demandé à l'arrivée près du PNJ ; fenêtres fermées si le joueur
    /// s'éloigne.
    pub(super) fn npc_frame(&mut self) {
        let Some(me) = self.player.as_ref().map(|p| p.position) else { return };
        let dist = |s: &Self, cid: i32| s.npc_position(cid).map(|n| Vec3::new(n.x - me.x, 0.0, n.z - me.z).length());
        if let Some((cid, since)) = self.npc.approach {
            match dist(self, cid) {
                Some(d) if d <= TALK_RANGE => {
                    self.npc.approach = None;
                    self.npc_request_dialog(cid);
                }
                _ if since.elapsed() > Duration::from_secs(20) || !self.walking => self.npc.approach = None,
                _ => {}
            }
        }
        let far = |s: &Self, cid: Option<i32>| cid.is_some_and(|c| dist(s, c).is_none_or(|d| d > CLOSE_RANGE));
        if far(self, self.npc.dialog) {
            self.npc.dialog = None;
        }
        if far(self, self.npc.shop.as_ref().map(|s| s.npc)) {
            self.npc.shop = None;
        }
        if far(self, self.npc.bank) {
            self.npc.bank = None;
        }
        if far(self, self.npc.quest_npc) {
            self.npc.quest_npc = None;
        }
    }

    /// Réponses des PNJ.
    pub(super) fn on_npc_event(&mut self, ev: NpcEvent) {
        let assets = self.assets.clone();
        match ev {
            NpcEvent::DialogGranted { npc } => {
                self.npc.dialog = Some(npc);
                self.npc.line += 1;
            }
            NpcEvent::ShopItem { npc, iid, tid, price, count, grid } => {
                if let Some(s) = self.npc.shop.as_mut().filter(|s| s.npc == npc) {
                    let name = item_name_for_tid(&assets, tid).unwrap_or_else(|| format!("Objet TID {tid}"));
                    s.items.retain(|e| e.iid != iid);
                    s.items.push(ShopEntry { iid, tid, price, count, grid, name });
                }
            }
            // La fin de liste ne désigne pas toujours le PNJ (champ 0 absent) : boutique ouverte.
            NpcEvent::ShopEnd { npc } => {
                if let Some(s) = self.npc.shop.as_mut().filter(|s| s.npc == npc || npc == 0) {
                    s.loading = false;
                }
            }
            NpcEvent::Bought { result } => self.ui.push_chat(match result {
                trade_result::BUY_OK => "[boutique] Achat effectué.".to_string(),
                trade_result::NOT_ENOUGH_MONEY => "[boutique] Vous n'avez pas assez d'or.".to_string(),
                trade_result::INVENTORY_FULL => "[boutique] Votre inventaire est plein.".to_string(),
                r => format!("[boutique] Achat refusé (code {r})."),
            }),
            NpcEvent::Sold { result } => self.ui.push_chat(match result {
                trade_result::SELL_OK => "[boutique] Objet vendu.".to_string(),
                trade_result::TOO_MUCH_MONEY => "[boutique] Vous avez déjà trop d'or.".to_string(),
                r => format!("[boutique] Vente refusée (code {r})."),
            }),
            NpcEvent::BankOpened { npc } => {
                self.npc.bank = Some(npc);
                self.ui.show_inventory = true;
            }
            NpcEvent::QuestWindow { npc } => {
                self.npc.quest_npc = Some(npc);
                self.npc.show_journal = false;
                self.npc.selected_quest = None;
            }
            NpcEvent::QuestResult { op, tid, success } => {
                let name = self.npc.quest_name(&assets, tid);
                let text = match (op, success) {
                    (1, true) => {
                        self.npc.journal.entry(tid).or_insert(QuestProgress { tid, monster_a: 0, monster_b: 0, done: false });
                        format!("Quête « {name} » acceptée.")
                    }
                    (1, false) => format!("Quête « {name} » refusée (niveau, race, classe, quête précédente, journal plein ou déjà faite)."),
                    (3, true) => {
                        self.npc.journal.remove(&tid);
                        self.npc.done.insert(tid as u32);
                        format!("Quête « {name} » terminée !")
                    }
                    (3, false) => format!("Quête « {name} » : objectifs pas encore remplis."),
                    (7, true) => {
                        self.npc.journal.remove(&tid);
                        format!("Quête « {name} » abandonnée.")
                    }
                    _ => format!("Quête « {name} » : demande refusée."),
                };
                self.ui.push_chat(format!("[quête] {text}"));
            }
            NpcEvent::QuestJournal { progress, update } => {
                let before = self.npc.journal.insert(progress.tid, progress);
                if update && before != Some(progress) {
                    let name = self.npc.quest_name(&assets, progress.tid);
                    let mut parts = Vec::new();
                    if let Some(q) = self.npc.data(&assets).quests.get(&progress.tid) {
                        for (i, (m, n)) in q.monsters.iter().enumerate() {
                            parts.push(format!("{m} {}/{n}", if i == 0 { progress.monster_a } else { progress.monster_b }.min(*n)));
                        }
                    }
                    if parts.is_empty() {
                        parts.push(format!("{} / {}", progress.monster_a, progress.monster_b));
                    }
                    let done = if progress.done { " — objectifs remplis !" } else { "" };
                    self.ui.push_chat(format!("[quête] {name} : {}{done}", parts.join(", ")));
                }
            }
            NpcEvent::QuestsDone(list) => self.npc.done = list.into_iter().collect(),
        }
    }

    /// Données des fenêtres pour cette image.
    pub(super) fn npc_views(&mut self) -> NpcViews {
        let assets = self.assets.clone();
        let mut v = NpcViews::default();
        if let Some(npc) = self.npc.dialog {
            let template = self.npc.function_data(npc, function::DIALOG);
            let line = self.npc.line;
            let text = template
                .and_then(|t| self.npc.data(&assets).dialogs.get(&t).filter(|l| !l.is_empty()).map(|l| l[line % l.len()].clone()))
                .unwrap_or_else(|| "Bonjour, voyageur.".into());
            let mut buttons = Vec::new();
            for f in self.npc.functions.get(&npc).into_iter().flatten() {
                let label = match f.function {
                    function::TRADE => "Commerce",
                    function::BANK => "Banque",
                    function::QUEST => "Quêtes",
                    function::SKILL_MASTER => "Compétences",
                    _ => continue,
                };
                buttons.push((f.function, label.to_string()));
            }
            v.dialog = Some(DialogView { npc_name: self.npc_name(npc), text, buttons });
        }
        if let Some(s) = &self.npc.shop {
            v.shop_name = self.npc_name(s.npc);
        }
        if self.npc.bank.is_some() {
            v.bank_items = self
                .items
                .values()
                .filter(|i| i.status == Some(STATUS_BANK))
                .map(|i| (i.clone(), i.template_id.and_then(|t| item_name_for_tid(&assets, t)).unwrap_or_else(|| "Objet".into())))
                .collect();
        }
        let journal = self.npc.show_journal && self.npc.quest_npc.is_none();
        if let Some(npc) = self.npc.quest_npc.filter(|_| !journal) {
            let group = self.npc.function_data(npc, function::QUEST);
            let (give, confirm) = group.and_then(|g| self.npc.data(&assets).groups.get(&g).cloned()).unwrap_or_default();
            let mut tids: Vec<i32> = give.iter().copied().filter(|t| !self.npc.done.contains(&(*t as u32))).collect();
            tids.extend(confirm.iter().copied().filter(|t| self.npc.journal.contains_key(t) && !give.contains(t)));
            let entries = self.quest_entries(&tids);
            let title = format!("Quêtes — {}", self.npc_name(npc));
            v.quests = Some((false, title, entries, String::new()));
        } else if journal {
            let tids: Vec<i32> = self.npc.journal.keys().copied().collect();
            let entries = self.quest_entries(&tids);
            v.quests = Some((true, format!("Journal des quêtes ({}/60)", tids.len()), entries, String::new()));
        }
        if let Some(q) = v.quests.as_mut() {
            if self.npc.selected_quest.is_none_or(|t| !q.2.iter().any(|e| e.tid == t)) {
                self.npc.selected_quest = q.2.first().map(|e| e.tid);
            }
            q.3 = match self.npc.selected_quest {
                Some(t) => self.npc.quest_text(&assets, t),
                None if q.0 => "Aucune quête en cours.".into(),
                None => "Ce personnage n'a pas de quête à vous proposer.".into(),
            };
        }
        v
    }

    fn quest_entries(&mut self, tids: &[i32]) -> Vec<QuestEntry> {
        let assets = self.assets.clone();
        tids.iter().map(|&tid| QuestEntry { tid, name: self.npc.quest_name(&assets, tid), state: self.npc.state(tid) }).collect()
    }

    /// `/parler [nom]`, `/boutique`, `/banque`, `/quetespnj`, `/quetes`, `/prendrequete TID`,
    /// `/abandonner TID`.
    pub(super) fn npc_slash(&mut self, cmd: &str, arg: &str) -> bool {
        match cmd {
            // Banque ouverte : `/agrandir` comme le bouton « Agrandir », `/agrandir oui` comme « Acheter ».
            "/agrandir" | "/expandbank" => {
                if self.npc.bank.is_none() || next_bank_price(&mut self.npc, &self.hud, &self.assets).is_none() {
                    self.ui.push_chat("[banque] Ouvrez la banque d'un gardien ; la page suivante doit être en vente contre de l'or.");
                } else if matches!(arg.trim(), "oui" | "yes") {
                    let assets = self.assets.clone();
                    confirm_buy_bank(&mut self.npc, &mut self.hud, &assets, self.net.as_ref(), &mut self.ui);
                } else {
                    self.npc.buy_bank = true;
                }
            }
            // Fonction du PNJ le plus proche qui la propose, s'il est à portée.
            "/boutique" | "/shop" | "/banque" | "/bank" | "/quetespnj" | "/npcquests" | "/instructeur" | "/trainer" => {
                let f = match cmd {
                    "/boutique" | "/shop" => function::TRADE,
                    "/banque" | "/bank" => function::BANK,
                    "/instructeur" | "/trainer" => function::SKILL_MASTER,
                    _ => function::QUEST,
                };
                let me = self.player.as_ref().map(|p| p.position).unwrap_or(Vec3::ZERO);
                let near = self
                    .entities
                    .iter()
                    .filter(|e| self.npc.functions.get(&e.cid).is_some_and(|fs| fs.iter().any(|x| x.function == f)))
                    .map(|e| (e.cid, Vec3::new(e.position.x - me.x, 0.0, e.position.z - me.z).length()))
                    .min_by(|a, b| a.1.total_cmp(&b.1));
                match near {
                    Some((cid, d)) if d <= TALK_RANGE * 1.2 => start_function(&mut self.npc, cid, f, self.net.as_ref(), &mut self.ui),
                    Some((cid, _)) => {
                        self.ui.push_chat(format!("[PNJ] {} est trop loin : approchez-vous (/parler).", self.npc_name(cid)));
                    }
                    None => self.ui.push_chat("[PNJ] Personne ici ne propose cela."),
                }
            }
            "/parler" | "/talk" => {
                let wanted = arg.trim().to_lowercase();
                let me = self.player.as_ref().map(|p| p.position).unwrap_or(Vec3::ZERO);
                let pick = if wanted.is_empty() {
                    self.addons.target_cid().filter(|c| self.npc.functions.contains_key(c))
                } else {
                    self.entities
                        .iter()
                        .filter(|e| self.npc.functions.contains_key(&e.cid) && e.name.to_lowercase().contains(&wanted))
                        .min_by_key(|e| e.position.distance(me) as i64)
                        .map(|e| e.cid)
                };
                match pick {
                    Some(cid) => {
                        self.npc_talk(cid);
                    }
                    None => self.ui.push_chat("[PNJ] Aucun personnage à qui parler (ciblez-le ou donnez son nom)."),
                }
            }
            "/quetes" | "/quêtes" | "/quests" | "/journal" => {
                self.npc.show_journal = !self.npc.show_journal;
                if self.npc.show_journal {
                    self.npc.quest_npc = None;
                }
            }
            "/prendrequete" | "/prendrequête" | "/abandonner" | "/abandon" => match arg.trim().parse::<i32>() {
                Ok(tid) => send(
                    self.net.as_ref(),
                    if cmd.starts_with("/prendre") { NpcCommand::QuestAccept { npc: None, tid } } else { NpcCommand::QuestAbandon(tid) },
                ),
                Err(_) => self.ui.push_chat(format!("[quête] usage : {cmd} TID")),
            },
            _ => return false,
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lecture_des_tables() {
        let rows = tsv("ID\tnom\n1\t\"un\ndeux\"\tx\n\t\tsuite\n");
        assert_eq!(rows[1], vec!["1".to_string(), "un\ndeux".to_string(), "x".to_string()]);
        assert_eq!(rows[2], vec!["".to_string(), "".to_string(), "suite".to_string()]);
        assert_eq!(clean("Detruisez <HFFFA7272><[Lord of the Vale]><HFFFFFFFF>."), "Detruisez Lord of the Vale.");
        assert_eq!(clean("a <b> c"), "a <b> c");
        assert_eq!(clean("Detruisez 6 <POS::115::-266267::-434221::[Winged Latch Deck]>."), "Detruisez 6 Winged Latch Deck.");
    }

    #[test]
    fn case_libre_de_la_banque() {
        let mut items = HashMap::new();
        items.insert(1, ItemRecord { operation: 0, id: 1, template_id: Some(1), owner_cid: None, count: Some(1), status: Some(STATUS_BANK), grid: Some([0, 0, 0]) });
        assert_eq!(free_bank_cell(&items, 0, 1), Some([0, 0, 1]));
        assert_eq!(free_bank_cell(&items, 1, 2), Some([1, 0, 0]));
    }

    #[test]
    fn prix_des_pages_de_banque() {
        assert_eq!(bank_slot_prices("Slot\tPrice\t\r\n1\t1000000\t# commentaire\r\n2\t-1\t\r\n3\t-1\t\r\n"), vec![1_000_000, -1, -1]);
    }
}
