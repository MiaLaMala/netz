//! `netzinfo offen`: which programs of this Mac accept connections, and from where (issues #38, #74).

use crate::netze;
use crate::pruefen::Klasse;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::net::{IpAddr, Ipv4Addr};
use std::path::Path;
#[cfg(target_os = "macos")]
use std::process::Command;

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub enum Reichweite {
    /// Only this Mac (127.0.0.1, ::1).
    Lokal,
    /// Wildcard or a network address: others on the network can connect.
    Netz,
    /// Bound to an address of a tunnel (utun, Tailscale range): reachable through the VPN only.
    Vpn,
    /// UDP socket on a random high port without a known service: a client, not a service.
    UdpClient,
    Unbekannt,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Lauscher {
    /// "TCP" or "UDP".
    pub protokoll: String,
    /// "*" for all addresses.
    pub adresse: String,
    pub port: u16,
    pub pid: u32,
    pub programm: String,
    pub nutzer: String,
    /// Well-known service of the port, empty if unknown.
    pub dienst: String,
    pub reichweite: Reichweite,
    /// Signed by Apple? Only checked for `Netz`; `None` = not checked or unknown.
    pub apple_signiert: Option<bool>,
    /// `ports` found the same TCP port open on this Mac's own network address.
    pub von_aussen: bool,
}

pub const OPTIONEN: &[&str] = &["--json", "--aussen", "--meins"];

pub fn run(args: &[String], explain_on: bool) -> Result<(), String> {
    crate::optionen::nur(args, OPTIONEN, "offen")?;
    let (mut json, mut aussen, mut meins) = (false, false, false);
    for a in args {
        match a.as_str() {
            "--json" => json = true,
            "--aussen" => aussen = true,
            "--meins" => meins = true,
            _ => {
                return Err(format!(
                    "Unbekannte Option „{a}“. `netzinfo offen` kennt nur `--json`, `--aussen` und `--meins`."
                ));
            }
        }
    }
    let e = |t: &str| crate::explain(explain_on, t);
    if !json {
        e(
            "Ich frage das Betriebssystem direkt, welche Programme dieses Rechners auf einem Port auf Verbindungen warten (libproc, bei einem Fehler ersatzweise `lsof`). Das ist nur Lesen, nichts geht ins Netz und es braucht kein Root.",
        );
        e(
            "Ohne Root sehe ich nur deine eigenen Programme. Systemdienste und Programme anderer Nutzer fehlen in der Liste, auch wenn sie lauschen.",
        );
    }
    let mut liste = offen_im_netz()?;
    if aussen {
        e(
            "Mit `--aussen` probiere ich die TCP-Ports der Netz-Zeilen an der Netzadresse dieses Macs aus (wie `netzinfo ports`, nur mit Verbindungsaufbau, nur in einem „meins“-Netz oder mit `--meins`). Ist ein Port dort offen, kann ihn auch ein anderes Gerät erreichen.",
        );
        aussen_abgleichen(&mut liste, meins)?;
    }
    if json {
        let s = serde_json::to_string_pretty(&liste).map_err(|e| format!("JSON-Fehler: {e}"))?;
        println!("{s}");
        return Ok(());
    }
    e(
        "„NETZ“ heißt: Das Programm lauscht auf allen Adressen (`*`) oder auf einer Netzadresse. Jeder im selben WLAN kann den Port ansprechen.",
    );
    e(
        "„lokal“ heißt: Das Programm lauscht auf 127.0.0.1 oder ::1. Nur dieser Mac selbst kommt hin, von außen niemand.",
    );
    e(
        "„VPN“ heißt: Die Adresse gehört zu einem Tunnel (utun, Tailscale). Erreichbar sind nur die Teilnehmer des VPN, nicht das WLAN.",
    );
    e(
        "„UDP-Client“ heißt: Ein UDP-Port weit oben (ab 49152) ohne bekannten Dienst. Das ist fast immer ein Programm, das als Client einen zufälligen Port für Antworten gebunden hat (z. B. DNS), kein Dienst.",
    );
    e("UDP kennt keine Verbindung, ein gebundener Port nimmt einfach Pakete an.");
    e(
        "„nicht von Apple signiert“ heißt: `codesign` kann das Programm nicht auf Apples Zertifikat zurückführen. Das ist bei selbst gebauten Programmen normal, bei einem Dienst im Netz aber einen Blick wert.",
    );
    if liste.is_empty() {
        println!("Keine lauschenden Programme von dir gefunden.");
    }
    for l in &liste {
        let wo = match l.reichweite {
            Reichweite::Netz => "NETZ ",
            Reichweite::Lokal => "lokal",
            Reichweite::Vpn => "VPN  ",
            Reichweite::UdpClient => "Client",
            Reichweite::Unbekannt => "unklar",
        };
        let mut zusatz = String::new();
        if !l.dienst.is_empty() {
            zusatz += &format!("  {}", l.dienst);
        }
        if l.apple_signiert == Some(false) {
            zusatz += "  nicht von Apple signiert";
        }
        if l.von_aussen {
            zusatz += "  auch von außen erreichbar";
        }
        println!(
            "  {wo:<6}  {} {:>5}  {:<15}  {} (PID {}){zusatz}",
            l.protokoll, l.port, l.adresse, l.programm, l.pid
        );
    }
    let zaehle = |r| liste.iter().filter(|l| l.reichweite == r).count();
    let clients = zaehle(Reichweite::UdpClient);
    let ohne = if clients > 0 {
        format!(
            " ({clients} UDP-Client{} nicht mitgezählt)",
            if clients == 1 { "" } else { "s" }
        )
    } else {
        String::new()
    };
    println!(
        "{} lauschend{ohne}, davon {} für das Netz erreichbar, {} nur über VPN. Programme anderer Nutzer sind ohne Root nicht sichtbar.",
        lauschend(&liste),
        zaehle(Reichweite::Netz),
        zaehle(Reichweite::Vpn)
    );
    Ok(())
}

/// Entries that really wait for connections: a UDP client only holds a port for answers.
pub fn lauschend(liste: &[Lauscher]) -> usize {
    liste
        .iter()
        .filter(|l| l.reichweite != Reichweite::UdpClient)
        .count()
}

/// Listening TCP and bound UDP sockets of the current user, network-reachable first.
pub fn offen() -> Result<Vec<Lauscher>, String> {
    einstufen_alle(sockets()?, &Default::default())
}

/// Like `offen`, but UDP ports that this network's store saw in several runs count as services.
pub fn offen_im_netz() -> Result<Vec<Lauscher>, String> {
    let fest = aktuelles_netz()
        .and_then(|(pfad, key)| netze::load_json::<Speicher>(&pfad).ok().map(|s| (s, key)))
        .map(|(s, key)| s.fest(&key))
        .unwrap_or_default();
    einstufen_alle(sockets()?, &fest)
}

fn einstufen_alle(socks: Vec<Socket>, fest: &Fest) -> Result<Vec<Lauscher>, String> {
    let mut v = lauscher(socks, &tunnel_adressen(), fest);
    bewerten(&mut v);
    v.sort_by_key(|l| (l.reichweite != Reichweite::Netz, l.port, l.pid));
    Ok(v)
}

/// Store file and key of the current network, if it is a saved one.
fn aktuelles_netz() -> Option<(std::path::PathBuf, String)> {
    let fp = netze::fingerprint(&crate::hier::collect().ok()?).ok()?;
    let path = netze::data_path().ok()?;
    netze::find(&netze::load(&path).ok()?, &fp)?;
    Some((path.with_file_name("offen.json"), fp.key()))
}

/// Marks TCP listeners in the list whose port `ports` finds open on this Mac's own address.
pub fn aussen_abgleichen(liste: &mut [Lauscher], meins: bool) -> Result<(), String> {
    let ports: Vec<u16> = liste
        .iter()
        .filter(|l| l.reichweite == Reichweite::Netz && l.protokoll == "TCP")
        .map(|l| l.port)
        .collect();
    if ports.is_empty() {
        return Ok(());
    }
    let ip = crate::hier::collect()?
        .ip
        .ok_or("Dieser Mac hat keine Netzadresse. Verbinde dich mit einem Netz und versuche es noch einmal.")?;
    let r = crate::ports::scannen(&ip.to_string(), meins, Some(ports), 500)?;
    let offen: Vec<u16> = r
        .ports
        .iter()
        .filter(|p| p.zustand == crate::ports::Zustand::Offen)
        .map(|p| p.port)
        .collect();
    abgleichen(liste, &offen);
    Ok(())
}

fn abgleichen(liste: &mut [Lauscher], offen: &[u16]) {
    for l in liste {
        l.von_aussen =
            l.reichweite == Reichweite::Netz && l.protokoll == "TCP" && offen.contains(&l.port);
    }
}

/// One TCP or UDP socket of the current user as `lsof` lists it. Shared by `offen` (#38) and
/// `verbindungen` (#39).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Socket {
    /// "TCP" or "UDP".
    pub protokoll: String,
    /// Local address ("*" for all) and port.
    pub lokal: (String, u16),
    /// Remote address and port of a connected socket.
    pub ziel: Option<(String, u16)>,
    /// TCP state like "LISTEN" or "ESTABLISHED"; empty if lsof gave none (UDP).
    pub zustand: String,
    pub pid: u32,
    pub programm: String,
    pub nutzer: String,
}

