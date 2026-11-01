//! Le peu d'HTTP/1.1 dont SOAP a besoin : une requête par connexion (`Connection: close`, comme gSOAP), en-têtes
//! bornés, corps par `Content-Length` ou `Transfer-Encoding: chunked`, `Expect: 100-continue`, authentification Basic.

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// Taille maximale de la ligne de requête et des en-têtes.
pub const MAX_HEAD: usize = 16 * 1024;

#[derive(Debug)]
pub struct Request {
    pub method: String,
    pub target: String,
    /// En-têtes dans l'ordre reçu (noms tels quels ; `header` les cherche sans tenir compte de la casse).
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Request {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }
}

#[derive(Debug)]
pub enum HttpError {
    /// Connexion fermée avant la fin de la requête (rien à répondre).
    Closed,
    BadRequest(&'static str),
    HeadTooLarge,
    BodyTooLarge,
    LengthRequired,
    Io(std::io::Error),
}

impl HttpError {
    /// Statut HTTP à répondre (`None` : connexion déjà fermée).
    pub fn status(&self) -> Option<u16> {
        match self {
            HttpError::Closed | HttpError::Io(_) => None,
            HttpError::BadRequest(_) => Some(400),
            HttpError::HeadTooLarge => Some(431),
            HttpError::BodyTooLarge => Some(413),
            HttpError::LengthRequired => Some(411),
        }
    }
    pub fn message(&self) -> String {
        match self {
            HttpError::Closed => "connexion fermée".into(),
            HttpError::Io(e) => format!("lecture : {e}"),
            HttpError::BadRequest(m) => format!("requête HTTP invalide : {m}"),
            HttpError::HeadTooLarge => format!("en-têtes trop longs (plus de {MAX_HEAD} octets)"),
            HttpError::BodyTooLarge => "corps de la requête trop long".into(),
            HttpError::LengthRequired => "Content-Length manquant".into(),
        }
    }
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

/// Lit des octets en plus dans `buf` ; `Closed` si la connexion est fermée.
async fn fill<S: AsyncRead + Unpin>(stream: &mut S, buf: &mut Vec<u8>) -> Result<(), HttpError> {
    let mut chunk = [0u8; 4096];
    let n = stream.read(&mut chunk).await.map_err(HttpError::Io)?;
    if n == 0 {
        return Err(HttpError::Closed);
    }
    buf.extend_from_slice(&chunk[..n]);
    Ok(())
}

/// Décode un corps `chunked` complet au début de `data` : `Ok(None)` s'il manque des octets.
pub fn dechunk(data: &[u8], max: usize) -> Result<Option<Vec<u8>>, HttpError> {
    let mut body = Vec::new();
    let mut pos = 0;
    loop {
        let Some(eol) = find(&data[pos..], b"\r\n") else {
            return if data.len() - pos > 64 { Err(HttpError::BadRequest("taille de bloc illisible")) } else { Ok(None) };
        };
        let line = std::str::from_utf8(&data[pos..pos + eol]).map_err(|_| HttpError::BadRequest("taille de bloc illisible"))?;
        let size_hex = line.split(';').next().unwrap_or("").trim();
        let size = usize::from_str_radix(size_hex, 16).map_err(|_| HttpError::BadRequest("taille de bloc illisible"))?;
        pos += eol + 2;
        if size == 0 {
            // Fin : en-têtes de fin éventuels, puis une ligne vide.
            loop {
                let Some(eol) = find(&data[pos..], b"\r\n") else { return Ok(None) };
                pos += eol + 2;
                if eol == 0 {
                    return Ok(Some(body));
                }
            }
        }
        if body.len() + size > max {
            return Err(HttpError::BodyTooLarge);
        }
        if data.len() < pos + size + 2 {
            return Ok(None);
        }
        body.extend_from_slice(&data[pos..pos + size]);
        if &data[pos + size..pos + size + 2] != b"\r\n" {
            return Err(HttpError::BadRequest("bloc mal terminé"));
        }
        pos += size + 2;
    }
}

/// Lit une requête : ligne de requête, en-têtes, puis le corps d'un POST (au plus `max_body` octets).
pub async fn read_request<S: AsyncRead + AsyncWrite + Unpin>(stream: &mut S, max_body: usize) -> Result<Request, HttpError> {
    let mut buf = Vec::with_capacity(4096);
    let (head_end, sep) = loop {
        if let Some(i) = find(&buf, b"\r\n\r\n") {
            break (i, 4);
        }
        if let Some(i) = find(&buf, b"\n\n") {
            break (i, 2);
        }
        if buf.len() > MAX_HEAD {
            return Err(HttpError::HeadTooLarge);
        }
        fill(stream, &mut buf).await?;
    };
    if head_end > MAX_HEAD {
        return Err(HttpError::HeadTooLarge);
    }
    let head = String::from_utf8_lossy(&buf[..head_end]).into_owned();
    let mut lines = head.split('\n').map(|l| l.trim_end_matches('\r'));
    let mut first = lines.next().unwrap_or("").split_whitespace();
    let (Some(method), Some(target), Some(version)) = (first.next(), first.next(), first.next()) else { return Err(HttpError::BadRequest("ligne de requête")) };
    if !version.starts_with("HTTP/1.") {
        return Err(HttpError::BadRequest("version HTTP"));
    }
    let mut headers = Vec::new();
    for line in lines {
        let Some((k, v)) = line.split_once(':') else { return Err(HttpError::BadRequest("en-tête sans « : »")) };
        headers.push((k.trim().to_string(), v.trim().to_string()));
    }
    let mut req = Request { method: method.to_string(), target: target.to_string(), headers, body: Vec::new() };
    if !req.method.eq_ignore_ascii_case("POST") {
        return Ok(req);
    }
    let mut rest = buf.split_off(head_end + sep);
    if req.header("Expect").is_some_and(|e| e.eq_ignore_ascii_case("100-continue")) {
        stream.write_all(b"HTTP/1.1 100 Continue\r\n\r\n").await.map_err(HttpError::Io)?;
    }
    if req.header("Transfer-Encoding").is_some_and(|t| t.to_ascii_lowercase().contains("chunked")) {
        loop {
            if let Some(body) = dechunk(&rest, max_body)? {
                req.body = body;
                return Ok(req);
            }
            if rest.len() > max_body + MAX_HEAD {
                return Err(HttpError::BodyTooLarge);
            }
            fill(stream, &mut rest).await?;
        }
    }
    let Some(len) = req.header("Content-Length") else { return Err(HttpError::LengthRequired) };
    let len: usize = len.parse().map_err(|_| HttpError::BadRequest("Content-Length"))?;
    if len > max_body {
        return Err(HttpError::BodyTooLarge);
    }
    while rest.len() < len {
        fill(stream, &mut rest).await?;
    }
    rest.truncate(len);
    req.body = rest;
    Ok(req)
}

pub fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        405 => "Method Not Allowed",
        408 => "Request Timeout",
        411 => "Length Required",
        413 => "Payload Too Large",
        431 => "Request Header Fields Too Large",
        500 => "Internal Server Error",
        503 => "Service Unavailable",
        _ => "Error",
    }
}

