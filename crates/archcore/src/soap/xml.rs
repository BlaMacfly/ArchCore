//! Enveloppes SOAP 1.1 de la méthode `executeCommand`, au format de gSOAP (AzerothCore, TrinityCore).
//!
//! * Lecture : un petit lecteur XML, tolérant sur la forme (préfixes quelconques, espace de noms par défaut, CDATA,
//!   commentaires, en-tête `SOAP-ENV:Header`) mais strict sur l'imbrication. Les DTD sont refusées : pas d'entités
//!   définies par le client, seulement les cinq entités XML et les références numériques.
//! * Écriture : réponse `ns1:executeCommandResponse` / `result` et `SOAP-ENV:Fault`, avec les mêmes espaces de noms que
//!   gSOAP (`ns1` reprend celui de la requête : `urn:AC`, `urn:TC`, `urn:ArchCore`…).

/// Espace de noms de l'enveloppe SOAP 1.1.
pub const ENV_NS: &str = "http://schemas.xmlsoap.org/soap/envelope/";
/// Codage SOAP 1.1 (style rpc/encoded de gSOAP).
pub const ENC_NS: &str = "http://schemas.xmlsoap.org/soap/encoding/";
/// `xsi` et `xsd` par défaut : ceux de la table des espaces de noms d'AzerothCore et de TrinityCore. Une requête qui en
/// déclare d'autres (2001) les retrouve dans la réponse, comme gSOAP le fait.
pub const XSI_NS: &str = "http://www.w3.org/1999/XMLSchema-instance";
pub const XSD_NS: &str = "http://www.w3.org/1999/XMLSchema";
/// `ns1` d'une requête qui n'en donne pas (méthode sans espace de noms).
pub const DEFAULT_NS: &str = "urn:AC";

/// Ce qu'une requête `executeCommand` porte.
#[derive(Debug, Default, PartialEq)]
pub struct Call {
    /// Espace de noms de l'élément `executeCommand` (`urn:AC`, `urn:TC`…), s'il en a un.
    pub namespace: Option<String>,
    /// Texte de l'élément `command`, entités décodées ; `None` s'il est absent.
    pub command: Option<String>,
    /// `xsi` et `xsd` déclarés par la requête, repris dans la réponse.
    pub xsi: Option<String>,
    pub xsd: Option<String>,
}

#[derive(Debug, PartialEq)]
pub enum ParseError {
    /// XML mal formé, ou pas une enveloppe SOAP.
    Malformed(String),
    /// Méthode autre que `executeCommand` (nom qualifié reçu, vide s'il n'y en a pas).
    UnknownMethod(String),
}

impl ParseError {
    /// Texte du `faultstring`, comme gSOAP pour une méthode inconnue.
    pub fn message(&self) -> String {
        match self {
            ParseError::Malformed(m) => format!("XML invalide : {m}"),
            ParseError::UnknownMethod(m) => format!("Method '{m}' not implemented: method name or namespace not recognized"),
        }
    }
}

fn bad<T>(m: impl Into<String>) -> Result<T, ParseError> {
    Err(ParseError::Malformed(m.into()))
}

enum Event<'a> {
    Start { name: &'a str, attrs: Vec<(&'a str, String)>, empty: bool },
    End(&'a str),
    Text(String),
}

struct Reader<'a> {
    s: &'a str,
    pos: usize,
}

fn is_name_end(c: char) -> bool {
    c.is_whitespace() || c == '/' || c == '>' || c == '='
}

impl<'a> Reader<'a> {
    fn skip_ws(&mut self) {
        let rest = &self.s[self.pos..];
        self.pos += rest.len() - rest.trim_start().len();
    }