/// All TCP and UDP sockets of the current user (all states): libproc, `lsof` as a fallback.
#[cfg(target_os = "macos")]
pub(crate) fn sockets() -> Result<Vec<Socket>, String> {
    sockets_libproc().or_else(|_| sockets_lsof())
}

#[cfg(target_os = "macos")]
fn sockets_lsof() -> Result<Vec<Socket>, String> {
    let out = Command::new("/usr/sbin/lsof")
        .args(["-nP", "-iTCP", "-iUDP", "-F", "pcLfPtnT"])
        .output()
        .map_err(|_| {
            "Die Socket-Liste ließ sich nicht lesen, auch `lsof` startet nicht. Es liegt unter macOS in /usr/sbin/lsof, prüfe, ob es fehlt."
                .to_string()
        })?;
    // lsof exits 1 when it finds nothing, so only the text counts.
    Ok(parse_sockets(&String::from_utf8_lossy(&out.stdout)))
}

/// `socket_fdinfo` as `proc_pidfdinfo(PROC_PIDFDSOCKETINFO)` fills it, and the offsets we read
/// (checked against sys/proc_info.h with offsetof).
const FDINFO_LEN: usize = 792;

/// One `socket_fdinfo` to (protocol, local, remote, TCP state); `None` for non-IP sockets.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
#[allow(clippy::type_complexity)]
fn parse_fdinfo(b: &[u8]) -> Option<(&'static str, (String, u16), Option<(String, u16)>, String)> {
    if b.len() < FDINFO_LEN {
        return None;
    }
    let i32_at = |o: usize| i32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]);
    let port = |o: usize| u16::from_be_bytes([b[o], b[o + 1]]);
    let (kind, proto) = (i32_at(256), i32_at(180));
    let (name, state) = match (kind, proto) {
        (2, _) => (
            "TCP",
            match i32_at(344) {
                1 => "LISTEN",
                4 => "ESTABLISHED",
                _ => "OTHER",
            },
        ),
        (1, 17) => ("UDP", ""),
        _ => return None,
    };
    let v6 = b[288] & 2 != 0 && b[288] & 1 == 0;
    let addr = |o: usize| -> Option<String> {
        let ip: IpAddr = if v6 {
            let mut a = [0u8; 16];
            a.copy_from_slice(&b[o..o + 16]);
            std::net::Ipv6Addr::from(a).into()
        } else {
            Ipv4Addr::new(b[o + 12], b[o + 13], b[o + 14], b[o + 15]).into()
        };
        (!ip.is_unspecified()).then(|| ip.to_string())
    };
    let lokal = (addr(312).unwrap_or_else(|| "*".into()), port(268));
    let ziel = addr(296).map(|a| (a, port(264))).filter(|z| z.1 != 0);
    Some((name, lokal, ziel, state.to_string()))
}

