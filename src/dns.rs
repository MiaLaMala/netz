//! `netz dns`: own DNS client over UDP (no DNS crate), see issue #2.
//! Responses are untrusted input: every length is checked, compression
//! pointer jumps are capped, ID and question must match the query.

use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};
use std::net::{IpAddr, SocketAddr, UdpSocket};
use std::process::Command;
use std::time::{Duration, Instant};

use crate::explain;

pub const A: u16 = 1;
pub const CNAME: u16 = 5;
pub const MX: u16 = 15;
pub const TXT: u16 = 16;
pub const AAAA: u16 = 28;
pub const IN: u16 = 1;
pub const CH: u16 = 3;

const MAX_POINTER_JUMPS: u32 = 10;
const MAX_NAME_LEN: usize = 255;

/// One resource record of the answer section; `data` is already readable text.
#[derive(Debug, Clone, PartialEq)]
pub struct Record {
    pub name: String,
    pub rtype: u16,
    pub class: u16,
    pub ttl: u32,
    pub data: String,
    /// Target offset if the record name was a compression pointer.
    pub ptr: Option<usize>,
}

/// A parsed, validated response.
#[derive(Debug, Clone, PartialEq)]
pub struct Answer {
    pub rcode: u8,
    pub authoritative: bool,
    pub recursion_available: bool,
    pub records: Vec<Record>,
}

/// Builds a query packet: header (RD set), one question.
pub fn build_query(id: u16, name: &str, qtype: u16, qclass: u16) -> Result<Vec<u8>, String> {
    let mut p = Vec::with_capacity(32 + name.len());
    p.extend_from_slice(&id.to_be_bytes());
    p.extend_from_slice(&[0x01, 0x00, 0, 1, 0, 0, 0, 0, 0, 0]);
    let name = name.trim_end_matches('.');
    if name.is_empty() || name.len() > 253 || !name.is_ascii() {
        return Err(format!(
            "„{name}“ ist kein gültiger Name. Gib einen Namen wie github.com an."
        ));
    }
    for label in name.split('.') {
        if label.is_empty() || label.len() > 63 {
            return Err(format!(
                "„{name}“ ist kein gültiger Name. Jeder Teil zwischen den Punkten darf 1 bis 63 Zeichen haben."
            ));
        }
        p.push(label.len() as u8);
        p.extend_from_slice(label.as_bytes());
    }
    p.push(0);
    p.extend_from_slice(&qtype.to_be_bytes());
    p.extend_from_slice(&qclass.to_be_bytes());
    Ok(p)
}

fn truncated() -> String {
    "Die DNS-Antwort ist abgeschnitten oder kaputt. Versuch es noch einmal.".into()
}

fn u16_at(b: &[u8], i: usize) -> Result<u16, String> {
    match b.get(i..i + 2) {
        Some(s) => Ok(u16::from_be_bytes([s[0], s[1]])),
        None => Err(truncated()),
    }
}

/// Reads a (possibly compressed) name at `pos`; returns it and the offset after it
/// at the original position (not after the pointer target).
fn read_name(buf: &[u8], pos: usize) -> Result<(String, usize), String> {
    let (mut p, mut jumps, mut end) = (pos, 0, None);
    let mut name = String::new();
    loop {
        let len = *buf.get(p).ok_or_else(truncated)? as usize;
        match len & 0xC0 {
            0 if len == 0 => return Ok((name, end.unwrap_or(p + 1))),
            0 => {
                let label = buf.get(p + 1..p + 1 + len).ok_or_else(truncated)?;
                if !name.is_empty() {
                    name.push('.');
                }
                // untrusted bytes: keep only printable ASCII
                name.extend(
                    label
                        .iter()
                        .map(|&c| if c.is_ascii_graphic() { c as char } else { '?' }),
                );
                if name.len() > MAX_NAME_LEN {
                    return Err(truncated());
                }
                p += 1 + len;
            }
            0xC0 => {
                let target = (u16_at(buf, p)? & 0x3FFF) as usize;
                jumps += 1;
                if jumps > MAX_POINTER_JUMPS || target >= buf.len() {
                    return Err(
                        "Die DNS-Antwort enthält einen ungültigen Zeiger. Versuch es noch einmal."
                            .into(),
                    );
                }
                end.get_or_insert(p + 2);
                p = target;
            }
            _ => return Err(truncated()),
        }
    }
}

