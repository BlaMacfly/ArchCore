//! `archcore re extract-schemas <fichiers…>` : relève, dans les pseudocodes Ghidra (sorties
//! `batch_decompile` de REA, JSON) des constructeurs de modules Agpm*/Agcm*, le nom du module, les
//! types de paquets enregistrés et chaque appel à `AuPacket::SetFieldType` (FUN_14014e850) :
//! (type, taille)… terminé par 0xff. Écrit `data/re/packet-schemas.json`.

use regex::Regex;
use serde_json::{json, Value};
use std::collections::HashMap;

const TYPE_NAMES: [&str; 16] = [
    "CHAR", "INT8", "UINT8", "INT16", "UINT16", "INT32", "UINT32", "INT64", "UINT64", "FLOAT", "VEC3F", "MATRIX", "PACKET", "MEMORY_BLOCK",
    "POS_BASEMETER", "WCHAR",
];

pub fn run(args: &[String]) -> anyhow::Result<()> {
    match args.first().map(String::as_str) {
        Some("extract-schemas") => extract(&args[1..]),
        _ => anyhow::bail!("usage : archcore re extract-schemas <fichiers batch_decompile…>"),
    }
}

fn num(tok: &str) -> Option<i64> {
    let t = tok.trim();
    if let Some(h) = t.strip_prefix("0x") {
        return i64::from_str_radix(h, 16).ok();
    }
    t.parse::<i64>().ok()
}

#[derive(Clone, Debug)]
enum Arg {
    N(i64),
    Raw(String),
}

fn resolve(tok: &str, vars: &HashMap<String, String>, depth: u32) -> Arg {
    let t = tok.trim();
    if let Some(inner) = t.strip_prefix("CONCAT44(").and_then(|r| r.strip_suffix(')')) {
        if let Some((_, b)) = inner.split_once(',') {
            return resolve(b, vars, depth + 1);
        }
    }
    if let Some(n) = num(t) {
        return Arg::N(n);
    }
    match vars.get(t) {
        Some(v) if depth < 5 => resolve(v, vars, depth + 1),
        _ => Arg::Raw(t.to_string()),
    }
}

fn split_args(s: &str) -> Vec<String> {
    let (mut out, mut depth, mut cur) = (Vec::new(), 0i32, String::new());
    for ch in s.chars() {
        match ch {
            '(' => depth += 1,
            ')' => depth -= 1,
            _ => {}
        }
        if ch == ',' && depth == 0 {
            out.push(std::mem::take(&mut cur));
        } else {
            cur.push(ch);
        }
    }
    out.push(cur);
    out
}

