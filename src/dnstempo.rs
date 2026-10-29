//! DNS-Server im Vergleich: wer antwortet am schnellsten. Issue #50.
//! Misst mit dem eigenen DNS-Client (`dns`), nacheinander und mit Pause, nie parallel.

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr, UdpSocket};
use std::path::PathBuf;
use std::thread::sleep;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::{dns, explain, hier, netze, statistik};

const FEST: [&str; 20] = [
    "google.com",
    "youtube.com",
    "facebook.com",
    "wikipedia.org",
    "amazon.com",
    "apple.com",
    "microsoft.com",
    "example.com",
    "github.com",
    "reddit.com",
    "netflix.com",
    "tagesschau.de",
    "spiegel.de",
    "x.com",
    "linkedin.com",
    "instagram.com",
    "whatsapp.com",
    "cloudflare.com",
    "mozilla.org",
    "bing.com",
];
const MAX_EIGENE: usize = 50;
const UNBEKANNT: usize = 5;
const MAX_ABFRAGEN: usize = 60;
/// Jeder Name wird zweimal gefragt (Aufwärmen, Messen), dazu die unbekannten.
const MAX_NAMEN: usize = (MAX_ABFRAGEN - UNBEKANNT) / 2;
const PAUSE: Duration = Duration::from_millis(50);
const FRIST: Duration = Duration::from_secs(1);
const MAX_LAEUFE: usize = 5;
const OEFFENTLICH: [(&str, [u8; 4]); 3] = [
    ("1.1.1.1 (Cloudflare)", [1, 1, 1, 1]),
    ("8.8.8.8 (Google)", [8, 8, 8, 8]),
    ("9.9.9.9 (Quad9)", [9, 9, 9, 9]),
];

/// Eine Tabellenzeile; Zeiten in Millisekunden.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Zeile {
    pub name: String,
    pub oeffentlich: bool,
    pub bekannt_median: Option<f64>,
    pub bekannt_p95: Option<f64>,
    pub unbekannt_median: Option<f64>,
    pub unbekannt_p95: Option<f64>,
    /// Anteil 0..=1 der gezählten Abfragen ohne gültige Antwort.
    pub fehlerrate: f64,
    /// Zeiten über dem Dreifachen des Medians ihrer Gruppe.
    pub ausreisser: usize,
}

struct Roh {
    bekannt: Vec<f64>,
    unbekannt: Vec<f64>,
    fehler: usize,
}

/// Eine A-Abfrage; Zeit in ms. `None`: Zeitüberschreitung, SERVFAIL oder Müll.
/// NXDOMAIN gilt als gültige Antwort.
fn abfrage(ziel: SocketAddr, name: &str) -> Option<f64> {
    let id = dns::random_id();
    let q = dns::build_query(id, name, dns::A, dns::IN).ok()?;
    let sock = UdpSocket::bind(if ziel.is_ipv4() {
        "0.0.0.0:0"
    } else {
        "[::]:0"
    })
    .ok()?;
    sock.connect(ziel).ok()?;
    let start = Instant::now();
    sock.send(&q).ok()?;
    let mut buf = [0u8; 4096];
    while let Some(rest) = FRIST.checked_sub(start.elapsed()) {
        sock.set_read_timeout(Some(rest.max(Duration::from_millis(1))))
            .ok()?;
        let n = sock.recv(&mut buf).ok()?;
        if let Ok(a) = dns::parse_response(&buf[..n], id, name, dns::A, dns::IN) {
            return matches!(a.rcode, 0 | 3).then(|| start.elapsed().as_secs_f64() * 1000.0);
        }
    }
    None
}

fn zufallsname() -> String {
    format!(
        "x{:04x}{:04x}.example.com",
        dns::random_id(),
        dns::random_id()
    )
}

