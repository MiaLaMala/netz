//! IPv4 und IPv6 im Vergleich, Happy Eyeballs. Issue #48.
//!
//! Pro Dienst und Protokoll: Namen auflösen (A, AAAA getrennt), fünfmal TCP-Connect auf Port 443
//! (kein Byte Nutzdaten), Median und Fehlerzahl. Daraus ein Urteil und die Rechnung nach RFC 8305.
//! Gespeichert wird nie eine Adresse, nur Urteil, Zeitpunkt und die beiden Mediane (`ipv6.json`).

use crate::pruefen::Klasse;
use crate::{dns, explain, hier, netze, statistik};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::net::{IpAddr, Ipv6Addr, SocketAddr, TcpStream, UdpSocket};
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

/// Dual-stack services; the user may add one more name on the command line.
const ZIELE: [&str; 5] = [
    "cloudflare.com",
    "google.com",
    "wikipedia.org",
    "dns.quad9.net",
    "www.apple.com",
];
const VERSUCHE: u32 = 5;
const ZEITLIMIT_MS: f64 = 2000.0;
/// RFC 8305 "Connection Attempt Delay": IPv4 starts this long after IPv6.
const VERSATZ_MS: f64 = 250.0;
const MIN_ZIELE: usize = 3;
/// 1.1.1.1 over IPv6; only used to ask the kernel which source address and route it would pick.
const PROBE_V6: Ipv6Addr = Ipv6Addr::new(0x2606, 0x4700, 0x4700, 0, 0, 0, 0, 0x1111);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Urteil {
    KeinIpv6,
    Gleich,
    Langsamer,
    Kaputt,
    Unsicher,
}

impl Urteil {
    pub fn wort(self) -> &'static str {
        match self {
            Self::KeinIpv6 => "kein IPv6",
            Self::Gleich => "IPv6 nutzbar und gleich schnell",
            Self::Langsamer => "IPv6 nutzbar, aber langsamer",
            Self::Kaputt => "IPv6 kaputt (Adresse vorhanden, Verbindungen scheitern)",
            Self::Unsicher => "unsicher",
        }
    }
}