fn rdata_text(buf: &[u8], rtype: u16, start: usize, len: usize) -> Result<String, String> {
    let rd = buf.get(start..start + len).ok_or_else(truncated)?;
    Ok(match (rtype, len) {
        (A, 4) => IpAddr::from([rd[0], rd[1], rd[2], rd[3]]).to_string(),
        (AAAA, 16) => {
            let mut o = [0u8; 16];
            o.copy_from_slice(rd);
            IpAddr::from(o).to_string()
        }
        (CNAME, _) => read_name(buf, start)?.0,
        (MX, 3..) => format!("{} {}", u16_at(buf, start)?, read_name(buf, start + 2)?.0),
        (TXT, _) => {
            let (mut i, mut out) = (0, String::new());
            while i < rd.len() {
                let l = rd[i] as usize;
                let part = rd.get(i + 1..i + 1 + l).ok_or_else(truncated)?;
                out.push_str(&String::from_utf8_lossy(part));
                i += 1 + l;
            }
            out
        }
        (A | AAAA | MX, _) => return Err(truncated()),
        _ => format!("(Typ {rtype}, {len} Bytes)"),
    })
}

/// Parses a response and checks it belongs to the query (ID and question).
pub fn parse_response(
    buf: &[u8],
    id: u16,
    name: &str,
    qtype: u16,
    qclass: u16,
) -> Result<Answer, String> {
    let mismatch = || "Die DNS-Antwort passt nicht zur Anfrage und wird ignoriert.".to_string();
    if buf.len() < 12 {
        return Err(truncated());
    }
    let flags = u16_at(buf, 2)?;
    if u16_at(buf, 0)? != id || flags & 0x8000 == 0 || u16_at(buf, 4)? != 1 {
        return Err(mismatch());
    }
    let (qname, mut pos) = read_name(buf, 12)?;
    if !qname.eq_ignore_ascii_case(name.trim_end_matches('.'))
        || u16_at(buf, pos)? != qtype
        || u16_at(buf, pos + 2)? != qclass
    {
        return Err(mismatch());
    }
    if flags & 0x0200 != 0 {
        // ponytail: no TCP fallback; a truncated answer is an error. Add TCP when a stage needs big answers.
        return Err("Die DNS-Antwort war zu groß für UDP und wurde gekürzt.".into());
    }
    pos += 4;
    let mut records = Vec::new();
    for _ in 0..u16_at(buf, 6)? {
        let ptr = match buf.get(pos) {
            Some(b) if b & 0xC0 == 0xC0 => Some((u16_at(buf, pos)? & 0x3FFF) as usize),
            _ => None,
        };
        let (rname, after) = read_name(buf, pos)?;
        let rtype = u16_at(buf, after)?;
        let class = u16_at(buf, after + 2)?;
        let ttl = (u32::from(u16_at(buf, after + 4)?) << 16) | u32::from(u16_at(buf, after + 6)?);
        let len = u16_at(buf, after + 8)? as usize;
        let data = rdata_text(buf, rtype, after + 10, len)?;
        pos = after + 10 + len;
        records.push(Record {
            name: rname,
            rtype,
            class,
            ttl,
            data,
            ptr,
        });
    }
    Ok(Answer {
        rcode: (flags & 0xF) as u8,
        authoritative: flags & 0x0400 != 0,
        recursion_available: flags & 0x0080 != 0,
        records,
    })
}

fn random_id() -> u16 {
    RandomState::new().build_hasher().finish() as u16
}

