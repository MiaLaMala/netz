//! `netzinfo mtu`: largest packet that reaches a target without being split, see issue #47.
//! One ICMP datagram socket (no root on macOS) sends echo requests of rising size with the
//! „Don't Fragment“ bit set; a binary search finds the largest size that is answered.
//! Received packets are untrusted: every read is bounds-checked.
//! ponytail: IPv4 only (IPv6 needs ICMPv6 and IPV6_DONTFRAG), Windows is not built.

use std::collections::BTreeMap;
use std::net::{IpAddr, Ipv4Addr};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::pfad::{be16, checksum, resolve4, strip_ip};
use crate::pruefen::Klasse;
use crate::{explain, hier, netze};

/// IPv4 header (20) plus ICMP header (8): packet size minus this is the echo payload.
#[cfg(unix)]
const KOPF: u16 = 28;
const KLEINSTE: u16 = 576;
const GROESSTE: u16 = 1500;
const VERSUCHE: u8 = 2;
/// Measurements kept per network.
const BEHALTEN: usize = 10;

/// What one probe of one size came back with.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Antwort {
    Echo,
    /// Too big: „Fragmentation Needed“ (type 3, code 4) or the local „Message too long“. The
    /// router may name the size it can pass.
    ZuGross(Option<u16>),
    Stille,
}

