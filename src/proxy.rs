//! Proxy, PAC und WPAD im Netz erkennen, rein lesend. Issue #53.
//! Die PAC-Datei wird nie ausgeführt, nur auf ausdrücklichen Wunsch geladen und nach Hostnamen
//! durchsucht. An den Einstellungen des Systems ändert dieses Modul nichts.

use crate::pruefen::Klasse;
use crate::{dns, explain, hier, netze, portal};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::net::IpAddr;
use std::path::Path;

/// Was das System und das Netz an Proxy-Angaben liefern.
#[derive(Debug, Default, Clone, PartialEq, Serialize)]
pub struct ProxyBefund {
    /// Fest eingestellter Proxy als `host:port` (HTTP, HTTPS oder SOCKS).
    pub manuell: Option<String>,
    pub pac_url: Option<String>,
    /// WPAD-URL aus DHCP-Option 252.
    pub wpad_dhcp: Option<String>,
    /// `wpad.<domain>` hat im Netz eine Adresse. Die Datei wird nicht geladen.
    pub wpad_dns: bool,
    /// Der Mac sucht selbst nach einer Proxy-Konfiguration (Einstellung „Automatische Proxy-Erkennung“).
    pub auto_suche: bool,
}

impl ProxyBefund {
    fn keiner(&self) -> bool {
        self.manuell.is_none()
            && self.pac_url.is_none()
            && self.wpad_dhcp.is_none()
            && !self.wpad_dns
    }
}

/// Liest die Ausgabe von `scutil --proxy` (Zeilen `Schlüssel : Wert`). Leere Ausgabe: kein Proxy.
fn parse_scutil(out: &str) -> ProxyBefund {
    let werte: BTreeMap<&str, &str> = out
        .lines()
        .filter_map(|l| l.split_once(" : "))
        .map(|(k, v)| (k.trim(), v.trim()))
        .collect();
    let an = |k: &str| werte.get(k) == Some(&"1");
    let manuell = ["HTTP", "HTTPS", "SOCKS"].iter().find_map(|p| {
        let host = werte
            .get(format!("{p}Proxy").as_str())
            .filter(|h| !h.is_empty())?;
        an(&format!("{p}Enable")).then(|| match werte.get(format!("{p}Port").as_str()) {
            Some(port) => format!("{host}:{port}"),
            None => host.to_string(),
        })
    });
    ProxyBefund {
        manuell,
        pac_url: werte
            .get("ProxyAutoConfigURLString")
            .filter(|u| an("ProxyAutoConfigEnable") && !u.is_empty())
            .map(|u| u.to_string()),
        auto_suche: an("ProxyAutoDiscoveryEnable"),
        ..Default::default()
    }
}

/// Hostnamen hinter `PROXY`, `SOCKS`, `HTTP` und `HTTPS` in einer PAC-Datei. Nichts wird ausgeführt.
fn pac_hosts(text: &str) -> Vec<String> {
    let teile: Vec<&str> = text
        .split(|c: char| !(c.is_ascii_alphanumeric() || ".:-_".contains(c)))
        .filter(|t| !t.is_empty())
        .collect();
    let mut hosts: Vec<String> = Vec::new();
    for w in teile.windows(2) {
        if !matches!(
            w[0].to_ascii_uppercase().as_str(),
            "PROXY" | "SOCKS" | "SOCKS4" | "SOCKS5" | "HTTP" | "HTTPS"
        ) {
            continue;
        }
        let (host, port) = w[1].split_once(':').unwrap_or((w[1], ""));
        let port_ok = !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit());
        let host = host.to_ascii_lowercase();
        if (port_ok || host.contains('.')) && !hosts.contains(&host) && hosts.len() < 20 {
            hosts.push(host);
        }
    }
    hosts
}

fn wpad_per_dns(info: &hier::NetInfo) -> bool {
    let Some(domain) = info.domain.as_deref() else {
        return false;
    };
    let Some(server) = info
        .dhcp_dns
        .first()
        .map(|ip| IpAddr::V4(*ip))
        .or_else(|| dns::system_resolver().ok())
    else {
        return false;
    };
    dns::query(server, &format!("wpad.{domain}"), dns::A, dns::IN)
        .is_ok_and(|a| a.rcode == 0 && a.records.iter().any(|r| r.rtype == dns::A))
}