/// Réponse complète, connexion fermée ensuite.
pub fn response(status: u16, extra: &[(&str, &str)], content_type: &str, body: &[u8]) -> Vec<u8> {
    let mut head = format!("HTTP/1.1 {status} {}\r\nServer: ArchCore/{}\r\n", reason(status), env!("CARGO_PKG_VERSION"));
    for (k, v) in extra {
        head.push_str(&format!("{k}: {v}\r\n"));
    }
    head.push_str(&format!("Content-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()));
    let mut out = head.into_bytes();
    out.extend_from_slice(body);
    out
}

/// Décodage base64 (alphabet standard, `=` facultatifs, blancs ignorés).
pub fn base64_decode(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0u32);
    let mut padding = false;
    for c in s.bytes().filter(|c| !c.is_ascii_whitespace()) {
        if c == b'=' {
            padding = true;
            continue;
        }
        if padding {
            return None;
        }
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        };
        acc = (acc << 6) | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

/// Texte reçu : UTF-8, sinon Windows-1252 (clients qui envoient l'ancienne page de code).
pub fn text_of(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_string(),
        Err(_) => encoding_rs::WINDOWS_1252.decode(bytes).0.into_owned(),
    }
}

/// `Authorization: Basic …` → (utilisateur, mot de passe) ; le mot de passe peut contenir des « : ».
pub fn basic_auth(header: Option<&str>) -> Option<(String, String)> {
    let h = header?.trim();
    let (scheme, value) = h.split_once(' ')?;
    if !scheme.eq_ignore_ascii_case("basic") {
        return None;
    }
    let raw = base64_decode(value.trim())?;
    let text = text_of(&raw);
    let (user, pass) = text.split_once(':')?;
    (!user.is_empty()).then(|| (user.to_string(), pass.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authentification_basic() {
        // « admin:mot:de passe » ; curl -u admin:admin envoie YWRtaW46YWRtaW4=.
        assert_eq!(basic_auth(Some("Basic YWRtaW46YWRtaW4=")), Some(("admin".into(), "admin".into())));
        assert_eq!(basic_auth(Some("basic YWRtaW46bW90OmRlIHBhc3Nl")), Some(("admin".into(), "mot:de passe".into())));
        // Sans remplissage, UTF-8 (« éa:b ») et Windows-1252 (« é:b » en un octet).
        assert_eq!(basic_auth(Some("Basic w6lhOmI")), Some(("éa".into(), "b".into())));
        assert_eq!(basic_auth(Some("Basic 6Tpi")), Some(("é".into(), "b".into())));
        assert_eq!(basic_auth(Some("Bearer YWRtaW46YWRtaW4=")), None);
        assert_eq!(basic_auth(Some("Basic !!!")), None);
        assert_eq!(basic_auth(Some("Basic OnNlY3JldA==")), None, "utilisateur vide");
        assert_eq!(basic_auth(Some("Basic YWRtaW4=")), None, "sans « : »");
        assert_eq!(basic_auth(None), None);
        assert_eq!(base64_decode("QQ==").unwrap(), b"A");
        assert_eq!(base64_decode("QQ=x"), None);
    }

    #[test]
    fn corps_chunked() {
        assert_eq!(dechunk(b"4\r\nWiki\r\n5;ext=1\r\npedia\r\n0\r\n\r\n", 100).unwrap().unwrap(), b"Wikipedia");
        assert!(dechunk(b"4\r\nWiki\r\n", 100).unwrap().is_none(), "incomplet");
        assert!(matches!(dechunk(b"10\r\n0123456789abcdef\r\n0\r\n\r\n", 8), Err(HttpError::BodyTooLarge)));
        assert!(matches!(dechunk(b"zz\r\n", 100), Err(HttpError::BadRequest(_))));
        assert_eq!(dechunk(b"0\r\nX-Fin: 1\r\n\r\n", 10).unwrap().unwrap(), b"");
    }

    #[tokio::test]
    async fn lecture_d_une_requete() {
        let (mut client, mut server) = tokio::io::duplex(1 << 16);
        client.write_all(b"POST / HTTP/1.1\r\nHost: x\r\ncontent-length: 5\r\n\r\nbonjour").await.unwrap();
        let r = read_request(&mut server, 1024).await.unwrap();
        assert_eq!((r.method.as_str(), r.target.as_str(), r.header("Content-Length"), r.body.as_slice()), ("POST", "/", Some("5"), &b"bonjo"[..]));

        let (mut client, mut server) = tokio::io::duplex(1 << 16);
        client.write_all(b"POST / HTTP/1.1\r\nContent-Length: 2000\r\n\r\n").await.unwrap();
        assert!(matches!(read_request(&mut server, 1024).await, Err(HttpError::BodyTooLarge)));

        let (mut client, mut server) = tokio::io::duplex(1 << 16);
        client.write_all(b"POST / HTTP/1.1\r\nHost: x\r\n\r\n").await.unwrap();
        assert!(matches!(read_request(&mut server, 1024).await, Err(HttpError::LengthRequired)));

        let (mut client, mut server) = tokio::io::duplex(1 << 16);
        client.write_all(b"GET /wsdl HTTP/1.0\r\n\r\n").await.unwrap();
        assert_eq!(read_request(&mut server, 1024).await.unwrap().method, "GET");

        let (mut client, mut server) = tokio::io::duplex(1 << 16);
        client.write_all(b"BONJOUR\r\n\r\n").await.unwrap();
        assert!(matches!(read_request(&mut server, 1024).await, Err(HttpError::BadRequest(_))));

        let (client, mut server) = tokio::io::duplex(1 << 16);
        drop(client);
        assert!(matches!(read_request(&mut server, 1024).await, Err(HttpError::Closed)));
    }

    #[test]
    fn reponse_http() {
        let r = String::from_utf8(response(401, &[("WWW-Authenticate", "Basic realm=\"gSOAP Web Service\"")], "text/xml; charset=utf-8", b"<x/>")).unwrap();
        assert!(r.starts_with("HTTP/1.1 401 Unauthorized\r\nServer: ArchCore/"));
        assert!(r.contains("\r\nWWW-Authenticate: Basic realm=\"gSOAP Web Service\"\r\nContent-Type: text/xml; charset=utf-8\r\nContent-Length: 4\r\nConnection: close\r\n\r\n<x/>"));
    }
}