    fn name(&mut self) -> Result<&'a str, ParseError> {
        let rest = &self.s[self.pos..];
        let n = rest.find(is_name_end).unwrap_or(rest.len());
        if n == 0 {
            return bad("nom de balise ou d'attribut attendu");
        }
        self.pos += n;
        Ok(&rest[..n])
    }

    fn next(&mut self) -> Result<Option<Event<'a>>, ParseError> {
        loop {
            let rest = &self.s[self.pos..];
            if rest.is_empty() {
                return Ok(None);
            }
            if let Some(r) = rest.strip_prefix("<!--") {
                let Some(end) = r.find("-->") else { return bad("commentaire non fermé") };
                self.pos += 4 + end + 3;
                continue;
            }
            if let Some(r) = rest.strip_prefix("<![CDATA[") {
                let Some(end) = r.find("]]>") else { return bad("section CDATA non fermée") };
                self.pos += 9 + end + 3;
                return Ok(Some(Event::Text(r[..end].to_string())));
            }
            if rest.starts_with("<!") {
                return bad("DTD non permise");
            }
            if let Some(r) = rest.strip_prefix("<?") {
                let Some(end) = r.find("?>") else { return bad("instruction de traitement non fermée") };
                self.pos += 2 + end + 2;
                continue;
            }
            if let Some(r) = rest.strip_prefix("</") {
                let Some(end) = r.find('>') else { return bad("balise fermante non terminée") };
                self.pos += 2 + end + 1;
                return Ok(Some(Event::End(r[..end].trim_end())));
            }
            if rest.starts_with('<') {
                self.pos += 1;
                let name = self.name()?;
                let mut attrs = Vec::new();
                loop {
                    self.skip_ws();
                    let rest = &self.s[self.pos..];
                    if rest.starts_with("/>") {
                        self.pos += 2;
                        return Ok(Some(Event::Start { name, attrs, empty: true }));
                    }
                    if rest.starts_with('>') {
                        self.pos += 1;
                        return Ok(Some(Event::Start { name, attrs, empty: false }));
                    }
                    if rest.is_empty() {
                        return bad(format!("balise {name} non terminée"));
                    }
                    let key = self.name()?;
                    self.skip_ws();
                    if !self.s[self.pos..].starts_with('=') {
                        return bad(format!("« = » attendu après l'attribut {key}"));
                    }
                    self.pos += 1;
                    self.skip_ws();
                    let rest = &self.s[self.pos..];
                    let Some(quote) = rest.chars().next().filter(|c| *c == '"' || *c == '\'') else { return bad(format!("valeur de l'attribut {key} sans guillemets")) };
                    let Some(end) = rest[1..].find(quote) else { return bad(format!("valeur de l'attribut {key} non fermée")) };
                    attrs.push((key, decode_entities(&rest[1..1 + end])?));
                    self.pos += end + 2;
                }
            }
            let n = rest.find('<').unwrap_or(rest.len());
            self.pos += n;
            return Ok(Some(Event::Text(decode_entities(&rest[..n])?)));
        }
    }
}