/// Höchstens `MAX_ABFRAGEN` Abfragen an einen Server. Antworten die ersten drei nicht,
/// gilt der Server als nicht erreichbar und der Rest entfällt.
fn messen(ziel: SocketAddr, namen: &[String], pause: Duration) -> Roh {
    let namen = &namen[..namen.len().min(MAX_NAMEN)];
    // (Name, zählt, bekannt)
    let mut plan: Vec<(String, bool, bool)> =
        namen.iter().map(|n| (n.clone(), false, true)).collect();
    plan.extend(namen.iter().map(|n| (n.clone(), true, true)));
    plan.extend((0..UNBEKANNT).map(|_| (zufallsname(), true, false)));
    let mut roh = Roh {
        bekannt: vec![],
        unbekannt: vec![],
        fehler: 0,
    };
    let mut hintereinander = 0;
    let mut letzte = Instant::now();
    for (i, (name, zaehlt, bekannt)) in plan.iter().enumerate() {
        // Abstand von Abfrage-Start zu Abfrage-Start
        sleep(pause.saturating_sub(letzte.elapsed()));
        letzte = Instant::now();
        let ms = abfrage(ziel, name);
        hintereinander = if ms.is_none() { hintereinander + 1 } else { 0 };
        if *zaehlt {
            match (ms, bekannt) {
                (Some(ms), true) => roh.bekannt.push(ms),
                (Some(ms), false) => roh.unbekannt.push(ms),
                (None, _) => roh.fehler += 1,
            }
        }
        if i == 2 && hintereinander == 3 {
            // ponytail: der Rest zählt als Fehler, damit die Rate nicht schöner aussieht als sie ist
            roh.fehler = namen.len() + UNBEKANNT;
            break;
        }
    }
    roh
}

fn auswerten(name: &str, oeffentlich: bool, r: &Roh) -> Zeile {
    let ausreisser =
        |v: &[f64]| statistik::median(v).map_or(0, |m| v.iter().filter(|x| **x > 3.0 * m).count());
    let gezaehlt = r.bekannt.len() + r.unbekannt.len() + r.fehler;
    Zeile {
        name: name.to_string(),
        oeffentlich,
        bekannt_median: statistik::median(&r.bekannt),
        bekannt_p95: statistik::perzentil(&r.bekannt, 95),
        unbekannt_median: statistik::median(&r.unbekannt),
        unbekannt_p95: statistik::perzentil(&r.unbekannt, 95),
        fehlerrate: if gezaehlt == 0 {
            1.0
        } else {
            r.fehler as f64 / gezaehlt as f64
        },
        ausreisser: ausreisser(&r.bekannt) + ausreisser(&r.unbekannt),
    }
}

/// Index des besten öffentlichen Servers, wenn er auf beiden Gruppen mindestens 30 % kürzeren
/// Median hat und nicht mehr Fehler als das System-DNS (`zeilen[0]`). Sonst `None`.
pub fn empfehlung(zeilen: &[Zeile]) -> Option<usize> {
    let sys = zeilen.first()?;
    let (sb, su) = (sys.bekannt_median?, sys.unbekannt_median?);
    let schnell = |z: &Zeile| {
        let (b, u) = (z.bekannt_median?, z.unbekannt_median?);
        // b <= 0,7 * sb ohne Gleitkomma-Rundung an der Grenze
        (b * 10.0 <= sb * 7.0 && u * 10.0 <= su * 7.0 && z.fehlerrate <= sys.fehlerrate)
            .then_some(b + u)
    };
    (1..zeilen.len())
        .filter_map(|i| Some((i, schnell(&zeilen[i])?)))
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| i)
}

/// Eigene Namen (kommagetrennt) zuerst, dann die feste Liste, höchstens `MAX_NAMEN`.
/// Zweiter Wert: wurde gekürzt?
fn namenliste(eigene: &str) -> Result<(Vec<String>, bool), String> {
    let eigene: Vec<String> = eigene
        .split(',')
        .map(|n| n.trim().to_ascii_lowercase())
        .filter(|n| !n.is_empty())
        .collect();
    if eigene.len() > MAX_EIGENE {
        return Err(format!(
            "Zu viele Namen: {} angegeben, erlaubt sind höchstens {MAX_EIGENE}.",
            eigene.len()
        ));
    }
    for n in &eigene {
        if !n
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-._".contains(c))
        {
            return Err(format!(
                "„{n}“ ist kein gültiger Domainname. Erlaubt sind Buchstaben, Ziffern, Punkt und Bindestrich."
            ));
        }
        dns::build_query(0, n, dns::A, dns::IN)?;
    }
    let mut alle = eigene;
    for f in FEST {
        if !alle.iter().any(|n| n == f) {
            alle.push(f.to_string());
        }
    }
    let gekuerzt = alle.len() > MAX_NAMEN;
    alle.truncate(MAX_NAMEN);
    Ok((alle, gekuerzt))
}