/// Asks `server` (port 53) one question. 2 s timeout, one retry.
/// Returns the sent query bytes, the raw response and the parsed answer.
pub fn query_raw(
    server: IpAddr,
    name: &str,
    qtype: u16,
    qclass: u16,
) -> Result<(Vec<u8>, Vec<u8>, Answer), String> {
    let id = random_id();
    let q = build_query(id, name, qtype, qclass)?;
    let fail = |why: &str| {
        format!(
            "{server} antwortet nicht ({why}). Prüfe die Verbindung oder frag einen anderen Server."
        )
    };
    let bind = if server.is_ipv4() {
        "0.0.0.0:0"
    } else {
        "[::]:0"
    };
    let sock = UdpSocket::bind(bind).map_err(|e| fail(&e.to_string()))?;
    // connect() makes the OS drop packets from any other source
    sock.connect(SocketAddr::new(server, 53))
        .map_err(|e| fail(&e.to_string()))?;
    let mut last = "keine Antwort in 2 s".to_string();
    for _ in 0..2 {
        sock.send(&q).map_err(|e| fail(&e.to_string()))?;
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut buf = [0u8; 4096];
        while let Some(left) = deadline.checked_duration_since(Instant::now()) {
            sock.set_read_timeout(Some(left.max(Duration::from_millis(1))))
                .map_err(|e| fail(&e.to_string()))?;
            let Ok(n) = sock.recv(&mut buf) else { break };
            match parse_response(&buf[..n], id, name, qtype, qclass) {
                Ok(a) => return Ok((q, buf[..n].to_vec(), a)),
                // wrong ID or garbage: keep waiting for the real one until the deadline
                Err(e) => last = e,
            }
        }
    }
    Err(fail(&last))
}

/// Asks any server one question; for later stages.
#[allow(dead_code)] // used by later stages
pub fn query(server: IpAddr, name: &str, qtype: u16, qclass: u16) -> Result<Answer, String> {
    query_raw(server, name, qtype, qclass).map(|r| r.2)
}

/// First nameserver from `scutil --dns`, else /etc/resolv.conf.
pub fn system_resolver() -> Result<IpAddr, String> {
    let scutil = Command::new("scutil")
        .arg("--dns")
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned());
    let resolv = std::fs::read_to_string("/etc/resolv.conf").ok();
    scutil
        .as_deref()
        .and_then(|t| first_nameserver(t, "nameserver["))
        .or_else(|| {
            resolv
                .as_deref()
                .and_then(|t| first_nameserver(t, "nameserver "))
        })
        .ok_or_else(|| {
            "Kein DNS-Server des Systems gefunden. Prüfe in den Systemeinstellungen unter Netzwerk, ob du verbunden bist.".into()
        })
}

fn first_nameserver(text: &str, prefix: &str) -> Option<IpAddr> {
    text.lines().find_map(|l| {
        let rest = l.trim().strip_prefix(prefix)?;
        let value = if prefix.ends_with('[') {
            rest.split_once(':')?.1
        } else {
            rest
        };
        // ponytail: scoped link-local IPv6 (fe80::1%en0) fails to parse and is skipped
        value.trim().parse().ok()
    })
}

fn type_name(t: u16) -> String {
    match t {
        A => "A".into(),
        CNAME => "CNAME".into(),
        MX => "MX".into(),
        TXT => "TXT".into(),
        AAAA => "AAAA".into(),
        _ => format!("Typ {t}"),
    }
}

fn is_null_addr(r: &Record) -> bool {
    matches!(r.data.as_str(), "0.0.0.0" | "::")
}

fn of_type(a: &Answer, qtype: u16) -> Vec<&Record> {
    a.records.iter().filter(|r| r.rtype == qtype).collect()
}

pub fn blocked(a: &Answer, qtype: u16) -> bool {
    let r = of_type(a, qtype);
    a.rcode == 3 || (a.rcode == 0 && !r.is_empty() && r.iter().all(|x| is_null_addr(x)))
}

fn has_real(a: &Answer, qtype: u16) -> bool {
    a.rcode == 0 && of_type(a, qtype).iter().any(|x| !is_null_addr(x))
}

fn key(a: &Answer, qtype: u16) -> (u8, Vec<String>) {
    let mut v: Vec<String> = of_type(a, qtype).iter().map(|r| r.data.clone()).collect();
    v.sort();
    (a.rcode, v)
}

type Reply = Result<Answer, String>;