/// Where the upper limit came from.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Quelle {
    /// A device or the Mac itself said „too big“ (or the biggest size simply passed).
    Antwort,
    /// Bigger packets just got no answer. Could also be loss or a filter.
    Stille,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Einstufung {
    Normal,
    Pppoe,
    Tunnel,
    Klein,
    Ungewoehnlich,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MtuErgebnis {
    pub ziel: String,
    pub ziel_ip: IpAddr,
    /// Largest IP packet in bytes that arrived unsplit.
    pub pfad_mtu: u16,
    /// MTU of the interface towards the target, if it could be read.
    pub lokale_mtu: Option<u16>,
    pub quelle: Quelle,
    pub einstufung: Einstufung,
    /// German explanation of `pfad_mtu`, ready to show.
    pub satz: String,
    /// Unix seconds.
    pub zeitpunkt: u64,
    /// Name of the remembered network the value was stored under.
    pub netz: Option<String>,
    /// Value of the visit before, same target.
    pub vorher: Option<u16>,
}

pub fn einstufen(mtu: u16) -> Einstufung {
    match mtu {
        1500.. => Einstufung::Normal,
        1492 => Einstufung::Pppoe,
        1480 | 1472 => Einstufung::Tunnel,
        ..=1420 => Einstufung::Klein,
        _ => Einstufung::Ungewoehnlich,
    }
}

/// Explanation, not proof: the same number can have other causes.
fn erklaerung(mtu: u16, lokal: Option<u16>, quelle: Quelle) -> String {
    let mut s = match einstufen(mtu) {
        Einstufung::Normal => {
            format!("Dein Netz lässt Pakete bis {mtu} Byte durch. Das ist der Normalwert.")
        }
        Einstufung::Pppoe => format!(
            "Dein Netz lässt Pakete bis {mtu} Byte durch statt 1500. Das ist bei DSL mit PPPoE üblich und normalerweise kein Fehler."
        ),
        Einstufung::Tunnel => format!(
            "Dein Netz lässt Pakete bis {mtu} Byte durch statt 1500. Das passt zu einem Tunnel auf dem Weg (zum Beispiel IPv6 über IPv4) und ist normalerweise kein Fehler."
        ),
        Einstufung::Klein => format!(
            "Bei {mtu} Byte ist ein Tunnel oder Mobilfunk im Weg. Große Dateien über manche Seiten können dann hängen."
        ),
        Einstufung::Ungewoehnlich => format!(
            "Dein Netz lässt Pakete bis {mtu} Byte durch. Das ist ein ungewöhnlicher Wert, vermutlich ist ein Tunnel oder VPN im Weg. Wenn Seiten halb laden oder ein VPN hängt, kann das die Ursache sein."
        ),
    };
    if lokal == Some(mtu) && mtu < GROESSTE {
        s += " Die Grenze liegt schon an deiner eigenen Schnittstelle, zum Beispiel einem VPN.";
    }
    if quelle == Quelle::Stille {
        s += " Das Ergebnis ist unsicher: Größere Pakete bekamen keine Antwort, und manche Netze verwerfen die Meldung „Paket zu groß“ ganz.";
    }
    s
}

// ---- packets -------------------------------------------------------------------------------

/// Echo request (type 8) with `daten` payload bytes.
#[cfg(unix)]
fn echo(id: u16, seq: u16, daten: usize) -> Vec<u8> {
    let mut p = vec![8, 0, 0, 0];
    p.extend_from_slice(&id.to_be_bytes());
    p.extend_from_slice(&seq.to_be_bytes());
    p.resize(8 + daten, 0x6e);
    let c = checksum(&p);
    p[2..4].copy_from_slice(&c.to_be_bytes());
    p
}

/// Echo reply (type 0) or „Fragmentation Needed“ (type 3, code 4, quoting our request) with
/// the sequence number of our request. Anything else, truncated or garbage gives `None`.
#[cfg(unix)]
fn parse(buf: &[u8]) -> Option<(u16, Antwort)> {
    let icmp = strip_ip(buf)?;
    match (*icmp.first()?, *icmp.get(1)?) {
        (0, 0) => Some((be16(icmp, 6)?, Antwort::Echo)),
        (3, 4) => {
            let hinweis = be16(icmp, 6)?;
            // 8 bytes ICMP header, then the quoted IP header and the first 8 bytes of our request
            let quoted = icmp.get(8..)?;
            if quoted.first()? >> 4 != 4 {
                return None;
            }
            let inner = strip_ip(quoted)?;
            (*inner.first()? == 8).then_some((
                be16(inner, 6)?,
                Antwort::ZuGross((hinweis != 0).then_some(hinweis)),
            ))
        }
        _ => None,
    }
}

// ---- search --------------------------------------------------------------------------------

/// One method so tests can fake the network. `groesse` is the whole IP packet in bytes.
trait Sender {
    fn probe(&mut self, groesse: u16) -> Antwort;
}

/// Up to `VERSUCHE` tries; only silence is retried.
fn pruefe(s: &mut dyn Sender, groesse: u16) -> Antwort {
    let mut a = Antwort::Stille;
    for _ in 0..VERSUCHE {
        a = s.probe(groesse);
        if a != Antwort::Stille {
            break;
        }
    }
    a
}

/// Binary search between `kleinste` (must pass) and `groesste`. A „too big“ with a size hint
/// is tried next: the hint is usually the answer.
fn suche(s: &mut dyn Sender, kleinste: u16, groesste: u16) -> Result<(u16, Quelle), String> {
    let mut hinweis = None;
    let mut quelle = match pruefe(s, groesste) {
        Antwort::Echo => return Ok((groesste, Quelle::Antwort)),
        Antwort::Stille => Quelle::Stille,
        Antwort::ZuGross(h) => {
            hinweis = h;
            Quelle::Antwort
        }
    };
    if pruefe(s, kleinste) != Antwort::Echo {
        return Err(format!(
            "Schon {kleinste} Byte große Pakete kommen nicht durch. Entweder blockt das Ziel Ping, oder der Weg ist gestört; die Paketgröße lässt sich so nicht bestimmen. Probiere ein anderes Ziel, zum Beispiel 1.1.1.1."
        ));
    }
    let (mut lo, mut hi) = (kleinste, groesste);
    while hi - lo > 1 {
        let aus_hinweis = hinweis.take().filter(|m| (lo + 1..hi).contains(m));
        let n = aus_hinweis.unwrap_or(lo + (hi - lo) / 2);
        match pruefe(s, n) {
            Antwort::Echo => {
                lo = n;
                if aus_hinweis.is_some() {
                    hi = lo + 1; // the router said it passes exactly this much
                    quelle = Quelle::Antwort;
                }
            }
            Antwort::ZuGross(h) => (hi, quelle, hinweis) = (n, Quelle::Antwort, h),
            Antwort::Stille => (hi, quelle) = (n, Quelle::Stille),
        }
    }
    Ok((lo, quelle))
}

// ---- real socket ---------------------------------------------------------------------------

#[cfg(unix)]
mod echt {
    use super::*;
    use socket2::{Domain, Protocol, Socket, Type};
    use std::net::{SocketAddr, UdpSocket};
    use std::os::fd::AsRawFd;
    use std::time::{Duration, Instant};

    const WARTEN: Duration = Duration::from_secs(1);

    pub(super) struct Echt {
        sock: UdpSocket,
        to: SocketAddr,
        id: u16,
        seq: u16,
        /// Last send error that was not „too big“: kept to explain a failed search.
        pub(super) fehler: Option<String>,
    }

    impl Echt {
        pub(super) fn open(target: Ipv4Addr) -> Result<Self, String> {
            let s = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::ICMPV4)).map_err(|e| {
                format!("Kann keinen ICMP-Socket öffnen ({e}). Auf dem Mac geht das ohne sudo; prüfe, ob eine Firewall oder ein Profil das blockt. Unter Linux muss `net.ipv4.ping_group_range` deine Gruppe enthalten.")
            })?;
            // macOS: IP_DONTFRAG on; Linux: IP_MTU_DISCOVER = IP_PMTUDISC_DO
            #[cfg(target_os = "macos")]
            let (opt, wert) = (libc::IP_DONTFRAG, 1 as libc::c_int);
            #[cfg(not(target_os = "macos"))]
            let (opt, wert) = (libc::IP_MTU_DISCOVER, libc::IP_PMTUDISC_DO);
            // SAFETY: the fd is open as long as `s`; `wert` is a c_int of the stated size.
            let rc = unsafe {
                libc::setsockopt(
                    s.as_raw_fd(),
                    libc::IPPROTO_IP,
                    opt,
                    (&raw const wert).cast(),
                    size_of::<libc::c_int>() as libc::socklen_t,
                )
            };
            if rc != 0 {
                return Err(format!(
                    "Das „Nicht zerlegen“-Bit ließ sich nicht setzen ({}). Ohne das ist keine Messung möglich.",
                    std::io::Error::last_os_error()
                ));
            }
            Ok(Echt {
                // the std type gives a safe receive buffer; the fd stays the ICMP socket
                sock: UdpSocket::from(s),
                to: SocketAddr::new(target.into(), 0),
                id: std::process::id() as u16,
                seq: 0,
                fehler: None,
            })
        }
    }

    impl Sender for Echt {
        fn probe(&mut self, groesse: u16) -> Antwort {
            self.seq = self.seq.wrapping_add(1);
            let paket = echo(self.id, self.seq, usize::from(groesse - KOPF));
            if let Err(e) = self.sock.send_to(&paket, self.to) {
                if e.raw_os_error() == Some(libc::EMSGSIZE) {
                    return Antwort::ZuGross(None);
                }
                self.fehler = Some(format!("Senden schlug fehl ({e})."));
                return Antwort::Stille;
            }
            let ende = Instant::now() + WARTEN;
            let mut buf = [0u8; 2048];
            loop {
                let Some(rest) = ende.checked_duration_since(Instant::now()) else {
                    return Antwort::Stille;
                };
                let rest = rest.max(Duration::from_millis(1));
                if self.sock.set_read_timeout(Some(rest)).is_err() {
                    return Antwort::Stille;
                }
                match self.sock.recv_from(&mut buf) {
                    // an echo reply must come from the target, a „too big“ from any router
                    Ok((n, from)) => match parse(&buf[..n]) {
                        Some((seq, a))
                            if seq == self.seq
                                && (a != Antwort::Echo || from.ip() == self.to.ip()) =>
                        {
                            return a;
                        }
                        _ => {}
                    },
                    // Linux reports the router's „too big“ as a socket error
                    Err(e) if e.raw_os_error() == Some(libc::EMSGSIZE) => {
                        return Antwort::ZuGross(None);
                    }
                    Err(_) => return Antwort::Stille,
                }
            }
        }
    }

    /// MTU of the interface that sends towards `ziel`; `None` if anything on the way fails.
    pub(super) fn lokale_mtu(ziel: Ipv4Addr) -> Option<u16> {
        use std::ffi::CStr;
        let u = UdpSocket::bind("0.0.0.0:0").ok()?;
        u.connect((ziel, 9)).ok()?; // sends nothing, only picks the route
        let IpAddr::V4(ich) = u.local_addr().ok()?.ip() else {
            return None;
        };
        // SAFETY: the list from getifaddrs is only read and freed once; names are NUL
        // terminated C strings; ifreq is plain data that the kernel fills in.
        unsafe {
            let mut liste: *mut libc::ifaddrs = std::ptr::null_mut();
            if libc::getifaddrs(&mut liste) != 0 {
                return None;
            }
            let mut name = None;
            let mut p = liste;
            while !p.is_null() {
                let a = &*p;
                if !a.ifa_addr.is_null() && i32::from((*a.ifa_addr).sa_family) == libc::AF_INET {
                    let sin = &*a.ifa_addr.cast::<libc::sockaddr_in>();
                    if Ipv4Addr::from(u32::from_be(sin.sin_addr.s_addr)) == ich {
                        name = Some(CStr::from_ptr(a.ifa_name).to_owned());
                        break;
                    }
                }
                p = a.ifa_next;
            }
            libc::freeifaddrs(liste);
            let name = name?;
            let mut req: libc::ifreq = std::mem::zeroed();
            for (d, s) in req.ifr_name.iter_mut().zip(name.as_bytes_with_nul()) {
                *d = *s as libc::c_char;
            }
            if libc::ioctl(u.as_raw_fd(), libc::SIOCGIFMTU as _, &mut req) != 0 {
                return None;
            }
            u16::try_from(req.ifr_ifru.ifru_mtu).ok()
        }
    }
}