#[cfg(target_os = "macos")]
fn sockets_libproc() -> Result<Vec<Socket>, String> {
    use libc::{c_int, c_void};
    const ALL_PIDS: u32 = 1; // PROC_ALL_PIDS
    const FDSOCKETINFO: c_int = 3; // PROC_PIDFDSOCKETINFO
    let fehler = || "Die Prozessliste ließ sich nicht lesen.".to_string();
    // SAFETY: plain libproc calls with buffers sized as the calls ask for.
    unsafe {
        let n = libc::proc_listpids(ALL_PIDS, 0, std::ptr::null_mut(), 0);
        if n <= 0 {
            return Err(fehler());
        }
        let mut pids = vec![0 as c_int; n as usize / 4 + 32];
        let n = libc::proc_listpids(
            ALL_PIDS,
            0,
            pids.as_mut_ptr() as *mut c_void,
            (pids.len() * 4) as c_int,
        );
        if n <= 0 {
            return Err(fehler());
        }
        pids.truncate(n as usize / 4);
        let mut out = Vec::new();
        let mut nutzer = std::collections::HashMap::<u32, String>::new();
        for pid in pids.into_iter().filter(|p| *p > 0) {
            // Other users' processes fail here without root, like with lsof.
            let bytes = libc::proc_pidinfo(pid, libc::PROC_PIDLISTFDS, 0, std::ptr::null_mut(), 0);
            if bytes <= 0 {
                continue;
            }
            let mut fds: Vec<libc::proc_fdinfo> =
                Vec::with_capacity(bytes as usize / std::mem::size_of::<libc::proc_fdinfo>() + 8);
            let got = libc::proc_pidinfo(
                pid,
                libc::PROC_PIDLISTFDS,
                0,
                fds.as_mut_ptr() as *mut c_void,
                (fds.capacity() * std::mem::size_of::<libc::proc_fdinfo>()) as c_int,
            );
            if got <= 0 {
                continue;
            }
            fds.set_len(got as usize / std::mem::size_of::<libc::proc_fdinfo>());
            let mut name = [0u8; 256];
            let len = libc::proc_name(pid, name.as_mut_ptr() as *mut c_void, 256).max(0) as usize;
            let programm = String::from_utf8_lossy(&name[..len]).into_owned();
            let mut bsd: libc::proc_bsdinfo = std::mem::zeroed();
            let uid = if libc::proc_pidinfo(
                pid,
                libc::PROC_PIDTBSDINFO,
                0,
                &mut bsd as *mut _ as *mut c_void,
                std::mem::size_of::<libc::proc_bsdinfo>() as c_int,
            ) > 0
            {
                bsd.pbi_uid
            } else {
                u32::MAX
            };
            let user = nutzer.entry(uid).or_insert_with(|| uid_name(uid)).clone();
            for fd in fds
                .iter()
                .filter(|f| f.proc_fdtype == libc::PROX_FDTYPE_SOCKET as u32)
            {
                let mut buf = [0u8; FDINFO_LEN];
                let r = libc::proc_pidfdinfo(
                    pid,
                    fd.proc_fd,
                    FDSOCKETINFO,
                    buf.as_mut_ptr() as *mut c_void,
                    FDINFO_LEN as c_int,
                );
                if r as usize != FDINFO_LEN {
                    continue;
                }
                let Some((proto, lokal, ziel, zustand)) = parse_fdinfo(&buf) else {
                    continue;
                };
                let s = Socket {
                    protokoll: proto.to_string(),
                    lokal,
                    ziel,
                    zustand,
                    pid: pid as u32,
                    programm: programm.clone(),
                    nutzer: user.clone(),
                };
                if !out.contains(&s) {
                    out.push(s);
                }
            }
        }
        Ok(out)
    }
}