/// One German sentence: gleich / abweichend / blockiert / keine Antwort.
fn verdict(local: &Reply, others: &[&Reply], qtype: u16) -> String {
    let publics: Vec<&Answer> = others.iter().filter_map(|r| r.as_ref().ok()).collect();
    let Ok(local) = local else {
        return if publics.is_empty() {
            "Netz antwortet nicht: weder dein Netz noch 1.1.1.1 und 9.9.9.9 haben geantwortet. Wahrscheinlich bist du offline.".into()
        } else {
            "Netz antwortet nicht: dein DNS-Server schweigt, 1.1.1.1 und 9.9.9.9 antworten aber. Prüfe den DNS-Server deines Netzes.".into()
        };
    };
    if publics.is_empty() {
        return "Vergleich nicht möglich: 1.1.1.1 und 9.9.9.9 antworten nicht. Manche Netze sperren fremde DNS-Server.".into();
    }
    if blocked(local, qtype) && publics.iter().any(|a| has_real(a, qtype)) {
        return "Vom eigenen Netz blockiert: dein Netz antwortet mit „gibt es nicht“ oder 0.0.0.0, die öffentlichen Server kennen den Namen. Typisch für AdGuard oder Pi-hole.".into();
    }
    if publics.iter().all(|a| key(a, qtype) == key(local, qtype)) {
        "Gleich: dein Netz antwortet wie 1.1.1.1 und 9.9.9.9.".into()
    } else {
        "Abweichend: dein Netz antwortet anders als 1.1.1.1 und 9.9.9.9. Bei CDNs (Cloudflare, Akamai) ist das normal, weil jeder Server die Adresse des nächsten Rechenzentrums nennt. Bei einer Bank oder einem Login wäre es verdächtig.".into()
    }
}