const NUR_MAC: &str = "Proxy-Einstellungen liest Netzinfo bisher nur auf dem Mac.";

/// Ob dieses System die Proxy-Einstellungen lesen kann (`scutil` gibt es nur auf dem Mac).
pub fn unterstuetzt() -> bool {
    cfg!(target_os = "macos")
}

/// Misst Proxy-Angaben des aktuellen Netzes. Sendet nur eine DNS-Frage nach `wpad.<domain>`.
pub fn messen() -> Result<ProxyBefund, String> {
    if !unterstuetzt() {
        return Err(NUR_MAC.into());
    }
    let out = crate::befehl("scutil", &["--proxy"], crate::BEFEHL_GRENZE).map_err(|e| {
        format!(
            "`scutil` lieferte nichts ({}). {NUR_MAC}",
            crate::io_satz(&e)
        )
    })?;
    let mut b = parse_scutil(&out);
    if let Ok(info) = hier::collect() {
        b.wpad_dhcp = crate::plattform::wpad_dhcp(&info.interface);
        b.wpad_dns = wpad_per_dns(&info);
    }
    Ok(b)
}

/// Lädt auf Wunsch die PAC-Datei des Systems (nur `http://`, 5 s, 64 KB) und gibt die Hostnamen
/// darin zurück. Die Datei wird nur gelesen, nie ausgeführt.
pub fn pac_ansehen() -> Result<Vec<String>, String> {
    let Some(url) = messen()?.pac_url else {
        return Err("Dieses Netz gibt keine PAC-Datei vor, es gibt nichts anzusehen.".into());
    };
    let nicht_http =
        || format!("Die PAC-Datei liegt nicht unter http://, ich lade sie nicht. Adresse: {url}");
    let rest = url.strip_prefix("http://").ok_or_else(nicht_http)?;
    let (auth, pfad) = rest.split_once('/').unwrap_or((rest, ""));
    let host = portal::host_of(&url).ok_or_else(nicht_http)?;
    let port: u16 = auth
        .rsplit_once(':')
        .map_or(Some(80), |(_, p)| p.parse().ok())
        .ok_or_else(nicht_http)?;
    let raw = portal::fetch((host.as_str(), port), &host, &format!("/{pfad}"))
        .map_err(|e| format!("Die PAC-Datei ließ sich nicht laden ({e}). Adresse: {url}"))?;
    match portal::parse(&raw) {
        Some((200, _, body)) => Ok(pac_hosts(&String::from_utf8_lossy(body))),
        _ => Err(format!(
            "Die PAC-Datei kam nicht als normale Antwort an. Adresse: {url}"
        )),
    }
}

/// Pro Netz gemerkt: nur Hostnamen und Ja/Nein, keine URLs mit Pfad.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Gespeichert {
    manuell: Option<String>,
    pac_host: Option<String>,
    wpad_dhcp: bool,
    wpad_dns: bool,
    zeit: u64,
}

type Store = BTreeMap<String, Gespeichert>;

impl Gespeichert {
    fn aus(b: &ProxyBefund) -> Self {
        Self {
            manuell: b
                .manuell
                .as_ref()
                .map(|m| m.split(':').next().unwrap_or(m).to_string()),
            pac_host: b.pac_url.as_deref().and_then(portal::host_of),
            wpad_dhcp: b.wpad_dhcp.is_some(),
            wpad_dns: b.wpad_dns,
            zeit: netze::now(),
        }
    }
}