#[cfg(unix)]
fn messen(ziel: Ipv4Addr) -> Result<(u16, Quelle, Option<u16>), String> {
    let mut e = echt::Echt::open(ziel)?;
    match suche(&mut e, KLEINSTE, GROESSTE) {
        Ok((m, q)) => Ok((m, q, echt::lokale_mtu(ziel))),
        Err(t) => Err(e.fehler.take().unwrap_or(t)),
    }
}

#[cfg(not(unix))]
fn messen(_: Ipv4Addr) -> Result<(u16, Quelle, Option<u16>), String> {
    Err("Auf Windows kann `netzinfo mtu` noch nicht messen. Auf einem Mac oder unter Linux geht es.".into())
}

// ---- store ---------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Messung {
    ziel: String,
    pfad: u16,
    zeitpunkt: u64,
}

type Store = BTreeMap<String, Vec<Messung>>;

fn store_path(daten: &Path) -> PathBuf {
    daten.with_file_name("mtu.json")
}

/// Appends `m`; returns the value of the visit before (same target).
fn anhaengen(store: &mut Store, key: &str, m: Messung) -> Option<u16> {
    let liste = store.entry(key.to_string()).or_default();
    let vorher = liste
        .iter()
        .rev()
        .find(|x| x.ziel == m.ziel)
        .map(|x| x.pfad);
    liste.push(m);
    let zuviel = liste.len().saturating_sub(BEHALTEN);
    liste.drain(..zuviel);
    vorher
}