fn hex(b: &[u8]) -> String {
    b.iter()
        .map(|x| format!("{x:02x}"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Byte-by-byte explanation of the query we built.
fn explain_query(on: bool, q: &[u8], name: &str, qtype: u16, qclass: u16) {
    if !on {
        return;
    }
    let line = |bytes: &[u8], text: String| explain(on, &format!("  {}  {text}", hex(bytes)));
    explain(
        on,
        "Die Anfrage ist ein UDP-Paket an Port 53. Es besteht aus Header (12 Bytes) und Frage:",
    );
    line(
        &q[0..2],
        "Transaktions-ID (zufällig, die Antwort muss sie wiederholen)".into(),
    );
    line(
        &q[2..4],
        "Flags: nur „RD“ gesetzt, also „bitte rekursiv auflösen“".into(),
    );
    line(&q[4..6], "1 Frage".into());
    line(
        &q[6..12],
        "0 Antworten, 0 Autoritäts-, 0 Zusatzeinträge (in der Anfrage immer 0)".into(),
    );
    let mut i = 12;
    for label in name.trim_end_matches('.').split('.') {
        let n = label.len();
        line(
            &q[i..i + 1 + n],
            format!("Länge {n}, dann der Text „{label}“"),
        );
        i += 1 + n;
    }
    line(&q[i..i + 1], "Ende des Namens".into());
    line(
        &q[i + 1..i + 3],
        format!("Typ {} (welche Art Eintrag gesucht wird)", type_name(qtype)),
    );
    let cls = if qclass == CH {
        "CH (Chaosnet, wird für Servertricks wie whoami benutzt)"
    } else {
        "IN (Internet)"
    };
    line(&q[i + 3..i + 5], format!("Klasse {cls}"));
}

/// Byte-by-byte explanation of what came back.
fn explain_response(on: bool, who: &str, raw: &[u8], a: &Answer) {
    if !on {
        return;
    }
    let rc = match a.rcode {
        0 => "NOERROR, alles gut",
        2 => "SERVFAIL, der Server hatte einen Fehler",
        3 => "NXDOMAIN, den Namen gibt es nicht",
        5 => "REFUSED, der Server verweigert die Antwort",
        _ => "anderer Fehlercode",
    };
    explain(on, &format!("Antwort von {who} ({} Bytes):", raw.len()));
    explain(
        on,
        &format!(
            "  {}  Flags: Antwort-Bit, RA={} (Server löst rekursiv auf), AA={}, Rückgabecode {} ({rc})",
            hex(&raw[2..4]),
            u8::from(a.recursion_available),
            u8::from(a.authoritative),
            a.rcode
        ),
    );
    explain(
        on,
        &format!(
            "  {}  Anzahl Antworten: {}",
            hex(&raw[6..8]),
            a.records.len()
        ),
    );
    for r in &a.records {
        let nm = match r.ptr {
            Some(p) => {
                format!("Name als Zeiger auf Offset {p} (Namenskompression, spart Bytes)")
            }
            None => format!("Name „{}“ ausgeschrieben", r.name),
        };
        explain(
            on,
            &format!(
                "  {} ({}): {nm}, Klasse {}, TTL {} s (so lange darf man die Antwort merken), Inhalt: {}",
                r.name,
                type_name(r.rtype),
                if r.class == CH { "CH" } else { "IN" },
                r.ttl,
                r.data
            ),
        );
    }
}

fn show(a: &Reply) -> String {
    match a {
        Err(e) => format!("keine Antwort ({e})"),
        Ok(a) if a.rcode == 3 => "gibt es nicht (NXDOMAIN)".into(),
        Ok(a) if a.records.is_empty() => format!("leer (Rückgabecode {})", a.rcode),
        Ok(a) => a
            .records
            .iter()
            .map(|r| r.data.as_str())
            .collect::<Vec<_>>()
            .join(", "),
    }
}

pub fn run(args: &[String], explain_on: bool) -> Result<(), String> {
    let usage = "Aufruf: netz dns <Name> [--typ A|AAAA|MX|TXT] oder netz dns --meine-ip";
    let (mut name, mut qtype, mut my_ip) = (None, A, false);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--meine-ip" => my_ip = true,
            "--typ" => {
                qtype = match it.next().map(|t| t.to_ascii_uppercase()).as_deref() {
                    Some("A") => A,
                    Some("AAAA") => AAAA,
                    Some("MX") => MX,
                    Some("TXT") => TXT,
                    _ => {
                        return Err(format!(
                            "Unbekannter Typ. Erlaubt sind A, AAAA, MX und TXT. {usage}"
                        ));
                    }
                }
            }
            n if !n.starts_with('-') && name.is_none() => name = Some(n),
            _ => return Err(format!("Unbekannte Angabe „{a}“. {usage}")),
        }
    }
    if my_ip {
        return run_my_ip(explain_on);
    }
    let name = name.ok_or_else(|| format!("Kein Name angegeben. {usage}"))?;
    let local = system_resolver()?;
    let servers = [
        (format!("deinem Netz ({local})"), local),
        (
            "1.1.1.1 (Cloudflare)".to_string(),
            IpAddr::from([1, 1, 1, 1]),
        ),
        ("9.9.9.9 (Quad9)".to_string(), IpAddr::from([9, 9, 9, 9])),
    ];
    explain(
        explain_on,
        &format!(
            "Ich frage dreimal dasselbe: {name}, Typ {}. Erst dein Netz, dann zwei unabhängige öffentliche Server.",
            type_name(qtype)
        ),
    );
    let mut replies = Vec::new();
    for (i, (label, ip)) in servers.iter().enumerate() {
        let r = query_raw(*ip, name, qtype, IN);
        if let Ok((q, raw, a)) = &r {
            if i == 0 {
                explain_query(explain_on, q, name, qtype, IN);
            }
            explain_response(explain_on, label, raw, a);
        }
        let r = r.map(|r| r.2);
        println!("{label}: {}", show(&r));
        replies.push(r);
    }
    println!(
        "{}",
        verdict(&replies[0], &[&replies[1], &replies[2]], qtype)
    );
    Ok(())
}

fn run_my_ip(explain_on: bool) -> Result<(), String> {
    explain(
        explain_on,
        "Cloudflare beantwortet die Frage „whoami.cloudflare“ (TXT, Klasse CH) mit der Adresse, von der die Frage kam. So erfährst du deine öffentliche IP ohne Webseite.",
    );
    let (q, raw, a) = query_raw(IpAddr::from([1, 1, 1, 1]), "whoami.cloudflare", TXT, CH)?;
    explain_query(explain_on, &q, "whoami.cloudflare", TXT, CH);
    explain_response(explain_on, "1.1.1.1", &raw, &a);
    let ip = a
        .records
        .iter()
        .find(|r| r.rtype == TXT)
        .ok_or("1.1.1.1 hat keine IP genannt. Versuch es später noch einmal.")?;
    println!("Deine öffentliche IP: {}", ip.data);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Response to `github.com A` as 1.1.1.1 sends it (ID 0x1234, address anonymised).
    fn github_response() -> Vec<u8> {
        let mut p = vec![0x12, 0x34, 0x81, 0x80, 0, 1, 0, 1, 0, 0, 0, 0];
        p.extend_from_slice(b"\x06github\x03com\x00\x00\x01\x00\x01");
        // answer: pointer to offset 12, A, IN, TTL 60, 4 bytes
        p.extend_from_slice(&[0xC0, 0x0C, 0, 1, 0, 1, 0, 0, 0, 60, 0, 4, 192, 0, 2, 1]);
        p
    }

    #[test]
    fn parses_real_capture() {
        // captured from 1.1.1.1 for github.com A, address replaced by 192.0.2.1
        let hex = "1234818000010001000000000667697468756203636f6d0000010001c00c000100010000001900 04c0000201".replace(' ', "");
        let raw: Vec<u8> = (0..hex.len() / 2)
            .map(|i| u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).unwrap())
            .collect();
        let a = parse_response(&raw, 0x1234, "github.com", A, IN).unwrap();
        assert_eq!(a.records[0].data, "192.0.2.1");
        assert_eq!(a.records[0].ttl, 25);
    }

    #[test]
    fn query_bytes() {
        let q = build_query(0xABCD, "github.com", A, IN).unwrap();
        let mut want = vec![0xAB, 0xCD, 0x01, 0x00, 0, 1, 0, 0, 0, 0, 0, 0];
        want.extend_from_slice(b"\x06github\x03com\x00");
        want.extend_from_slice(&[0, 1, 0, 1]);
        assert_eq!(q, want);
        let ch = build_query(1, "whoami.cloudflare.", TXT, CH).unwrap();
        assert_eq!(&ch[ch.len() - 4..], &[0, 16, 0, 3]);
    }

    #[test]
    fn invalid_names_rejected() {
        assert!(build_query(1, "", A, IN).is_err());
        assert!(build_query(1, "a..b", A, IN).is_err());
        assert!(build_query(1, &"x".repeat(64), A, IN).is_err());
        assert!(build_query(1, "bücher.de", A, IN).is_err());
    }

    #[test]
    fn parses_compressed_a_record() {
        let a = parse_response(&github_response(), 0x1234, "GitHub.com", A, IN).unwrap();
        assert_eq!(a.rcode, 0);
        assert!(a.recursion_available);
        let r = &a.records[0];
        assert_eq!(
            (r.name.as_str(), r.data.as_str(), r.ttl),
            ("github.com", "192.0.2.1", 60)
        );
        assert_eq!(r.ptr, Some(12));
    }

    #[test]
    fn rejects_wrong_id_or_question() {
        let p = github_response();
        assert!(parse_response(&p, 0x1235, "github.com", A, IN).is_err());
        assert!(parse_response(&p, 0x1234, "gitlab.com", A, IN).is_err());
        assert!(parse_response(&p, 0x1234, "github.com", AAAA, IN).is_err());
        assert!(parse_response(&p, 0x1234, "github.com", A, CH).is_err());
        let mut not_response = p.clone();
        not_response[2] = 0x01;
        assert!(parse_response(&not_response, 0x1234, "github.com", A, IN).is_err());
    }

    #[test]
    fn every_truncation_is_an_error_not_a_panic() {
        let p = github_response();
        for n in 0..p.len() {
            assert!(
                parse_response(&p[..n], 0x1234, "github.com", A, IN).is_err(),
                "len {n}"
            );
        }
    }

    #[test]
    fn pointer_loops_terminate() {
        // question name is a pointer to itself
        let p = vec![
            0x12, 0x34, 0x81, 0x80, 0, 1, 0, 0, 0, 0, 0, 0, 0xC0, 0x0C, 0, 1, 0, 1,
        ];
        assert!(parse_response(&p, 0x1234, "github.com", A, IN).is_err());
        // answer name is a pointer to itself (offset 29)
        let mut p = github_response();
        p[29] = 0x1D;
        assert!(parse_response(&p, 0x1234, "github.com", A, IN).is_err());
        // pointer past the end
        let mut q = github_response();
        q[29] = 0xFF;
        assert!(parse_response(&q, 0x1234, "github.com", A, IN).is_err());
    }

    #[test]
    fn lying_lengths_are_errors() {
        let mut p = github_response();
        let n = p.len();
        p[n - 5] = 0xFF; // rdlength way too big
        assert!(parse_response(&p, 0x1234, "github.com", A, IN).is_err());
        let mut q = github_response();
        q[7] = 50; // claims 50 answers
        assert!(parse_response(&q, 0x1234, "github.com", A, IN).is_err());
    }

    #[test]
    fn parses_cname_mx_txt_aaaa_and_ch() {
        let mut p = vec![0, 7, 0x81, 0x80, 0, 1, 0, 4, 0, 0, 0, 0];
        p.extend_from_slice(b"\x03www\x07example\x03org\x00\x00\x0F\x00\x03");
        let hdr = |t: u8, len: u8| vec![0xC0, 0x0C, 0, t, 0, 3, 0, 0, 1, 0, 0, len];
        p.extend(hdr(5, 6));
        p.extend_from_slice(b"\x03abc\xC0\x10"); // abc.example.org via pointer
        p.extend(hdr(15, 9));
        p.extend_from_slice(&[0, 10, 4]);
        p.extend_from_slice(b"mail\xC0\x10");
        p.extend(hdr(16, 8));
        p.extend_from_slice(b"\x03foo\x03bar");
        p.extend(hdr(28, 16));
        p.extend_from_slice(&[0x20, 0x01, 0x0d, 0xb8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]);
        let a = parse_response(&p, 7, "www.example.org", MX, CH).unwrap();
        let d: Vec<_> = a.records.iter().map(|r| r.data.as_str()).collect();
        assert_eq!(
            d,
            [
                "abc.example.org",
                "10 mail.example.org",
                "foobar",
                "2001:db8::1"
            ]
        );
        assert_eq!(a.records[0].class, CH);
    }

    #[test]
    fn nameserver_parsing() {
        let scutil = "DNS configuration\n\nresolver #1\n  search domain[0] : fritz.box\n  nameserver[0] : 100.100.100.100\n  nameserver[1] : 192.0.2.53\n";
        assert_eq!(
            first_nameserver(scutil, "nameserver["),
            Some("100.100.100.100".parse().unwrap())
        );
        assert_eq!(
            first_nameserver("# x\nnameserver 192.0.2.53\n", "nameserver "),
            Some("192.0.2.53".parse().unwrap())
        );
        assert_eq!(first_nameserver("nothing", "nameserver "), None);
    }

    fn ans(rcode: u8, ips: &[&str]) -> Reply {
        Ok(Answer {
            rcode,
            authoritative: false,
            recursion_available: true,
            records: ips
                .iter()
                .map(|i| Record {
                    name: "x".into(),
                    rtype: A,
                    class: IN,
                    ttl: 1,
                    data: i.to_string(),
                    ptr: None,
                })
                .collect(),
        })
    }

    #[test]
    fn verdicts() {
        let same = ans(0, &["192.0.2.1", "192.0.2.2"]);
        let swapped = ans(0, &["192.0.2.2", "192.0.2.1"]);
        let other = ans(0, &["203.0.113.9"]);
        let down: Reply = Err("x".into());
        assert!(verdict(&same, &[&swapped, &swapped], A).starts_with("Gleich"));
        assert!(verdict(&other, &[&same, &same], A).starts_with("Abweichend"));
        let null = ans(0, &["0.0.0.0"]);
        assert!(verdict(&null, &[&same, &same], A).starts_with("Vom eigenen Netz blockiert"));
        let nx = ans(3, &[]);
        assert!(verdict(&nx, &[&same, &down], A).starts_with("Vom eigenen Netz blockiert"));
        assert!(verdict(&nx, &[&nx, &nx], A).starts_with("Gleich"));
        assert!(verdict(&down, &[&same, &same], A).starts_with("Netz antwortet nicht"));
        assert!(verdict(&same, &[&down, &down], A).starts_with("Vergleich nicht möglich"));
    }
}