/// Was in einem „meins“-Netz auffällt: ein Proxy, der vorher nicht da war oder jetzt woanders
/// hinzeigt (auffällig), und WPAD, das vorher fehlte (Alarm). Gleichbleibende Proxys sind harmlos.
fn vergleiche(alt: &Gespeichert, neu: &Gespeichert) -> Vec<(Klasse, String)> {
    let mut out = Vec::new();
    let ziel = |g: &Gespeichert| g.manuell.clone().or_else(|| g.pac_host.clone());
    if let Some(z) = ziel(neu).filter(|z| ziel(alt).as_ref() != Some(z)) {
        out.push((
            Klasse::Auffaellig,
            format!(
                "Dein Rechner soll Webseiten über den Proxy {z} laden, das war bei deinem letzten Besuch anders. Ein Proxy sieht deine Webanfragen. In einem Heimnetz ist ein neuer Eintrag ein Grund zum Nachsehen."
            ),
        ));
    }
    for (vorher, jetzt, wie) in [
        (alt.wpad_dhcp, neu.wpad_dhcp, "den DHCP-Server (Option 252)"),
        (alt.wpad_dns, neu.wpad_dns, "einen DNS-Eintrag wpad im Netz"),
    ] {
        if jetzt && !vorher {
            out.push((
                Klasse::Alarm,
                format!(
                    "Dieses Netz bietet jetzt per WPAD eine automatische Proxy-Konfiguration an, erkennbar an {wie}. Das war früher nicht so. Auf diesem Weg lassen sich Webanfragen umleiten."
                ),
            ));
        }
    }
    out
}

/// Findings for `netzinfo pruefen`; `daten` is the path of netze.json, `key` the network key.
/// Der Speicher liegt in proxy.json daneben. Nur „meins“-Netze werden verglichen; Firmen- und
/// Schulnetze setzen Proxys absichtlich.
pub fn fuer_pruefen(daten: &Path, key: &str, meins: bool) -> Vec<(Klasse, String)> {
    let Ok(befund) = messen() else {
        return Vec::new();
    };
    let neu = Gespeichert::aus(&befund);
    let pfad = daten.with_file_name("proxy.json");
    let Ok(mut store) = netze::load_json::<Store>(&pfad) else {
        return Vec::new();
    };
    let alt = store.insert(key.to_string(), neu.clone());
    if netze::save(&pfad, &store).is_err() {
        return Vec::new();
    }
    match alt {
        Some(alt) if meins => vergleiche(&alt, &neu),
        _ => Vec::new(),
    }
}

pub const OPTIONEN: &[&str] = &["--ansehen"];