pub struct Ergebnis {
    pub zeilen: Vec<Zeile>,
    pub empfehlung: Option<usize>,
}

/// Misst System-DNS und die drei öffentlichen Server nacheinander.
pub fn tempo(namen: &[String], system: IpAddr) -> Ergebnis {
    let mut ziele = vec![(format!("deinem Netz ({system})"), system, false)];
    ziele.extend(
        OEFFENTLICH
            .iter()
            .map(|(n, ip)| (n.to_string(), IpAddr::from(*ip), true)),
    );
    let zeilen: Vec<Zeile> = ziele
        .iter()
        .map(|(n, ip, oeff)| auswerten(n, *oeff, &messen(SocketAddr::new(*ip, 53), namen, PAUSE)))
        .collect();
    Ergebnis {
        empfehlung: empfehlung(&zeilen),
        zeilen,
    }
}

/// Gespeichert werden nur Mediane und der Zeitpunkt.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Lauf {
    pub zeit: u64,
    pub server: Vec<(String, Option<f64>, Option<f64>)>,
}

type Store = HashMap<String, Vec<Lauf>>;

fn ablegen(store: &mut Store, key: &str, zeilen: &[Zeile], zeit: u64) {
    let l = store.entry(key.to_string()).or_default();
    l.push(Lauf {
        zeit,
        server: zeilen
            .iter()
            .map(|z| (z.name.clone(), z.bekannt_median, z.unbekannt_median))
            .collect(),
    });
    let zu_viel = l.len().saturating_sub(MAX_LAEUFE);
    l.drain(..zu_viel);
}

/// Speicherdatei und Schlüssel des aktuellen Netzes, wenn es gemerkt ist.
fn ziel() -> Option<(PathBuf, String)> {
    let fp = netze::fingerprint(&hier::collect().ok()?).ok()?;
    let pfad = netze::data_path().ok()?;
    netze::find(&netze::load(&pfad).ok()?, &fp)?;
    Some((pfad.with_file_name("dnstempo.json"), fp.key()))
}

fn ms(v: Option<f64>) -> String {
    v.map_or("-".into(), |x| format!("{x:.0} ms"))
}

fn datenschutz() -> String {
    format!(
        "Datenschutz: Die öffentlichen Anbieter ({}) sehen die Namen, die du abfragst.",
        OEFFENTLICH.map(|(n, _)| n).join(", ")
    )
}

fn urteil(e: &Ergebnis) -> (String, &'static str) {
    match e.empfehlung {
        Some(i) => (
            format!(
                "Empfehlung: {} antwortet auf bekannte und neue Namen mindestens 30 % schneller als dein Netz.",
                e.zeilen[i].name
            ),
            "Ein Wechsel lohnt, weil der Server auf beiden Gruppen deutlich schneller ist und nicht öfter ausfällt. Bedenke: Schneller zu sein ist nicht das Einzige, was zählt. Quad9 filtert zum Beispiel bekannte Schadseiten, Cloudflare nicht, und jeder öffentliche Anbieter sieht deine Namen.",
        ),
        None => (
            "Kein Wechsel nötig.".to_string(),
            "Kein öffentlicher Server ist auf beiden Gruppen mindestens 30 % schneller als dein Netz oder er fällt öfter aus. Ein kleiner Unterschied von wenigen Millisekunden fällt beim Surfen nicht auf.",
        ),
    }
}

fn ausgeben(e: &Ergebnis, gekuerzt: bool, explain_on: bool) {
    println!(
        "{:<28} {:>17} {:>17} {:>8} {:>10}",
        "Server", "bekannt (P50/P95)", "neu (P50/P95)", "Fehler", "Ausreißer"
    );
    for z in &e.zeilen {
        println!(
            "{:<28} {:>17} {:>17} {:>7.0}% {:>10}",
            z.name,
            format!("{} / {}", ms(z.bekannt_median), ms(z.bekannt_p95)),
            format!("{} / {}", ms(z.unbekannt_median), ms(z.unbekannt_p95)),
            z.fehlerrate * 100.0,
            z.ausreisser
        );
    }
    if gekuerzt {
        println!(
            "Hinweis: Wegen der Obergrenze von {MAX_ABFRAGEN} Abfragen je Server zählen nur die ersten {MAX_NAMEN} Namen."
        );
    }
    println!("{}", datenschutz());
    let (satz, erklaerung) = urteil(e);
    println!("{satz}");
    explain(explain_on, erklaerung);
}