#[cfg(target_os = "macos")]
fn uid_name(uid: u32) -> String {
    let mut buf = [0 as libc::c_char; 512];
    // SAFETY: getpwuid_r writes into our buffers; pw_name is read only when it returned a record.
    unsafe {
        let mut pw: libc::passwd = std::mem::zeroed();
        let mut res: *mut libc::passwd = std::ptr::null_mut();
        if libc::getpwuid_r(uid, &mut pw, buf.as_mut_ptr(), buf.len(), &mut res) == 0
            && !res.is_null()
        {
            return std::ffi::CStr::from_ptr(pw.pw_name)
                .to_string_lossy()
                .into_owned();
        }
    }
    uid.to_string()
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn sockets() -> Result<Vec<Socket>, String> {
    Err("Die Liste der Sockets gibt es bisher nur unter macOS (über libproc).".into())
}

/// UDP ports (program, port) that count as services: seen in several runs of one network.
type Fest = std::collections::HashSet<(String, u16)>;

/// Listening or bound sockets, i.e. everything that is not a connection.
fn lauscher(socks: Vec<Socket>, tunnel: &[IpAddr], fest: &Fest) -> Vec<Lauscher> {
    let mut out: Vec<Lauscher> = Vec::new();
    for s in socks {
        if s.ziel.is_some() || !(s.zustand.is_empty() || s.zustand == "LISTEN") {
            continue;
        }
        let mut l = Lauscher {
            protokoll: s.protokoll,
            reichweite: reichweite_mit(&s.lokal.0, tunnel),
            adresse: s.lokal.0,
            port: s.lokal.1,
            pid: s.pid,
            programm: s.programm,
            nutzer: s.nutzer,
            dienst: crate::ports::service_name(s.lokal.1),
            apple_signiert: None,
            von_aussen: false,
        };
        // Random high UDP port without a known service: a client waiting for answers, not a service.
        if l.reichweite == Reichweite::Netz
            && l.protokoll == "UDP"
            && l.port >= 49152
            && l.dienst.is_empty()
            && !fest.contains(&(l.programm.clone(), l.port))
        {
            l.reichweite = Reichweite::UdpClient;
        }
        if !out.contains(&l) {
            out.push(l);
        }
    }
    out
}

#[cfg(test)]
fn parse_lsof(text: &str) -> Vec<Lauscher> {
    lauscher(parse_sockets(text), &[], &Default::default())
}

/// Parses `lsof -F pcLfPtnT` output (one field per line, first char = field id).
/// Unknown or missing fields and unusable records are skipped.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))] // lsof parser, tested; Linux will need it too
pub(crate) fn parse_sockets(text: &str) -> Vec<Socket> {
    let (mut pid, mut prog, mut user) = (None, String::new(), String::new());
    let (mut proto, mut name, mut state) = (String::new(), String::new(), String::new());
    let mut out: Vec<Socket> = Vec::new();
    let flush = |out: &mut Vec<Socket>,
                 pid: Option<u32>,
                 prog: &str,
                 user: &str,
                 proto: &str,
                 name: &str,
                 state: &str| {
        let (local, remote) = match name.split_once("->") {
            Some((l, r)) => (l, split_addr(r)),
            None => (name, None),
        };
        let (Some(pid), Some(lokal)) = (pid, split_addr(local)) else {
            return;
        };
        if name.contains("->") && remote.is_none() {
            return;
        }
        let s = Socket {
            protokoll: proto.to_string(),
            lokal,
            ziel: remote,
            zustand: state.to_string(),
            pid,
            programm: prog.to_string(),
            nutzer: user.to_string(),
        };
        if (proto == "TCP" || proto == "UDP") && !out.contains(&s) {
            out.push(s);
        }
    };
    for line in text.lines() {
        let mut c = line.chars();
        let Some(id) = c.next() else { continue };
        let val = c.as_str();
        match id {
            'p' => {
                flush(&mut out, pid, &prog, &user, &proto, &name, &state);
                (pid, prog, user) = (val.parse().ok(), String::new(), String::new());
                (proto, name, state) = (String::new(), String::new(), String::new());
            }
            'f' => {
                flush(&mut out, pid, &prog, &user, &proto, &name, &state);
                (proto, name, state) = (String::new(), String::new(), String::new());
            }
            'c' => prog = val.to_string(),
            'L' => user = val.to_string(),
            'P' => proto = val.to_string(),
            'n' => name = val.to_string(),
            'T' => {
                if let Some(st) = val.strip_prefix("ST=") {
                    state = st.to_string();
                }
            }
            _ => {}
        }
    }
    flush(&mut out, pid, &prog, &user, &proto, &name, &state);
    out
}

/// "*:80", "127.0.0.1:631", "[::1]:8080" to (host, port); None for "*:*".
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn split_addr(s: &str) -> Option<(String, u16)> {
    let (host, port) = s.rsplit_once(':')?;
    let host = host.trim_start_matches('[').trim_end_matches(']');
    Some((host.to_string(), port.parse().ok()?))
}

#[cfg(test)]
fn reichweite(host: &str) -> Reichweite {
    reichweite_mit(host, &[])
}

/// `tunnel`: addresses of this Mac's tunnel interfaces. Tailscale's ranges count without it.
fn reichweite_mit(host: &str, tunnel: &[IpAddr]) -> Reichweite {
    if host == "*" {
        return Reichweite::Netz;
    }
    let Ok(ip) = host.parse::<IpAddr>() else {
        return Reichweite::Unbekannt;
    };
    let tailscale = match ip {
        IpAddr::V4(a) => a.octets()[0] == 100 && a.octets()[1] & 0xc0 == 64, // 100.64.0.0/10
        IpAddr::V6(a) => a.segments()[..3] == [0xfd7a, 0x115c, 0xa1e0],
    };
    if ip.is_loopback() {
        Reichweite::Lokal
    } else if tailscale || tunnel.contains(&ip) {
        Reichweite::Vpn
    } else {
        Reichweite::Netz
    }
}

/// Addresses of tunnel interfaces (utun on macOS, tun, wg, tailscale elsewhere).
fn tunnel_adressen() -> Vec<IpAddr> {
    let mut out = Vec::new();
    let mut first: *mut libc::ifaddrs = std::ptr::null_mut();
    // SAFETY: getifaddrs hands out a linked list that we only read and free again.
    unsafe {
        if libc::getifaddrs(&mut first) != 0 {
            return out;
        }
        let mut p = first;
        while !p.is_null() {
            let i = &*p;
            p = i.ifa_next;
            let name = std::ffi::CStr::from_ptr(i.ifa_name).to_string_lossy();
            if i.ifa_addr.is_null()
                || !["utun", "tun", "wg", "tailscale"]
                    .iter()
                    .any(|n| name.starts_with(n))
            {
                continue;
            }
            match (*i.ifa_addr).sa_family as i32 {
                libc::AF_INET => {
                    let a = &*(i.ifa_addr as *const libc::sockaddr_in);
                    out.push(IpAddr::from(a.sin_addr.s_addr.to_ne_bytes()));
                }
                libc::AF_INET6 => {
                    let a = &*(i.ifa_addr as *const libc::sockaddr_in6);
                    out.push(IpAddr::from(a.sin6_addr.s6_addr));
                }
                _ => {}
            }
        }
        libc::freeifaddrs(first);
    }
    out
}

/// Fills `apple_signiert` for network-reachable listeners (`codesign`, macOS only).
fn bewerten(liste: &mut [Lauscher]) {
    #[cfg(target_os = "macos")]
    {
        let mut gesehen = std::collections::HashMap::<u32, Option<bool>>::new();
        for l in liste
            .iter_mut()
            .filter(|l| l.reichweite == Reichweite::Netz)
        {
            l.apple_signiert = *gesehen
                .entry(l.pid)
                .or_insert_with(|| apple_signiert(l.pid));
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = liste;
}

/// `Some(true)`: the program traces back to Apple's certificate. `Some(false)`: unsigned or signed
/// by someone else. `None`: path or `codesign` gave no answer (shown as unknown, not as a finding).
#[cfg(target_os = "macos")]
fn apple_signiert(pid: u32) -> Option<bool> {
    let mut buf = [0u8; 4096];
    // SAFETY: proc_pidpath writes at most buf.len() bytes and returns the count.
    let n = unsafe { libc::proc_pidpath(pid as libc::c_int, buf.as_mut_ptr().cast(), 4096) };
    if n <= 0 {
        return None;
    }
    let pfad = String::from_utf8_lossy(&buf[..n as usize]).into_owned();
    let o = Command::new("/usr/bin/codesign")
        .args(["--verify", "-R=anchor apple", &pfad])
        .output()
        .ok()?;
    if o.status.success() {
        return Some(true);
    }
    let t = String::from_utf8_lossy(&o.stderr);
    (t.contains("not signed at all") || t.contains("failed to satisfy")).then_some(false)
}

/// What `offen.json` keeps per network: the network-reachable listeners of the last run.
#[derive(Debug, Default, Serialize, Deserialize)]
struct Speicher(BTreeMap<String, Vec<Gesehen>>);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Gesehen {
    protokoll: String,
    programm: String,
    port: u16,
    /// In how many runs in a row this listener was there.
    laeufe: u32,
    zuletzt: u64,
}

impl Speicher {
    /// UDP listeners that were there in at least three runs: services, not client noise.
    fn fest(&self, key: &str) -> Fest {
        self.0
            .get(key)
            .into_iter()
            .flatten()
            .filter(|g| g.protokoll == "UDP" && g.laeufe >= 3)
            .map(|g| (g.programm.clone(), g.port))
            .collect()
    }
}

/// Compares `jetzt` with the stored listeners of network `key`, stores `jetzt`. The first run in a
/// network is the baseline and reports nothing. Client noise is stored (to count runs) but never reported.
fn vergleichen(
    sp: &mut Speicher,
    key: &str,
    jetzt: &[Lauscher],
    now: u64,
) -> Vec<(Klasse, String)> {
    let erster = !sp.0.contains_key(key);
    let alt = sp.0.remove(key).unwrap_or_default();
    let (mut neu, mut out) = (Vec::<Gesehen>::new(), Vec::new());
    for l in jetzt
        .iter()
        .filter(|l| matches!(l.reichweite, Reichweite::Netz | Reichweite::UdpClient))
    {
        let gleich = |g: &Gesehen| {
            (g.protokoll.as_str(), g.programm.as_str(), g.port)
                == (l.protokoll.as_str(), l.programm.as_str(), l.port)
        };
        if neu.iter().any(gleich) {
            continue;
        }
        let vorher = alt.iter().find(|g| gleich(g));
        if vorher.is_none() && !erster && l.reichweite == Reichweite::Netz {
            let dienst = if l.dienst.is_empty() {
                String::new()
            } else {
                format!(", {}", l.dienst)
            };
            let signiert = if l.apple_signiert == Some(false) {
                ", nicht von Apple signiert"
            } else {
                ""
            };
            out.push((
                Klasse::Auffaellig,
                format!(
                    "neu: Programm {} lauscht jetzt im Netz ({} {}{dienst}{signiert})",
                    l.programm, l.protokoll, l.port
                ),
            ));
        }
        neu.push(Gesehen {
            protokoll: l.protokoll.clone(),
            programm: l.programm.clone(),
            port: l.port,
            laeufe: vorher.map_or(1, |g| g.laeufe.saturating_add(1)),
            zuletzt: now,
        });
    }
    sp.0.insert(key.to_string(), neu);
    out
}

/// Findings for `netzinfo pruefen`: network-reachable listeners that were not there last time.
pub fn fuer_pruefen(
    daten: &Path,
    key: &str,
    _meins: bool,
) -> Vec<(crate::pruefen::Klasse, String)> {
    let pfad = daten.with_file_name("offen.json");
    let Ok(mut sp) = netze::load_json::<Speicher>(&pfad) else {
        return Vec::new();
    };
    let Ok(jetzt) = sockets().and_then(|s| einstufen_alle(s, &sp.fest(key))) else {
        return Vec::new();
    };
    let out = vergleichen(&mut sp, key, &jetzt, netze::now());
    let _ = netze::save(&pfad, &sp);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "p100\ncserver\nLuser\nf5\nPTCP\ntIPv4\nn*:8080\nf6\nPTCP\ntIPv6\nn[::1]:8080\nf7\nPUDP\ntIPv4\nn*:5353\nf8\nPUDP\ntIPv4\nn192.0.2.5:123->192.0.2.9:123\np200\ncExampleApp\nLuser\nf3\nPTCP\ntIPv4\nn127.0.0.1:631\n";

    #[test]
    fn parses_normal_ipv6_and_udp() {
        let v = parse_lsof(SAMPLE);
        // the connected UDP socket is dropped
        assert_eq!(v.len(), 4);
        assert_eq!(v[0].reichweite, Reichweite::Netz);
        assert_eq!(
            (v[0].port, v[0].pid, v[0].programm.as_str()),
            (8080, 100, "server")
        );
        assert_eq!(v[0].dienst, "HTTP (alternativ)");
        assert_eq!(
            (v[1].adresse.as_str(), v[1].reichweite),
            ("::1", Reichweite::Lokal)
        );
        assert_eq!((v[2].protokoll.as_str(), v[2].port), ("UDP", 5353));
        assert_eq!((v[3].pid, v[3].programm.as_str()), (200, "ExampleApp"));
        assert_eq!(v[3].reichweite, Reichweite::Lokal);
    }

    #[test]
    fn tolerates_truncated_and_garbage() {
        assert!(parse_lsof("").is_empty());
        assert!(parse_lsof("p1\ncx\nf3\nPTCP\nn127.0.0.1:").is_empty());
        assert!(parse_lsof("p\nn*:80\n").is_empty());
        assert!(parse_lsof("\u{0}\u{ff}garbage\n\n::\nn\nnnn:99999\nPTCP\nn*:*").is_empty());
        // last record cut off after the name still counts
        assert_eq!(parse_lsof("p1\ncx\nf3\nPTCP\nn*:80").len(), 1);
        // no protocol: dropped
        assert!(parse_lsof("p1\ncx\nf3\nn*:80").is_empty());
    }

    #[test]
    fn scope() {
        assert_eq!(reichweite("*"), Reichweite::Netz);
        assert_eq!(reichweite("127.0.0.1"), Reichweite::Lokal);
        assert_eq!(reichweite("::1"), Reichweite::Lokal);
        assert_eq!(reichweite("192.0.2.5"), Reichweite::Netz);
        assert_eq!(reichweite("host"), Reichweite::Unbekannt);
    }

    #[test]
    #[cfg(target_os = "macos")] // lsof lives in /usr/sbin only there
    fn own_test_port_shows_up_as_local() {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let v = offen().unwrap();
        let me = v
            .iter()
            .find(|x| x.port == port && x.pid == std::process::id())
            .expect("lsof should list our own listening test port");
        assert_eq!(
            (me.protokoll.as_str(), me.reichweite),
            ("TCP", Reichweite::Lokal)
        );
    }

    fn sock(proto: &str, adr: &str, port: u16, prog: &str) -> Socket {
        Socket {
            protokoll: proto.into(),
            lokal: (adr.into(), port),
            ziel: None,
            zustand: if proto == "TCP" {
                "LISTEN".into()
            } else {
                String::new()
            },
            pid: 1,
            programm: prog.into(),
            nutzer: "user".into(),
        }
    }

    #[test]
    fn vpn_and_udp_client_are_told_apart() {
        let tun: IpAddr = "198.51.100.7".parse().unwrap();
        assert_eq!(reichweite_mit("198.51.100.7", &[tun]), Reichweite::Vpn);
        assert_eq!(reichweite("100.100.1.2"), Reichweite::Vpn);
        assert_eq!(reichweite("100.128.0.1"), Reichweite::Netz); // outside 100.64.0.0/10
        assert_eq!(reichweite("fd7a:115c:a1e0::1"), Reichweite::Vpn);
        assert_eq!(reichweite("2001:db8::1"), Reichweite::Netz);
        let socks = vec![
            sock("UDP", "*", 53211, "dnsclient"),
            sock("UDP", "*", 5353, "mdns"),
            sock("UDP", "*", 60000, "dienst"),
            sock("UDP", "127.0.0.1", 53212, "lokal"),
            sock("TCP", "*", 50000, "server"),
        ];
        let fest: Fest = [("dienst".to_string(), 60000)].into();
        let alle = lauscher(socks, &[], &fest);
        let r: Vec<_> = alle.iter().map(|l| l.reichweite).collect();
        use Reichweite::*;
        assert_eq!(r, vec![UdpClient, Netz, Netz, Lokal, Netz]);
        assert_eq!(lauschend(&alle), 4); // the UDP client does not listen
    }

    #[test]
    fn fdinfo_parses_tcp_listen_and_udp_v6() {
        let mut b = vec![0u8; FDINFO_LEN];
        b[256] = 2; // TCP
        b[344] = 1; // LISTEN
        b[288] = 1; // IPv4
        b[268..270].copy_from_slice(&8080u16.to_be_bytes());
        b[312 + 12..312 + 16].copy_from_slice(&[192, 0, 2, 5]);
        let (p, lokal, ziel, st) = parse_fdinfo(&b).unwrap();
        assert_eq!(
            (p, lokal, ziel, st.as_str()),
            ("TCP", ("192.0.2.5".into(), 8080), None, "LISTEN")
        );

        let mut u = vec![0u8; FDINFO_LEN];
        u[256] = 1; // IN
        u[180] = 17; // UDP
        u[288] = 2; // IPv6, unspecified address = wildcard
        u[268..270].copy_from_slice(&5353u16.to_be_bytes());
        u[264..266].copy_from_slice(&53u16.to_be_bytes());
        u[296..312].copy_from_slice(
            &"2001:db8::9"
                .parse::<std::net::Ipv6Addr>()
                .unwrap()
                .octets(),
        );
        let (p, lokal, ziel, _) = parse_fdinfo(&u).unwrap();
        assert_eq!((p, lokal), ("UDP", ("*".into(), 5353)));
        assert_eq!(ziel, Some(("2001:db8::9".into(), 53)));

        assert!(parse_fdinfo(&b[..100]).is_none());
        b[256] = 3; // not an IP socket
        assert!(parse_fdinfo(&b).is_none());
    }

    fn netz_l(prog: &str, port: u16, signiert: Option<bool>) -> Lauscher {
        Lauscher {
            protokoll: "TCP".into(),
            adresse: "*".into(),
            port,
            pid: 1,
            programm: prog.into(),
            nutzer: "user".into(),
            dienst: String::new(),
            reichweite: Reichweite::Netz,
            apple_signiert: signiert,
            von_aussen: false,
        }
    }

    #[test]
    fn store_reports_only_new_listeners_after_the_baseline() {
        let mut sp = Speicher::default();
        let alt = [netz_l("alt", 8080, Some(true))];
        assert!(vergleichen(&mut sp, "k", &alt, 1).is_empty()); // baseline
        let jetzt = [alt[0].clone(), netz_l("neu", 9000, Some(false))];
        let f = vergleichen(&mut sp, "k", &jetzt, 2);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].0, Klasse::Auffaellig);
        assert!(
            f[0].1.contains("Programm neu lauscht jetzt im Netz")
                && f[0].1.contains("nicht von Apple")
        );
        // the second run knows both; another network starts its own baseline
        assert!(vergleichen(&mut sp, "k", &jetzt, 3).is_empty());
        assert!(vergleichen(&mut sp, "k2", &jetzt, 3).is_empty());
        assert_eq!(sp.0["k"][0].laeufe, 3);
    }

    #[test]
    fn udp_noise_is_stored_not_reported_and_becomes_a_service_after_three_runs() {
        let mut sp = Speicher::default();
        let mut u = netz_l("dns", 53211, None);
        u.protokoll = "UDP".into();
        u.reichweite = Reichweite::UdpClient;
        vergleichen(&mut sp, "k", &[], 1);
        for run in 2..5 {
            assert!(vergleichen(&mut sp, "k", &[u.clone()], run).is_empty());
        }
        assert!(sp.fest("k").contains(&("dns".to_string(), 53211)));
        assert!(sp.fest("anderes").is_empty());
    }

    #[test]
    fn outside_match_marks_only_tcp_in_the_network() {
        let mut l = vec![netz_l("a", 80, None), netz_l("b", 81, None)];
        l[1].reichweite = Reichweite::Lokal;
        abgleichen(&mut l, &[80, 81]);
        assert_eq!((l[0].von_aussen, l[1].von_aussen), (true, false));
    }
}