pub fn run(args: &[String], explain_on: bool) -> Result<(), String> {
    crate::optionen::nur(args, OPTIONEN, "proxy")?;
    let ansehen = match args {
        [] => false,
        [a] if a == "--ansehen" => true,
        _ => {
            return Err(
                "`netzinfo proxy` kennt nur `--ansehen` (lädt die PAC-Datei und zeigt die Hostnamen darin).".into(),
            );
        }
    };
    explain(
        explain_on,
        "Ein Proxy ist ein Zwischenrechner, über den dein Rechner Webseiten laden soll. Der Betreiber könnte dabei deine Webanfragen sehen. In einem Heimnetz gibt es normalerweise keinen.",
    );
    explain(
        explain_on,
        "Ich lese die Proxy-Einstellung des Systems (`scutil --proxy`), die WPAD-Adresse aus dem DHCP (Option 252) und frage den DNS-Server deines Netzes nach `wpad.<Domain>`. Ich ändere nichts und lade nur auf Wunsch eine Datei.",
    );
    if ansehen {
        explain(
            explain_on,
            "Die PAC-Datei ist ein kleines Programm, das dem Browser sagt, welcher Proxy für welche Adresse gilt. Ich führe es nie aus, ich lese nur die Hostnamen darin als Text.",
        );
        let hosts = pac_ansehen()?;
        if hosts.is_empty() {
            println!("In der PAC-Datei stehen keine Proxy-Hostnamen.");
        }
        hosts
            .iter()
            .for_each(|h| println!("Proxy in der PAC-Datei: {h}"));
        return Ok(());
    }
    let b = messen()?;
    let zeig = |o: &Option<String>| o.clone().unwrap_or_else(|| "keiner".into());
    println!("Fester Proxy: {}", zeig(&b.manuell));
    println!("PAC-Datei: {}", zeig(&b.pac_url));
    println!("WPAD per DHCP: {}", zeig(&b.wpad_dhcp));
    println!(
        "WPAD per DNS: {}",
        if b.wpad_dns {
            "gefunden"
        } else {
            "nicht gefunden"
        }
    );
    println!(
        "Automatische Proxy-Erkennung im Mac: {}",
        if b.auto_suche { "an" } else { "aus" }
    );
    if b.keiner() {
        println!("Kein Proxy.");
    } else if b.pac_url.is_some() {
        crate::satz("Die PAC-Datei lade ich nur mit `netzinfo proxy --ansehen`.");
    }
    explain(
        explain_on,
        "Firmen- und Schulnetze setzen Proxys oft absichtlich. Auffällig ist ein Proxy erst, wenn er in deinem Heimnetz plötzlich neu auftaucht. `netzinfo pruefen` vergleicht das mit dem letzten Besuch.",
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCUTIL: &str = "<dictionary> {\n  ExceptionsList : <array> {\n    0 : *.local\n    1 : 169.254/16\n  }\n  FTPPassive : 1\n  HTTPEnable : 1\n  HTTPPort : 8080\n  HTTPProxy : proxy.example.net\n  HTTPSEnable : 0\n  HTTPSPort : 8443\n  HTTPSProxy : other.example.net\n  ProxyAutoConfigEnable : 1\n  ProxyAutoConfigURLString : http://wpad.example.net/proxy.pac\n  ProxyAutoDiscoveryEnable : 1\n}\n";
    const NUR_AUSNAHMEN: &str =
        "<dictionary> {\n  ExceptionsList : <array> {\n    0 : *.local\n  }\n  FTPPassive : 1\n}\n";

    #[test]
    fn scutil_mit_proxy_und_pac() {
        let b = parse_scutil(SCUTIL);
        assert_eq!(b.manuell.as_deref(), Some("proxy.example.net:8080"));
        assert_eq!(
            b.pac_url.as_deref(),
            Some("http://wpad.example.net/proxy.pac")
        );
        assert!(b.auto_suche && !b.keiner());
    }

    #[test]
    fn scutil_ohne_proxy_oder_leer() {
        assert!(parse_scutil(NUR_AUSNAHMEN).keiner());
        assert_eq!(parse_scutil(""), ProxyBefund::default());
        // ausgeschaltet zählt nicht, auch wenn noch ein Host dasteht
        assert_eq!(
            parse_scutil("HTTPEnable : 0\nHTTPProxy : proxy.example.net\n").manuell,
            None
        );
    }

    #[test]
    fn pac_nur_hostnamen() {
        let pac = "function FindProxyForURL(url, host) {\n if (dnsDomainIs(host, \".example.org\")) return \"DIRECT\";\n return \"PROXY Proxy1.example.net:3128; PROXY 192.0.2.7:8080; SOCKS proxy1.example.net; DIRECT\";\n}";
        assert_eq!(pac_hosts(pac), ["proxy1.example.net", "192.0.2.7"]);
        assert!(pac_hosts("").is_empty());
    }

    fn g(manuell: Option<&str>, pac: Option<&str>, dhcp: bool, dns: bool) -> Gespeichert {
        Gespeichert {
            manuell: manuell.map(Into::into),
            pac_host: pac.map(Into::into),
            wpad_dhcp: dhcp,
            wpad_dns: dns,
            zeit: 0,
        }
    }

    #[test]
    fn neu_im_meins_netz() {
        let keiner = g(None, None, false, false);
        let proxy = g(Some("proxy.example.net"), None, false, false);
        assert!(vergleiche(&keiner, &keiner).is_empty());
        // gleichbleibender Proxy: harmlos
        assert!(vergleiche(&proxy, &proxy).is_empty());
        // neuer Proxy: auffällig
        let v = vergleiche(&keiner, &proxy);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].0, Klasse::Auffaellig);
        // neues WPAD: Alarm, je Quelle ein Befund
        let wpad = g(None, None, true, true);
        let v = vergleiche(&keiner, &wpad);
        assert_eq!(v.len(), 2);
        assert!(v.iter().all(|(k, _)| *k == Klasse::Alarm));
        // WPAD war schon da: kein Alarm
        assert!(vergleiche(&wpad, &wpad).is_empty());
    }
}