pub fn run(args: &[String], explain_on: bool) -> Result<(), String> {
    let usage = "Aufruf: netzinfo dns --tempo [--namen a.de,b.de] (höchstens 50 eigene Namen)";
    let mut eigene = String::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--namen" => {
                eigene = it
                    .next()
                    .ok_or_else(|| format!("Nach --namen fehlt die Liste. {usage}"))?
                    .clone();
            }
            _ => return Err(format!("Unbekannte Angabe „{a}“. {usage}")),
        }
    }
    let (namen, gekuerzt) = namenliste(&eigene)?;
    explain(
        explain_on,
        &format!(
            "Ich frage vier DNS-Server nacheinander: deinen im Netz und drei öffentliche. Je Server erst {} bekannte Namen zweimal (nur die zweite Runde zählt, sie kommt meist aus dem Zwischenspeicher), dann {UNBEKANNT} zufällige Namen unter example.com, die niemand kennt. Das zeigt die Zeit für eine echte Auflösung. Zwischen zwei Abfragen liegen 50 ms, mehr als {MAX_ABFRAGEN} Abfragen je Server sendet netzinfo nie.",
            namen.len()
        ),
    );
    let e = messen_und_merken(&namen)?;
    ausgeben(&e, gekuerzt, explain_on);
    Ok(())
}

fn messen_und_merken(namen: &[String]) -> Result<Ergebnis, String> {
    let e = tempo(namen, dns::system_resolver()?);
    if let Some((pfad, key)) = ziel() {
        let mut store: Store = netze::load_json(&pfad)?;
        ablegen(&mut store, &key, &e.zeilen, netze::now());
        netze::save(&pfad, &store)?;
    }
    Ok(e)
}

/// Was die App zeigt (Tabelle, Urteil, Datenschutzhinweis), Issue #50.
#[derive(Serialize)]
pub struct Anzeige {
    pub zeilen: Vec<Zeile>,
    pub empfehlung: Option<usize>,
    pub satz: String,
    pub erklaerung: &'static str,
    pub datenschutz: String,
}