/// Décode les entités XML : `&lt;` `&gt;` `&amp;` `&quot;` `&apos;` et les références numériques `&#…;` / `&#x…;`.
pub fn decode_entities(s: &str) -> Result<String, ParseError> {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        let after = &rest[i + 1..];
        let Some(end) = after.find(';').filter(|&e| e <= 10) else { return bad("entité non terminée") };
        let ent = &after[..end];
        let c = match ent {
            "lt" => '<',
            "gt" => '>',
            "amp" => '&',
            "quot" => '"',
            "apos" => '\'',
            _ => {
                let code = if let Some(h) = ent.strip_prefix("#x").or_else(|| ent.strip_prefix("#X")) {
                    u32::from_str_radix(h, 16).ok()
                } else if let Some(d) = ent.strip_prefix('#') {
                    d.parse::<u32>().ok()
                } else {
                    None
                };
                match code.and_then(char::from_u32) {
                    Some(c) => c,
                    None => return bad(format!("entité inconnue : &{ent};")),
                }
            }
        };
        out.push(c);
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

fn local_name(qname: &str) -> (&str, &str) {
    qname.split_once(':').unwrap_or(("", qname))
}

/// Lit une requête SOAP `executeCommand` : enveloppe, corps, méthode, puis son élément `command`.
pub fn parse_request(body: &str) -> Result<Call, ParseError> {
    let body = body.strip_prefix('\u{feff}').unwrap_or(body);
    let mut r = Reader { s: body, pos: 0 };
    // Éléments ouverts : nom qualifié et déclarations d'espaces de noms (préfixe, URI).
    let mut stack: Vec<(&str, Vec<(String, String)>)> = Vec::new();
    let mut call = Call::default();
    let mut in_body = false;
    let mut method = false;
    let mut capture: Option<String> = None;
    let mut seen_root = false;
    let resolve = |stack: &[(&str, Vec<(String, String)>)], prefix: &str| -> Option<String> {
        stack.iter().rev().find_map(|(_, decls)| decls.iter().find(|(p, _)| p == prefix).map(|(_, uri)| uri.clone())).filter(|u| !u.is_empty())
    };
    while let Some(ev) = r.next()? {
        match ev {
            Event::Text(t) => {
                if stack.is_empty() && !t.trim().is_empty() {
                    return bad("texte hors de l'enveloppe");
                }
                if let Some(c) = capture.as_mut() {
                    if stack.len() == 4 {
                        c.push_str(&t);
                    }
                }
            }
            Event::Start { name, attrs, empty } => {
                if stack.is_empty() && seen_root {
                    return bad("plusieurs éléments racine");
                }
                seen_root = true;
                let decls: Vec<(String, String)> = attrs
                    .iter()
                    .filter_map(|(k, v)| if *k == "xmlns" { Some((String::new(), v.clone())) } else { k.strip_prefix("xmlns:").map(|p| (p.to_string(), v.clone())) })
                    .collect();
                for (_, uri) in &decls {
                    if uri.ends_with("/XMLSchema-instance") {
                        call.xsi.get_or_insert_with(|| uri.clone());
                    } else if uri.ends_with("/XMLSchema") {
                        call.xsd.get_or_insert_with(|| uri.clone());
                    }
                }
                stack.push((name, decls));
                let depth = stack.len();
                let (prefix, local) = local_name(name);
                match depth {
                    1 if local != "Envelope" => return bad(format!("enveloppe SOAP attendue, reçu <{name}>")),
                    2 if local == "Body" => in_body = true,
                    3 if in_body && !method => {
                        if local != "executeCommand" {
                            return Err(ParseError::UnknownMethod(name.to_string()));
                        }
                        method = true;
                        call.namespace = resolve(&stack, prefix);
                    }
                    4 if method && local == "command" && call.command.is_none() && capture.is_none() => {
                        capture = Some(String::new());
                    }
                    _ => {}
                }
                if empty {
                    if depth == 4 {
                        if let Some(c) = capture.take() {
                            call.command = Some(c);
                        }
                    }
                    stack.pop();
                }
            }
            Event::End(name) => {
                let Some((open, _)) = stack.pop() else { return bad(format!("balise </{name}> sans ouverture")) };
                if open != name {
                    return bad(format!("balise </{name}> à la place de </{open}>"));
                }
                match stack.len() {
                    3 => {
                        if let Some(c) = capture.take() {
                            call.command = Some(c);
                        }
                    }
                    1 if local_name(open).1 == "Body" => in_body = false,
                    _ => {}
                }
            }
        }
    }
    if !stack.is_empty() {
        return bad("document incomplet");
    }
    if !seen_root {
        return bad("document vide");
    }
    if !method {
        return Err(ParseError::UnknownMethod(String::new()));
    }
    Ok(call)
}

/// Texte d'élément échappé comme gSOAP : `&`, `<`, `>`, retour chariot en `&#xD;` ; les caractères de contrôle
/// interdits en XML 1.0 sont retirés.
pub fn escape_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 16);
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '\r' => out.push_str("&#xD;"),
            '\t' | '\n' => out.push(c),
            c if (c as u32) < 0x20 || c == '\u{fffe}' || c == '\u{ffff}' => {}
            c => out.push(c),
        }
    }
    out
}

fn escape_attr(s: &str) -> String {
    escape_text(s).replace('"', "&quot;")
}

/// Espaces de noms de la réponse : ceux de la requête quand elle en donne.
#[derive(Debug, Clone, PartialEq)]
pub struct Namespaces {
    pub ns1: String,
    pub xsi: String,
    pub xsd: String,
}

impl Default for Namespaces {
    fn default() -> Self {
        Namespaces { ns1: DEFAULT_NS.into(), xsi: XSI_NS.into(), xsd: XSD_NS.into() }
    }
}

impl Namespaces {
    pub fn of(call: &Call) -> Self {
        let d = Namespaces::default();
        Namespaces { ns1: call.namespace.clone().unwrap_or(d.ns1), xsi: call.xsi.clone().unwrap_or(d.xsi), xsd: call.xsd.clone().unwrap_or(d.xsd) }
    }
}

fn envelope(ns: &Namespaces, body: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<SOAP-ENV:Envelope xmlns:SOAP-ENV=\"{ENV_NS}\" xmlns:SOAP-ENC=\"{ENC_NS}\" xmlns:xsi=\"{}\" xmlns:xsd=\"{}\" xmlns:ns1=\"{}\">{body}</SOAP-ENV:Envelope>\n",
        escape_attr(&ns.xsi),
        escape_attr(&ns.xsd),
        escape_attr(&ns.ns1)
    )
}

/// Réponse d'une commande réussie : `<ns1:executeCommandResponse><result>…</result>`.
pub fn response(ns: &Namespaces, result: &str) -> String {
    envelope(
        ns,
        &format!("<SOAP-ENV:Body SOAP-ENV:encodingStyle=\"{ENC_NS}\"><ns1:executeCommandResponse><result>{}</result></ns1:executeCommandResponse></SOAP-ENV:Body>", escape_text(result)),
    )
}