/// What this machine has locally. Never holds the address itself.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
pub struct Lokal {
    /// A global (not fe80::, not fc00::/7) IPv6 address would be used as source.
    pub globale_adresse: bool,
    pub standardroute: bool,
    /// Default route via a link-local router, which is how Router Advertisements show up. `None`: unknown.
    pub router_ankuendigung: Option<bool>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Proto {
    /// The name has an address of this family.
    pub aufgeloest: bool,
    pub dns_ms: Option<f64>,
    /// Median connect time over the successful tries; `None`: no try worked.
    pub median_ms: Option<f64>,
    pub versuche: u32,
    pub fehler: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub enum Gewinner {
    V4,
    V6,
}

/// The race of RFC 8305 with fixed times.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Rennen {
    pub gewinner: Gewinner,
    /// Time until the client has a connection.
    pub ms: f64,
    /// Time lost against plain IPv4 (0 if IPv6 wins).
    pub verzoegerung_ms: f64,
    /// Time lost by a client that tries IPv6 alone and waits for it (to the time limit if it hangs).
    pub ohne_he_ms: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ZielErgebnis {
    pub name: String,
    pub v4: Proto,
    pub v6: Proto,
    pub rennen: Option<Rennen>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Ergebnis {
    pub urteil: Urteil,
    pub satz: String,
    pub lokal: Lokal,
    pub ziele: Vec<ZielErgebnis>,
    /// Median over the per-service medians, only services with both medians.
    pub median_v4_ms: Option<f64>,
    pub median_v6_ms: Option<f64>,
}

/// RFC 8305 for one service. IPv6 starts at 0, IPv4 at 250 ms; the first connection wins.
/// `None` means the tries failed (the client waits for the time limit).
pub fn rennen(v6: Option<f64>, v4: Option<f64>) -> Option<Rennen> {
    let (gewinner, ms, verz, ohne) = match (v6, v4) {
        (None, None) => return None,
        (Some(b), None) => (Gewinner::V6, b, 0.0, 0.0),
        (None, Some(a)) => (Gewinner::V4, VERSATZ_MS + a, VERSATZ_MS, ZEITLIMIT_MS),
        (Some(b), Some(a)) if b <= VERSATZ_MS + a => (Gewinner::V6, b, 0.0, (b - a).max(0.0)),
        (Some(b), Some(a)) => (Gewinner::V4, VERSATZ_MS + a, VERSATZ_MS, b - a),
    };
    Some(Rennen {
        gewinner,
        ms,
        verzoegerung_ms: verz,
        ohne_he_ms: ohne,
    })
}

/// Verdict and sentence from fixed numbers. Needs `MIN_ZIELE` services where IPv4 works and the
/// name has an IPv6 address; fewer says "unsicher" (a dead service is not the network's fault).
pub fn auswerten(lokal: Lokal, ziele: Vec<ZielErgebnis>) -> Ergebnis {
    let fertig = |urteil, satz: String, v4, v6, ziele| Ergebnis {
        urteil,
        satz,
        lokal,
        ziele,
        median_v4_ms: v4,
        median_v6_ms: v6,
    };
    if !lokal.globale_adresse || !lokal.standardroute {
        let satz = "Dieses Netz gibt dir kein IPv6, alles läuft über IPv4. Das ist heute noch weit verbreitet und kein Fehler.";
        return fertig(Urteil::KeinIpv6, satz.into(), None, None, ziele);
    }
    let brauchbar: Vec<&ZielErgebnis> = ziele
        .iter()
        .filter(|z| z.v6.aufgeloest && z.v4.median_ms.is_some())
        .collect();
    if brauchbar.len() < MIN_ZIELE {
        let satz = format!(
            "Nur {} von mindestens {MIN_ZIELE} Diensten ließen sich vergleichen. Das sagt noch nichts über dein Netz, versuche es später noch mal.",
            brauchbar.len()
        );
        return fertig(Urteil::Unsicher, satz, None, None, ziele);
    }
    let ok6 = brauchbar
        .iter()
        .filter(|z| z.v6.median_ms.is_some())
        .count();
    if ok6 * 2 < brauchbar.len() {
        let satz = "Dein Netz hat eine IPv6-Adresse, aber Verbindungen darüber scheitern. Programme warten dann erst und weichen dann auf IPv4 aus. Das macht Seiten spürbar langsamer.";
        return fertig(Urteil::Kaputt, satz.into(), None, None, ziele);
    }
    let paare: Vec<(f64, f64)> = brauchbar
        .iter()
        .filter_map(|z| Some((z.v4.median_ms?, z.v6.median_ms?)))
        .collect();
    let v4 = statistik::median(&paare.iter().map(|p| p.0).collect::<Vec<_>>());
    let v6 = statistik::median(&paare.iter().map(|p| p.1).collect::<Vec<_>>());
    let (a, b) = (v4.unwrap_or(0.0), v6.unwrap_or(0.0));
    let (urteil, satz) = if b > a * 1.5 && b - a > 10.0 {
        (
            Urteil::Langsamer,
            format!(
                "IPv6 funktioniert, ist hier aber {:.0} ms langsamer als IPv4. Dein Rechner startet IPv6 zuerst und nimmt nach 250 ms zusätzlich IPv4, es gewinnt die Verbindung, die zuerst steht.",
                b - a
            ),
        )
    } else if a - b >= 1.0 {
        (
            Urteil::Gleich,
            format!(
                "IPv6 ist hier {:.0} ms schneller als IPv4. Dein Rechner nimmt es automatisch.",
                a - b
            ),
        )
    } else {
        (
            Urteil::Gleich,
            "IPv6 und IPv4 sind hier gleich schnell. Dein Rechner nimmt IPv6 automatisch.".into(),
        )
    };
    fertig(urteil, satz, v4, v6, ziele)
}

/// Global unicast only: not link-local, not unique-local (fc00::/7), not loopback or unspecified.
fn ist_global(a: &Ipv6Addr) -> bool {
    let erste = a.segments()[0];
    !(a.is_loopback()
        || a.is_unspecified()
        || a.is_multicast()
        || erste & 0xffc0 == 0xfe80
        || erste & 0xfe00 == 0xfc00)
}

/// Is there a default route whose gateway is a link-local address (as a Router Advertisement
/// creates it)? Input: `netstat -rn -f inet6`.
fn parse_router(out: &str) -> bool {
    out.lines().any(|l| {
        let mut w = l.split_whitespace();
        w.next() == Some("default") && w.next().is_some_and(|g| g.starts_with("fe80"))
    })
}

/// Asks the kernel (UDP connect sends nothing) which source address and route IPv6 would use.
fn lokal() -> Lokal {
    let quelle = UdpSocket::bind("[::]:0").ok().and_then(|s| {
        s.connect(SocketAddr::new(PROBE_V6.into(), 53)).ok()?;
        s.local_addr().ok()
    });
    let router = Command::new("netstat")
        .args(["-rn", "-f", "inet6"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| parse_router(&String::from_utf8_lossy(&o.stdout)));
    Lokal {
        globale_adresse: matches!(quelle, Some(SocketAddr::V6(a)) if ist_global(a.ip())),
        standardroute: quelle.is_some(),
        router_ankuendigung: router,
    }
}

fn ms_seit(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1000.0
}

/// Resolves `name` for one family and connects `VERSUCHE` times to the first address.
fn proto_messen(resolver: IpAddr, name: &str, v6: bool) -> Proto {
    let t = Instant::now();
    let qtype = if v6 { 28 } else { 1 };
    let ip = dns::query(resolver, name, qtype, dns::IN)
        .ok()
        .and_then(|a| {
            a.records
                .iter()
                .filter(|r| r.rtype == qtype)
                .find_map(|r| r.data.parse::<IpAddr>().ok())
        });
    let mut p = Proto {
        aufgeloest: ip.is_some(),
        dns_ms: ip.map(|_| ms_seit(t)),
        ..Proto::default()
    };
    let Some(ip) = ip else { return p };
    let mut zeiten = Vec::new();
    for _ in 0..VERSUCHE {
        let t = Instant::now();
        p.versuche += 1;
        match TcpStream::connect_timeout(
            &SocketAddr::new(ip, 443),
            Duration::from_millis(ZEITLIMIT_MS as u64),
        ) {
            Ok(_) => zeiten.push(ms_seit(t)),
            Err(_) => p.fehler += 1,
        }
    }
    p.median_ms = statistik::median(&zeiten);
    p
}

/// Measures every service in parallel: the worst case is one DNS timeout plus five connect
/// timeouts, well under 30 s.
pub fn messen(extra: Option<&str>) -> Result<Ergebnis, String> {
    let lokal = lokal();
    if !lokal.globale_adresse || !lokal.standardroute {
        return Ok(auswerten(lokal, Vec::new()));
    }
    let resolver = dns::system_resolver()?;
    let namen: Vec<&str> = ZIELE.iter().copied().chain(extra).collect();
    let ziele = std::thread::scope(|s| {
        let laeufe: Vec<_> = namen
            .iter()
            .map(|n| {
                let v4 = s.spawn(move || proto_messen(resolver, n, false));
                let v6 = s.spawn(move || proto_messen(resolver, n, true));
                (n, v4, v6)
            })
            .collect();
        laeufe
            .into_iter()
            .map(|(n, v4, v6)| {
                let v4 = v4.join().unwrap_or_default();
                let v6 = v6.join().unwrap_or_default();
                let rennen = if v6.aufgeloest {
                    rennen(v6.median_ms, v4.median_ms)
                } else {
                    None
                };
                ZielErgebnis {
                    name: n.to_string(),
                    v4,
                    v6,
                    rennen,
                }
            })
            .collect()
    });
    Ok(auswerten(lokal, ziele))
}

// ---- Speicher ----

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Gemerkt {
    pub urteil: Urteil,
    pub zeitpunkt: u64,
    pub median_v4_ms: Option<f64>,
    pub median_v6_ms: Option<f64>,
}

type Store = BTreeMap<String, Gemerkt>;

/// Stores a verdict (not "unsicher": that is no news) and returns the one before.
fn speichern(path: &Path, key: &str, e: &Ergebnis) -> Result<Option<Gemerkt>, String> {
    if e.urteil == Urteil::Unsicher {
        return Ok(None);
    }
    let mut store: Store = netze::load_json(path)?;
    let neu = Gemerkt {
        urteil: e.urteil,
        zeitpunkt: netze::now(),
        median_v4_ms: e.median_v4_ms,
        median_v6_ms: e.median_v6_ms,
    };
    let alt = store.insert(key.to_string(), neu);
    netze::save(path, &store)?;
    Ok(alt)
}

fn unterschiede(alt: Option<&Gemerkt>, neu: Urteil) -> Vec<(Klasse, String)> {
    let Some(alt) = alt else { return Vec::new() };
    let nutzbar = |u| matches!(u, Urteil::Gleich | Urteil::Langsamer);
    let text = match (alt.urteil, neu) {
        (a, Urteil::KeinIpv6 | Urteil::Kaputt) if nutzbar(a) => {
            "IPv6 war früher nutzbar, jetzt nicht mehr (`netzinfo ipv6` zeigt, was sich geändert hat)."
        }
        (Urteil::KeinIpv6, Urteil::Kaputt) => {
            "Das Netz hat jetzt eine IPv6-Adresse, aber Verbindungen darüber scheitern (`netzinfo ipv6`)."
        }
        _ => return Vec::new(),
    };
    vec![(Klasse::Auffaellig, text.to_string())]
}

/// For `pruefen`: measures, stores and compares with the last visit. A store error skips the
/// comparison, it does not stop the check.
pub fn fuer_pruefen(daten: &Path, key: &str, _meins: bool) -> Vec<(Klasse, String)> {
    let Ok(e) = messen(None) else {
        return Vec::new();
    };
    match speichern(&daten.with_file_name("ipv6.json"), key, &e) {
        Ok(alt) => unterschiede(alt.as_ref(), e.urteil),
        Err(_) => Vec::new(),
    }
}

/// Stores for the current network if it is remembered. `None`: nothing to store.
fn speichern_aktuell(e: &Ergebnis) -> Option<Result<(), String>> {
    let fp = netze::fingerprint(&hier::collect().ok()?).ok()?;
    let path = netze::data_path().ok()?;
    netze::find(&netze::load(&path).ok()?, &fp)?;
    Some(speichern(&path.with_file_name("ipv6.json"), &fp.key(), e).map(|_| ()))
}

/// For the app: measures the fixed list and stores the result for a remembered network.
pub fn daten() -> Result<Ergebnis, String> {
    let e = messen(None)?;
    // A store problem must not hide the measurement.
    let _ = speichern_aktuell(&e);
    Ok(e)
}

// ---- Befehl ----

fn ms(v: Option<f64>) -> String {
    v.map_or("-".into(), |v| format!("{v:.0} ms"))
}

fn zeile(p: &Proto) -> String {
    if !p.aufgeloest {
        return "keine Adresse für diesen Namen".into();
    }
    let verbindung = p
        .median_ms
        .map_or("scheitert".into(), |m| format!("{m:.0} ms"));
    format!(
        "Name {}, Verbindung {verbindung} ({} von {} gescheitert)",
        ms(p.dns_ms),
        p.fehler,
        p.versuche
    )
}

fn rennen_text(r: &Rennen) -> String {
    let wer = match r.gewinner {
        Gewinner::V6 => "IPv6",
        Gewinner::V4 => "IPv4",
    };
    let mut t = format!("{wer} gewinnt nach {:.0} ms", r.ms);
    if r.verzoegerung_ms > 0.0 {
        t += &format!(", {:.0} ms später als IPv4 allein", r.verzoegerung_ms);
    }
    if r.ohne_he_ms >= ZEITLIMIT_MS {
        t += &format!(
            ". Ohne Happy Eyeballs würde ein Programm bis zu {:.0} ms auf IPv6 warten",
            r.ohne_he_ms
        );
    }
    t
}

pub const OPTIONEN: &[&str] = &["--json"];

pub fn run(args: &[String], explain_on: bool) -> Result<(), String> {
    crate::optionen::nur(args, OPTIONEN, "ipv6")?;
    let (json, extra) = match args {
        [] => (false, None),
        [a] if a == "--json" => (true, None),
        [n] if !n.starts_with('-') && !n.contains(char::is_whitespace) => (false, Some(n.as_str())),
        [a, ..] => {
            return Err(format!(
                "`netzinfo ipv6` kennt „{a}“ nicht. Erlaubt sind `--json` oder ein zusätzlicher Dienstname, z. B. `netzinfo ipv6 example.org`."
            ));
        }
    };
    explain(
        explain_on,
        "Ich frage den Rechner, ob er für IPv6 eine globale Adresse (nicht fe80::) und eine Standardroute hat. Dazu öffne ich nur einen UDP-Socket und sende nichts.",
    );
    explain(
        explain_on,
        "Dann löse ich fünf bekannte Dienste getrennt nach A (IPv4) und AAAA (IPv6) auf und baue je Protokoll fünfmal eine TCP-Verbindung auf Port 443 auf. Ich messe die Zeit bis zum Handshake und schließe sofort wieder, es fließen keine Nutzdaten.",
    );
    explain(
        explain_on,
        "Happy Eyeballs (RFC 8305) heißt: Dein Rechner startet IPv6 zuerst, wartet etwa 250 ms und startet dann IPv4 daneben. Es zählt, was zuerst steht. So merkst du ein kaputtes IPv6 meist nur als kleine Wartezeit.",
    );
    let e = messen(extra)?;
    // A changed service list would make verdicts incomparable, so only the fixed list is stored.
    let gespeichert = if extra.is_some() {
        None
    } else {
        speichern_aktuell(&e)
    };
    if json {
        let s = serde_json::to_string_pretty(&e).map_err(|e| format!("JSON-Fehler: {e}"))?;
        println!("{s}");
        return Ok(());
    }
    let ja = |b| if b { "ja" } else { "nein" };
    println!(
        "Hier: globale IPv6-Adresse {}, Standardroute {}, Router-Ankündigung {}",
        ja(e.lokal.globale_adresse),
        ja(e.lokal.standardroute),
        e.lokal.router_ankuendigung.map_or("unbekannt", ja)
    );
    for z in &e.ziele {
        println!("{}", z.name);
        println!("  IPv4: {}", zeile(&z.v4));
        println!("  IPv6: {}", zeile(&z.v6));
        if let Some(r) = &z.rennen {
            println!("  Happy Eyeballs: {}", rennen_text(r));
        }
    }
    println!("Urteil: {}", e.urteil.wort());
    println!("{}", e.satz);
    if let Some(Err(why)) = gespeichert {
        println!("Hinweis: {why}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(median: Option<f64>) -> Proto {
        Proto {
            aufgeloest: true,
            dns_ms: Some(10.0),
            median_ms: median,
            versuche: 5,
            fehler: if median.is_some() { 0 } else { 5 },
        }
    }
    fn ziel(v4: Option<f64>, v6: Option<f64>) -> ZielErgebnis {
        ZielErgebnis {
            name: "dienst.example".into(),
            v4: p(v4),
            v6: p(v6),
            rennen: rennen(v6, v4),
        }
    }
    const DA: Lokal = Lokal {
        globale_adresse: true,
        standardroute: true,
        router_ankuendigung: Some(true),
    };

    #[test]
    fn urteil_vier_faelle() {
        let drei = |v4, v6| vec![ziel(v4, v6), ziel(v4, v6), ziel(v4, v6)];
        // no IPv6 locally: calm result without services
        assert_eq!(
            auswerten(Lokal::default(), Vec::new()).urteil,
            Urteil::KeinIpv6
        );
        let kein_route = Lokal {
            standardroute: false,
            ..DA
        };
        assert_eq!(auswerten(kein_route, Vec::new()).urteil, Urteil::KeinIpv6);
        let gleich = auswerten(DA, drei(Some(20.0), Some(16.0)));
        assert_eq!(gleich.urteil, Urteil::Gleich);
        assert!(gleich.satz.starts_with("IPv6 ist hier 4 ms schneller"));
        assert_eq!(
            auswerten(DA, drei(Some(20.0), Some(21.0))).urteil,
            Urteil::Gleich
        );
        assert_eq!(
            auswerten(DA, drei(Some(20.0), Some(60.0))).urteil,
            Urteil::Langsamer
        );
        let kaputt = auswerten(DA, drei(Some(20.0), None));
        assert_eq!(kaputt.urteil, Urteil::Kaputt);
        assert!(kaputt.satz.contains("Verbindungen darüber scheitern"));
        // two of three work: not broken
        let mix = vec![
            ziel(Some(20.0), Some(20.0)),
            ziel(Some(20.0), Some(20.0)),
            ziel(Some(20.0), None),
        ];
        assert_eq!(auswerten(DA, mix).urteil, Urteil::Gleich);
    }

    #[test]
    fn weniger_als_drei_ziele_ist_unsicher() {
        let mut z = vec![ziel(Some(20.0), Some(20.0)), ziel(Some(20.0), Some(20.0))];
        assert_eq!(auswerten(DA, z.clone()).urteil, Urteil::Unsicher);
        // IPv4 down: the service does not count
        z.push(ziel(None, Some(20.0)));
        assert_eq!(auswerten(DA, z.clone()).urteil, Urteil::Unsicher);
        // no AAAA record: neither
        z.pop();
        let mut ohne_aaaa = ziel(Some(20.0), None);
        ohne_aaaa.v6.aufgeloest = false;
        z.push(ohne_aaaa);
        assert_eq!(auswerten(DA, z).urteil, Urteil::Unsicher);
    }

    #[test]
    fn happy_eyeballs_rechnung() {
        // IPv6 wins: 16 ms against 250 + 20
        let r = rennen(Some(16.0), Some(20.0)).unwrap();
        assert_eq!(
            (r.gewinner, r.ms, r.verzoegerung_ms),
            (Gewinner::V6, 16.0, 0.0)
        );
        // IPv6 works but is 300 ms slower than IPv4: IPv4 wins at 250 + 20
        let r = rennen(Some(320.0), Some(20.0)).unwrap();
        assert_eq!(
            (r.gewinner, r.ms, r.verzoegerung_ms),
            (Gewinner::V4, 270.0, 250.0)
        );
        assert_eq!(r.ohne_he_ms, 300.0);
        // IPv6 hangs: IPv4 wins after the 250 ms delay, a client without Happy Eyeballs loses 2 s
        let r = rennen(None, Some(20.0)).unwrap();
        assert_eq!(
            (r.gewinner, r.ms, r.verzoegerung_ms),
            (Gewinner::V4, 270.0, 250.0)
        );
        assert_eq!(r.ohne_he_ms, ZEITLIMIT_MS);
        assert_eq!(rennen(None, None), None);
    }

    #[test]
    fn adressen_und_router() {
        let a = |s: &str| ist_global(&s.parse().unwrap());
        assert!(a("2001:db8::1"));
        assert!(!a("fe80::1"));
        assert!(!a("fd00::1"));
        assert!(!a("::1"));
        let mit = "Destination Gateway Flags Netif\ndefault fe80::1%en0 UGcg en0\n";
        assert!(parse_router(mit));
        assert!(!parse_router(
            "default link#14 UCSIg utun4\n2001:db8::/64 link#4 UC en0\n"
        ));
        assert!(!parse_router(""));
    }

    #[test]
    fn speicher_ohne_adresse_und_unterschiede() {
        let dir = std::env::temp_dir().join(format!("ipv6-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("ipv6.json");
        let mut e = auswerten(DA, vec![ziel(Some(20.0), Some(16.0)); 3]);
        assert_eq!(speichern(&path, "k", &e).unwrap(), None);
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("gleich") && text.contains("zeitpunkt"));
        assert!(!text.contains("::") && !text.contains("2001"), "{text}");
        // a later broken result is reported against the stored good one
        e.urteil = Urteil::Kaputt;
        let alt = speichern(&path, "k", &e).unwrap();
        let u = unterschiede(alt.as_ref(), e.urteil);
        assert_eq!(u.len(), 1);
        assert_eq!(u[0].0, Klasse::Auffaellig);
        assert!(u[0].1.starts_with("IPv6 war früher nutzbar"));
        // "unsicher" is neither stored nor reported
        e.urteil = Urteil::Unsicher;
        assert_eq!(speichern(&path, "k", &e).unwrap(), None);
        assert!(unterschiede(alt.as_ref(), Urteil::Unsicher).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