/// Misst mit der festen Namensliste und legt wie die Kommandozeile ab.
pub fn daten() -> Result<Anzeige, String> {
    let (namen, _) = namenliste("")?;
    let e = messen_und_merken(&namen)?;
    let (satz, erklaerung) = urteil(&e);
    Ok(Anzeige {
        satz,
        erklaerung,
        datenschutz: datenschutz(),
        empfehlung: e.empfehlung,
        zeilen: e.zeilen,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering::SeqCst};

    /// Fake-DNS auf localhost: antwortet nach `verzoegerung` mit Rückgabecode `rcode`.
    /// Gibt Adresse, Zähler und Stopp-Schalter zurück.
    fn fake(verzoegerung: Duration, rcode: u8) -> (SocketAddr, Arc<AtomicUsize>, Arc<AtomicBool>) {
        let sock = UdpSocket::bind("127.0.0.1:0").unwrap();
        sock.set_read_timeout(Some(Duration::from_millis(50)))
            .unwrap();
        let adr = sock.local_addr().unwrap();
        let (zaehler, stopp) = (
            Arc::new(AtomicUsize::new(0)),
            Arc::new(AtomicBool::new(false)),
        );
        let (z, s) = (zaehler.clone(), stopp.clone());
        std::thread::spawn(move || {
            let mut buf = [0u8; 512];
            while !s.load(SeqCst) {
                let Ok((n, von)) = sock.recv_from(&mut buf) else {
                    continue;
                };
                z.fetch_add(1, SeqCst);
                sleep(verzoegerung);
                let mut r = buf[..n].to_vec();
                r[2] = 0x81;
                r[3] = 0x80 | rcode;
                let _ = sock.send_to(&r, von);
            }
        });
        (adr, zaehler, stopp)
    }

    fn namen(n: usize) -> Vec<String> {
        (0..n).map(|i| format!("n{i}.example.net")).collect()
    }

    #[test]
    fn misst_und_wertet_aus() {
        let (adr, zaehler, stopp) = fake(Duration::from_millis(5), 0);
        let r = messen(adr, &namen(4), Duration::ZERO);
        stopp.store(true, SeqCst);
        assert_eq!((r.bekannt.len(), r.unbekannt.len(), r.fehler), (4, 5, 0));
        assert_eq!(zaehler.load(SeqCst), 4 * 2 + 5);
        let z = auswerten("fake", false, &r);
        assert!(z.bekannt_median.unwrap() >= 5.0);
        assert!(z.bekannt_p95.unwrap() >= z.bekannt_median.unwrap());
        assert_eq!(z.fehlerrate, 0.0);
    }

    #[test]
    fn obergrenze_60_abfragen() {
        let (adr, zaehler, stopp) = fake(Duration::ZERO, 0);
        // absichtlich 50 Namen direkt an `messen`: es muss selbst kürzen
        let r = messen(adr, &namen(50), Duration::ZERO);
        stopp.store(true, SeqCst);
        let gesendet = zaehler.load(SeqCst);
        assert!(gesendet <= MAX_ABFRAGEN, "{gesendet}");
        assert_eq!(r.bekannt.len(), MAX_NAMEN);
        let (liste, gekuerzt) = namenliste(&namen(50).join(",")).unwrap();
        assert_eq!((liste.len(), gekuerzt), (MAX_NAMEN, true));
    }

    #[test]
    fn nxdomain_gilt_servfail_ist_fehler() {
        let (adr, _, stopp) = fake(Duration::ZERO, 3);
        let r = messen(adr, &namen(2), Duration::ZERO);
        stopp.store(true, SeqCst);
        assert_eq!(r.fehler, 0);
        let (adr, _, stopp) = fake(Duration::ZERO, 2);
        let r = messen(adr, &namen(2), Duration::ZERO);
        stopp.store(true, SeqCst);
        assert_eq!((r.fehler, r.bekannt.len()), (2 + UNBEKANNT, 0));
        assert_eq!(auswerten("x", false, &r).fehlerrate, 1.0);
    }

    fn zeile(b: f64, u: f64, fehler: f64) -> Zeile {
        Zeile {
            name: "z".into(),
            oeffentlich: true,
            bekannt_median: Some(b),
            bekannt_p95: Some(b),
            unbekannt_median: Some(u),
            unbekannt_p95: Some(u),
            fehlerrate: fehler,
            ausreisser: 0,
        }
    }

    #[test]
    fn schwelle_29_nein_31_ja() {
        let sys = zeile(100.0, 200.0, 0.0);
        assert_eq!(empfehlung(&[sys.clone(), zeile(71.0, 142.0, 0.0)]), None);
        assert_eq!(empfehlung(&[sys.clone(), zeile(69.0, 138.0, 0.0)]), Some(1));
        assert_eq!(empfehlung(&[sys.clone(), zeile(70.0, 140.0, 0.0)]), Some(1));
        // nur eine Gruppe schneller reicht nicht
        assert_eq!(empfehlung(&[sys.clone(), zeile(50.0, 190.0, 0.0)]), None);
        // mehr Fehler als das System-DNS
        assert_eq!(empfehlung(&[sys.clone(), zeile(50.0, 100.0, 0.1)]), None);
        // der Schnellste gewinnt
        let zs = [sys, zeile(60.0, 120.0, 0.0), zeile(40.0, 100.0, 0.0)];
        assert_eq!(empfehlung(&zs), Some(2));
    }

    #[test]
    fn namen_pruefen_und_speicher_kuerzen() {
        assert!(namenliste(&namen(51).join(",")).is_err());
        assert!(namenliste("kein name").is_err());
        let (l, k) = namenliste(" Eigen.Example.net ,example.com").unwrap();
        assert_eq!(
            (l[0].as_str(), l.len(), k),
            ("eigen.example.net", 21, false)
        );
        let mut st = Store::new();
        for t in 0..7 {
            ablegen(&mut st, "k", &[zeile(1.0, 2.0, 0.0)], t);
        }
        let v = &st["k"];
        assert_eq!((v.len(), v[0].zeit, v[4].zeit), (5, 2, 6));
    }
}