/// `SOAP-ENV:Fault` : code (`SOAP-ENV:Client` ou `SOAP-ENV:Server`), message et détail (comme `soap_sender_fault`).
pub fn fault(ns: &Namespaces, code: &str, message: &str, detail: Option<&str>) -> String {
    let detail = detail.map(|d| format!("<detail>{}</detail>", escape_text(d))).unwrap_or_default();
    envelope(
        ns,
        &format!("<SOAP-ENV:Body><SOAP-ENV:Fault><faultcode>{code}</faultcode><faultstring>{}</faultstring>{detail}</SOAP-ENV:Fault></SOAP-ENV:Body>", escape_text(message)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Requête d'un client AzerothCore (acore-soap, panneaux web) : préfixes de gSOAP, espaces de noms 1999.
    const AZEROTHCORE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<SOAP-ENV:Envelope xmlns:SOAP-ENV="http://schemas.xmlsoap.org/soap/envelope/" xmlns:SOAP-ENC="http://schemas.xmlsoap.org/soap/encoding/" xmlns:xsi="http://www.w3.org/1999/XMLSchema-instance" xmlns:xsd="http://www.w3.org/1999/XMLSchema" xmlns:ns1="urn:AC"><SOAP-ENV:Body><ns1:executeCommand><command>.server info</command></ns1:executeCommand></SOAP-ENV:Body></SOAP-ENV:Envelope>"#;

    /// Requête de PHP SoapClient (exemple du wiki de TrinityCore : `uri` urn:TC, `SoapParam($command, 'command')`).
    const TRINITYCORE_PHP: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<SOAP-ENV:Envelope xmlns:SOAP-ENV="http://schemas.xmlsoap.org/soap/envelope/" xmlns:ns1="urn:TC" xmlns:xsd="http://www.w3.org/2001/XMLSchema" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xmlns:SOAP-ENC="http://schemas.xmlsoap.org/soap/encoding/" SOAP-ENV:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/"><SOAP-ENV:Body><ns1:executeCommand><command xsi:type="xsd:string">account create Joueur &quot;mdp&quot; &lt;x&gt; &amp; &#233;&#xE9;</command></ns1:executeCommand></SOAP-ENV:Body></SOAP-ENV:Envelope>"#;

    /// Charge utile écrite à la main du wiki de TrinityCore (Postman, cURL) : indentée, fins de ligne CRLF.
    const TRINITYCORE_RAW: &str = "<?xml version=\"1.0\" encoding=\"utf-8\"?>\r\n<SOAP-ENV:Envelope xmlns:SOAP-ENV=\"http://schemas.xmlsoap.org/soap/envelope/\" xmlns:ns1=\"urn:TC\">\r\n<SOAP-ENV:Body>\r\n<ns1:executeCommand>\r\n<command>server info</command>\r\n</ns1:executeCommand>\r\n</SOAP-ENV:Body>\r\n</SOAP-ENV:Envelope>";

    #[test]
    fn enveloppes_azerothcore_et_trinitycore() {
        let c = parse_request(AZEROTHCORE).unwrap();
        assert_eq!(c.namespace.as_deref(), Some("urn:AC"));
        assert_eq!(c.command.as_deref(), Some(".server info"));
        assert_eq!(Namespaces::of(&c), Namespaces::default(), "xsi et xsd de 1999, comme la table de gSOAP");

        let c = parse_request(TRINITYCORE_RAW).unwrap();
        assert_eq!((c.namespace.as_deref(), c.command.as_deref()), (Some("urn:TC"), Some("server info")));
        assert_eq!(c.xsi, None);

        let c = parse_request(TRINITYCORE_PHP).unwrap();
        assert_eq!(c.command.as_deref(), Some("account create Joueur \"mdp\" <x> & éé"));
        let ns = Namespaces::of(&c);
        assert_eq!((ns.ns1.as_str(), ns.xsi.as_str(), ns.xsd.as_str()), ("urn:TC", "http://www.w3.org/2001/XMLSchema-instance", "http://www.w3.org/2001/XMLSchema"));
    }

    #[test]
    fn variantes_de_forme() {
        // Espace de noms par défaut, préfixe soap:, en-tête, commentaire, CDATA, urn:ArchCore.
        let x = r#"<?xml version="1.0"?><!-- client .NET --><soap:Envelope xmlns:soap="http://schemas.xmlsoap.org/soap/envelope/"><soap:Header><a:Jeton xmlns:a="urn:x">1</a:Jeton></soap:Header><soap:Body><executeCommand xmlns="urn:ArchCore"><command><![CDATA[announce <b>Bonjour</b> & bienvenue]]></command></executeCommand></soap:Body></soap:Envelope>"#;
        let c = parse_request(x).unwrap();
        assert_eq!((c.namespace.as_deref(), c.command.as_deref()), (Some("urn:ArchCore"), Some("announce <b>Bonjour</b> & bienvenue")));
        // Sans espace de noms, élément command vide ou absent.
        let c = parse_request("<Envelope><Body><executeCommand><command/></executeCommand></Body></Envelope>").unwrap();
        assert_eq!((c.namespace, c.command.as_deref()), (None, Some("")));
        let c = parse_request("<e:Envelope xmlns:e=\"x\"><e:Body><m:executeCommand xmlns:m=\"urn:AC\"/></e:Body></e:Envelope>").unwrap();
        assert_eq!((c.namespace.as_deref(), c.command), (Some("urn:AC"), None));
    }

    #[test]
    fn requetes_refusees() {
        let m = |x: &str| parse_request(x).unwrap_err();
        assert_eq!(m("<E:Envelope xmlns:E=\"x\"><E:Body><ns1:getVersion xmlns:ns1=\"urn:AC\"/></E:Body></E:Envelope>"), ParseError::UnknownMethod("ns1:getVersion".into()));
        assert_eq!(m("<Envelope><Body/></Envelope>"), ParseError::UnknownMethod(String::new()));
        assert!(m("<!DOCTYPE x [<!ENTITY a \"b\">]><Envelope/>").message().contains("DTD"));
        assert!(matches!(m("<Envelope><Body><executeCommand><command>x</cmd></executeCommand></Body></Envelope>"), ParseError::Malformed(_)));
        assert!(matches!(m("<Envelope><Body>"), ParseError::Malformed(_)));
        assert!(matches!(m("<html><body>bonjour</body></html>"), ParseError::Malformed(_)));
        assert!(matches!(m(""), ParseError::Malformed(_)));
        assert!(matches!(m("<Envelope><Body><executeCommand><command>&nbsp;</command></executeCommand></Body></Envelope>"), ParseError::Malformed(_)));
        assert!(matches!(m("<Envelope a=b/>"), ParseError::Malformed(_)));
        assert!(matches!(m("<Envelope><Body><executeCommand><command>&#x E9;</command></executeCommand></Body></Envelope>"), ParseError::Malformed(_)));
    }

    #[test]
    fn echappement() {
        assert_eq!(escape_text("a < b && c > d\r\n\"x\"\u{1}"), "a &lt; b &amp;&amp; c &gt; d&#xD;\n\"x\"");
        assert_eq!(decode_entities(&escape_text("<é & \">")).unwrap(), "<é & \">");
        assert_eq!(escape_attr("urn:\"x\""), "urn:&quot;x&quot;");
    }

    #[test]
    fn reponses_au_format_de_gsoap() {
        let ns = Namespaces::default();
        assert_eq!(
            response(&ns, "Account created: test\r\n"),
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<SOAP-ENV:Envelope xmlns:SOAP-ENV=\"http://schemas.xmlsoap.org/soap/envelope/\" xmlns:SOAP-ENC=\"http://schemas.xmlsoap.org/soap/encoding/\" xmlns:xsi=\"http://www.w3.org/1999/XMLSchema-instance\" xmlns:xsd=\"http://www.w3.org/1999/XMLSchema\" xmlns:ns1=\"urn:AC\"><SOAP-ENV:Body SOAP-ENV:encodingStyle=\"http://schemas.xmlsoap.org/soap/encoding/\"><ns1:executeCommandResponse><result>Account created: test&#xD;\n</result></ns1:executeCommandResponse></SOAP-ENV:Body></SOAP-ENV:Envelope>\n"
        );
        let f = fault(&Namespaces { ns1: "urn:TC".into(), ..ns }, "SOAP-ENV:Client", "Commande inconnue : <x>", Some("Commande inconnue : <x>"));
        assert!(f.contains("xmlns:ns1=\"urn:TC\""));
        assert!(f.contains("<SOAP-ENV:Body><SOAP-ENV:Fault><faultcode>SOAP-ENV:Client</faultcode><faultstring>Commande inconnue : &lt;x&gt;</faultstring><detail>Commande inconnue : &lt;x&gt;</detail></SOAP-ENV:Fault></SOAP-ENV:Body>"));
        // Les réponses se relisent comme des enveloppes (sans méthode executeCommand).
        assert_eq!(parse_request(&f), Err(ParseError::UnknownMethod("SOAP-ENV:Fault".into())));
    }
}