fn parse_function(code: &str) -> Value {
    let assign = Regex::new(r"^(\w+) = (.+);$").unwrap();
    let name_re = Regex::new(r#"strncpy\(\w+,"([^"]+)""#).unwrap();
    let ptype_re = Regex::new(r"\+ 0x1264\) = (0x[0-9a-f]+|\d+);").unwrap();
    let flag_re = Regex::new(r"\*\(undefined2 \*\)\(.*\) = (\d+);").unwrap();
    let call_re = Regex::new(r"FUN_14014e850\(([^;]*?)\);").unwrap();
    let mut vars = HashMap::new();
    let (mut name, mut packet_types, mut flag_lens) = (Value::Null, Vec::new(), Vec::new());
    for raw in code.lines() {
        let line = raw.trim();
        if let Some(c) = assign.captures(line).filter(|_| !line.contains("FUN_")) {
            vars.insert(c[1].to_string(), c[2].to_string());
        }
        if let Some(c) = name_re.captures(line) {
            name = json!(c[1].to_string());
        }
        if let Some(c) = ptype_re.captures(line) {
            packet_types.extend(num(&c[1]));
        }
        if let Some(c) = flag_re.captures(line) {
            flag_lens.push(json!({ "target": line.split(" = ").next().unwrap_or(""), "value": num(&c[1]) }));
        }
    }
    let joined: String = code.lines().map(str::trim).collect::<Vec<_>>().join(" ");
    let mut field_calls = Vec::new();
    for c in call_re.captures_iter(&joined) {
        let args = split_args(&c[1]);
        let target = args[0].trim().to_string();
        let vals: Vec<Arg> = args[1..].iter().map(|a| resolve(a, &vars, 0)).collect();
        let (mut fields, mut pretty, mut unresolved) = (Vec::new(), Vec::new(), false);
        let mut i = 0;
        while i + 1 < vals.len() {
            if let Arg::N(0xff) = vals[i] {
                break;
            }
            match (&vals[i], &vals[i + 1]) {
                (Arg::N(t), Arg::N(s)) => {
                    fields.push(json!([t, s]));
                    let n = TYPE_NAMES.get(*t as usize).map(|n| n.to_string()).unwrap_or(t.to_string());
                    pretty.push(if *s != 1 { format!("{n}[{s}]") } else { n });
                }
                (a, b) => {
                    unresolved = true;
                    let show = |x: &Arg| match x {
                        Arg::N(n) => n.to_string(),
                        Arg::Raw(r) => r.clone(),
                    };
                    fields.push(json!([show(a), show(b)]));
                    pretty.push(format!("?{}:{}", show(a), show(b)));
                }
            }
            i += 2;
        }
        field_calls.push(json!({ "target": target, "fields": fields, "unresolved": unresolved, "pretty": pretty.join(" ") }));
    }
    json!({ "name": name, "packetTypes": packet_types, "flagLens": flag_lens, "fieldCalls": field_calls })
}

fn extract(files: &[String]) -> anyhow::Result<()> {
    let head_re = Regex::new(r"FUN_[0-9a-f]+\(").unwrap();
    let mut modules: Vec<Value> = Vec::new();
    for f in files {
        let d: Value = serde_json::from_str(&std::fs::read_to_string(f)?)?;
        for it in d["normalized_result"]["items"].as_array().cloned().unwrap_or_default() {
            let code = it["pseudocode"].as_str().or_else(|| it["pseudocode"]["value"].as_str()).unwrap_or("").to_string();
            let head: String = code.lines().find(|l| head_re.is_match(l)).unwrap_or("").trim().chars().take(60).collect();
            let mut m = json!({ "address": it["address"], "fn": head });
            if let (Value::Object(o), Value::Object(info)) = (&mut m, parse_function(&code)) {
                o.extend(info);
            }
            modules.push(m);
        }
    }
    modules.sort_by_key(|m| m["packetTypes"][0].as_i64().unwrap_or(999));
    let out = crate::config::root().join("data").join("re").join("packet-schemas.json");
    std::fs::write(&out, serde_json::to_string_pretty(&modules)?)?;
    for m in &modules {
        let types: Vec<String> = m["packetTypes"].as_array().unwrap().iter().map(|t| format!("0x{:x}", t.as_i64().unwrap_or(0))).collect();
        let flags: Vec<String> = m["flagLens"].as_array().unwrap().iter().map(|f| f["value"].to_string()).collect();
        println!("== {} @{} types=[{}] flagLens=[{}]", m["name"].as_str().unwrap_or("(?)"), m["address"], types.join(","), flags.join(","));
        for (i, fc) in m["fieldCalls"].as_array().unwrap().iter().enumerate() {
            println!("   #{i} {} {}", if fc["unresolved"].as_bool() == Some(true) { "(!)" } else { "   " }, fc["pretty"].as_str().unwrap_or(""));
        }
    }
    println!("→ {}", out.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn releve_d_un_constructeur() {
        let code = "void FUN_1(longlong p)\n{\n  uVar1 = 5;\n  strncpy(_Dest,\"AgpmTest\",0x27);\n  *(undefined4 *)(p + 0x1264) = 0x32;\n  FUN_14014e850(p + 0x10,1,1,CONCAT44(x,uVar1),1,0xff);\n}";
        let v = super::parse_function(code);
        assert_eq!(v["name"], "AgpmTest");
        assert_eq!(v["packetTypes"][0], 0x32);
        assert_eq!(v["fieldCalls"][0]["pretty"], "INT8 INT32");
    }
}