fn unterschiede(store: &Store, key: &str) -> Vec<(Klasse, String)> {
    let Some((neu, rest)) = store.get(key).and_then(|l| l.split_last()) else {
        return Vec::new();
    };
    let Some(alt) = rest.iter().rev().find(|x| x.ziel == neu.ziel) else {
        return Vec::new();
    };
    if neu.pfad == alt.pfad {
        return Vec::new();
    }
    vec![(
        Klasse::Auffaellig,
        format!(
            "Die größte Paketgröße zum Ziel {} war beim letzten Besuch {} Byte, jetzt sind es {} Byte. Ein anderer Anschluss, ein Tunnel oder ein VPN kann das erklären.",
            neu.ziel, alt.pfad, neu.pfad
        ),
    )]
}

/// Findings for `netzinfo pruefen`; `daten` is the path of netze.json, `key` the network key.
/// Sends nothing: compares the last two stored measurements of one target.
pub fn fuer_pruefen(daten: &Path, key: &str, _meins: bool) -> Vec<(Klasse, String)> {
    netze::load_json::<Store>(&store_path(daten))
        .map(|s| unterschiede(&s, key))
        .unwrap_or_default()
}

// ---- public API ----------------------------------------------------------------------------

/// Measures the path MTU to `ziel` (IPv4 address or name). In a remembered network the value
/// is stored.
pub fn mtu_daten(ziel: &str) -> Result<MtuErgebnis, String> {
    if ziel.parse::<IpAddr>().is_ok_and(|ip| ip.is_ipv6()) {
        return Err("IPv6 kann `netzinfo mtu` noch nicht. Gib ein IPv4-Ziel an.".into());
    }
    let ip = resolve4(ziel)?;
    let (pfad_mtu, quelle, lokale_mtu) = messen(ip)?;
    let zeitpunkt = netze::now();
    let daten = netze::data_path()?;
    let bekannt = hier::collect().ok().and_then(|info| {
        let fp = netze::fingerprint(&info).ok()?;
        let list = netze::load(&daten).ok()?;
        netze::find(&list, &fp).map(|i| (fp.key(), list[i].name.clone()))
    });
    let (netz, vorher) = match bekannt {
        Some((key, name)) => {
            let pfad = store_path(&daten);
            let mut store: Store = netze::load_json(&pfad)?;
            let m = Messung {
                ziel: ip.to_string(),
                pfad: pfad_mtu,
                zeitpunkt,
            };
            let vorher = anhaengen(&mut store, &key, m);
            netze::save(&pfad, &store)?;
            (Some(name), vorher)
        }
        None => (None, None),
    };
    Ok(MtuErgebnis {
        ziel: ziel.to_string(),
        ziel_ip: IpAddr::V4(ip),
        pfad_mtu,
        lokale_mtu,
        quelle,
        einstufung: einstufen(pfad_mtu),
        satz: erklaerung(pfad_mtu, lokale_mtu, quelle),
        zeitpunkt,
        netz,
        vorher,
    })
}

pub fn run(args: &[String], explain_on: bool) -> Result<(), String> {
    let mut ziel = None;
    for a in args {
        match a.as_str() {
            f if f.starts_with("--") => return Err(format!("Unbekannte Option „{f}“.")),
            _ if ziel.is_none() => ziel = Some(a.as_str()),
            _ => return Err("Es passt nur ein Ziel: `netzinfo mtu <Ziel>`.".into()),
        }
    }
    let ziel = ziel.unwrap_or("1.1.1.1");
    println!("Messe die größte Paketgröße zu {ziel}. Das dauert meist unter 15 Sekunden.");
    explain(
        explain_on,
        "Jedes Netzgerät hat eine Obergrenze, wie große Pakete es weitergibt (MTU). Ich schicke Ping-Pakete mit dem Zeichen „Nicht zerlegen“. Ist ein Paket zu groß, wird es nicht zerlegt, sondern abgewiesen. Das braucht kein sudo.",
    );
    explain(
        explain_on,
        "Ich probiere nicht alle Größen aus, sondern halbiere den Bereich von 576 bis 1500 Byte immer wieder: kam das Paket an, geht es nach oben, sonst nach unten. Nach etwa zehn Schritten steht die Grenze fest. Nennt ein Router seine Grenze selbst, springe ich direkt dorthin.",
    );
    let r = mtu_daten(ziel)?;
    println!("Ziel {}", r.ziel_ip);
    println!("Größte Paketgröße ohne Zerlegen: {} Byte", r.pfad_mtu);
    if let Some(l) = r.lokale_mtu {
        println!("Grenze deiner Schnittstelle zum Ziel: {l} Byte");
    }
    println!("\n{}", r.satz);
    match (&r.netz, r.vorher) {
        (Some(n), Some(v)) if v != r.pfad_mtu => println!(
            "\nGespeichert für „{n}“. Beim letzten Besuch waren es {v} Byte, das ist anders."
        ),
        (Some(n), _) => println!("\nGespeichert für „{n}“."),
        (None, _) => println!(
            "\nDieses Netz ist nicht gemerkt, es wird nichts gespeichert (`netzinfo merken <Name>`)."
        ),
    }
    explain(
        explain_on,
        "Normal sind 1500 Byte. Weniger bedeutet, dass auf dem Weg etwas Platz für eigene Kopfdaten braucht: DSL mit PPPoE (1492), ein Tunnel oder VPN, Mobilfunk. Das ist eine Erklärung, kein Beweis.",
    );
    explain(
        explain_on,
        "Manche Netze verwerfen die Meldung „Paket zu groß“. Dann sieht die Messung aus wie Funkstille, und das Ergebnis gilt als unsicher. Der Wert gilt nur für dieses Ziel und den Weg dorthin.",
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Passes everything up to `mtu`; bigger packets get `ueber`.
    struct Fake {
        mtu: u16,
        ueber: Antwort,
        gesendet: Vec<u16>,
    }

    impl Sender for Fake {
        fn probe(&mut self, groesse: u16) -> Antwort {
            self.gesendet.push(groesse);
            if groesse <= self.mtu {
                Antwort::Echo
            } else {
                self.ueber
            }
        }
    }

    fn fake(mtu: u16, ueber: Antwort) -> Fake {
        Fake {
            mtu,
            ueber,
            gesendet: vec![],
        }
    }

    #[test]
    fn binary_search_finds_every_mtu_without_hint() {
        for mtu in [576, 1280, 1400, 1492, 1499] {
            let mut f = fake(mtu, Antwort::ZuGross(None));
            assert_eq!(
                suche(&mut f, 576, 1500),
                Ok((mtu, Quelle::Antwort)),
                "{mtu}"
            );
            assert!(f.gesendet.len() <= 12, "{mtu}: {} Pakete", f.gesendet.len());
        }
        let mut f = fake(1500, Antwort::ZuGross(None));
        assert_eq!(suche(&mut f, 576, 1500), Ok((1500, Quelle::Antwort)));
        assert_eq!(f.gesendet, [1500]);
    }

    #[test]
    fn hint_from_router_shortens_the_search() {
        let mut f = fake(1492, Antwort::ZuGross(Some(1492)));
        assert_eq!(suche(&mut f, 576, 1500), Ok((1492, Quelle::Antwort)));
        assert_eq!(f.gesendet, [1500, 576, 1492]);
        // a wrong hint (bigger than the real limit) is only tried, then the search goes on
        let mut f = fake(1400, Antwort::ZuGross(Some(1450)));
        assert_eq!(suche(&mut f, 576, 1500), Ok((1400, Quelle::Antwort)));
    }

    #[test]
    fn silence_gives_the_value_but_marks_it_unsure() {
        let mut f = fake(1400, Antwort::Stille);
        assert_eq!(suche(&mut f, 576, 1500), Ok((1400, Quelle::Stille)));
        // every size above the limit is tried twice
        let zu_gross = f.gesendet.iter().filter(|g| **g > 1400).count();
        assert!(zu_gross >= 2 && zu_gross % 2 == 0);
        assert!(erklaerung(1400, None, Quelle::Stille).contains("unsicher"));
    }

    #[test]
    fn nothing_arrives_is_an_error_not_a_value() {
        let mut f = fake(0, Antwort::Stille);
        let e = suche(&mut f, 576, 1500).unwrap_err();
        assert!(e.contains("576"), "{e}");
        assert_eq!(f.gesendet.len(), 4); // 1500 and 576, twice each
    }

    #[test]
    fn classifies_known_values() {
        use Einstufung::*;
        for (mtu, want) in [
            (1500, Normal),
            (1492, Pppoe),
            (1480, Tunnel),
            (1472, Tunnel),
            (1420, Klein),
            (1350, Klein),
            (1280, Klein),
            (1450, Ungewoehnlich),
            (1496, Ungewoehnlich),
        ] {
            assert_eq!(einstufen(mtu), want, "{mtu}");
        }
        assert!(erklaerung(1492, None, Quelle::Antwort).contains("PPPoE"));
        assert!(erklaerung(1350, None, Quelle::Antwort).contains("Tunnel oder Mobilfunk"));
        assert!(erklaerung(1280, Some(1280), Quelle::Antwort).contains("eigenen Schnittstelle"));
    }

    fn m(ziel: &str, pfad: u16, zeitpunkt: u64) -> Messung {
        Messung {
            ziel: ziel.into(),
            pfad,
            zeitpunkt,
        }
    }

    #[test]
    fn deviation_from_last_visit_is_flagged() {
        let mut s = Store::new();
        assert_eq!(anhaengen(&mut s, "a", m("192.0.2.1", 1500, 1)), None);
        assert!(unterschiede(&s, "a").is_empty()); // first visit: nothing to compare
        // another target does not count as the visit before
        assert_eq!(anhaengen(&mut s, "a", m("203.0.113.5", 1400, 2)), None);
        assert!(unterschiede(&s, "a").is_empty());
        assert_eq!(anhaengen(&mut s, "a", m("192.0.2.1", 1500, 3)), Some(1500));
        assert!(unterschiede(&s, "a").is_empty());
        assert_eq!(anhaengen(&mut s, "a", m("192.0.2.1", 1492, 4)), Some(1500));
        let u = unterschiede(&s, "a");
        assert_eq!(u.len(), 1);
        assert_eq!(u[0].0, Klasse::Auffaellig);
        assert!(
            u[0].1.contains("1500") && u[0].1.contains("1492"),
            "{}",
            u[0].1
        );
        assert!(unterschiede(&s, "anderes-netz").is_empty());
    }

    #[test]
    fn store_keeps_only_the_last_ten() {
        let mut s = Store::new();
        for i in 0..25 {
            anhaengen(&mut s, "a", m("192.0.2.1", 1500, i));
        }
        assert_eq!(s["a"].len(), BEHALTEN);
        assert_eq!(s["a"][0].zeitpunkt, 15);
    }

    #[test]
    fn fuer_pruefen_reads_mtu_json_next_to_the_data_file() {
        let dir = std::env::temp_dir().join(format!("netzinfo-mtu-{}", std::process::id()));
        let daten = dir.join("netze.json");
        assert!(fuer_pruefen(&daten, "a", false).is_empty()); // no file yet
        let mut s = Store::new();
        anhaengen(&mut s, "a", m("192.0.2.1", 1500, 1));
        anhaengen(&mut s, "a", m("192.0.2.1", 1420, 2));
        netze::save(&dir.join("mtu.json"), &s).unwrap();
        assert_eq!(fuer_pruefen(&daten, "a", false).len(), 1);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[cfg(unix)]
    mod paket {
        use super::*;

        #[test]
        fn echo_request_has_the_asked_size_and_valid_checksum() {
            let p = echo(0x1234, 7, 1472);
            assert_eq!(p.len(), 1480);
            assert_eq!(&p[..2], &[8, 0]);
            assert_eq!(&p[4..8], &[0x12, 0x34, 0, 7]);
            assert_eq!(checksum(&p), 0);
        }

        /// 20 byte IPv4 header (protocol 1) in front of `rest`.
        fn mit_ip(rest: &[u8]) -> Vec<u8> {
            let mut v = vec![
                0x45, 0, 0, 0, 0, 0, 0, 0, 64, 1, 0, 0, 192, 0, 2, 1, 192, 0, 2, 2,
            ];
            v.extend_from_slice(rest);
            v
        }

        fn zu_gross(seq: u16, hinweis: u16) -> Vec<u8> {
            let mut icmp = vec![3, 4, 0, 0, 0, 0];
            icmp.extend_from_slice(&hinweis.to_be_bytes());
            icmp.extend_from_slice(&mit_ip(&echo(1, seq, 8)[..8]));
            icmp
        }

        #[test]
        fn parses_echo_reply_and_fragmentation_needed() {
            let reply = [0, 0, 0, 0, 0x12, 0x34, 0x00, 0x07, 1, 2, 3];
            assert_eq!(parse(&reply), Some((7, Antwort::Echo)));
            assert_eq!(parse(&mit_ip(&reply)), Some((7, Antwort::Echo)));
            let want = Some((9, Antwort::ZuGross(Some(1492))));
            assert_eq!(parse(&zu_gross(9, 1492)), want);
            assert_eq!(parse(&mit_ip(&zu_gross(9, 1492))), want);
            assert_eq!(parse(&zu_gross(9, 0)), Some((9, Antwort::ZuGross(None))));
        }

        #[test]
        fn rejects_truncated_and_garbage_packets() {
            let full = mit_ip(&zu_gross(5, 1400));
            for n in 0..full.len() {
                assert_eq!(parse(&full[..n]), None, "len {n}");
            }
            assert_eq!(parse(&[0xff; 64]), None);
            assert_eq!(parse(&[0x40; 64]), None); // IHL 0
            assert_eq!(parse(&[3, 3, 0, 0, 0, 0, 0, 0]), None); // other code
            let mut other = zu_gross(5, 1400);
            other[8 + 20] = 3; // quoted packet is not an echo request
            assert_eq!(parse(&other), None);
        }
    }
}
