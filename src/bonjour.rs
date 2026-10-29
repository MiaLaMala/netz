//! `netzinfo bonjour`: services that devices announce via mDNS / DNS-SD (issue #28).
//! Own client on top of `dns::build_query`; no mDNS crate. Packets are untrusted input.
//! mDNS answers have no question and ID 0, so `dns::parse_response` does not fit; this file
//! walks the records itself (SRV and names with spaces need more than `dns::Record.data`).

use crate::dns::{self, u16_at};
use crate::netze;
use serde::{Deserialize, Serialize};
use socket2::{Domain, SockRef, Socket, Type};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddrV4, SocketAddrV6, UdpSocket};
use std::time::{Duration, Instant};

const SRV: u16 = 33;
const META: &str = "_services._dns-sd._udp.local";
const GROUP: Ipv4Addr = Ipv4Addr::new(224, 0, 0, 251);
const GROUP6: Ipv6Addr = Ipv6Addr::new(0xff02, 0, 0, 0, 0, 0, 0, 0xfb);
const PORT: u16 = 5353;
/// Question class IN with the „unicast response“ bit (RFC 6762, 5.4).
const IN_QU: u16 = 0x8001;
const SAMMELZEIT: u64 = 4;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Dienst {
    /// Service type, e.g. `_ipp._tcp`.
    pub typ: String,
    /// Instance name chosen by the device (can contain spaces and personal names).
    pub instanz: String,
    /// Host name from the SRV record, e.g. `drucker.local`.
    pub host: String,
    pub port: u16,
    pub ipv4: Vec<Ipv4Addr>,
    pub ipv6: Vec<Ipv6Addr>,
    /// TXT pairs. Values may hold serial numbers or names: stored, printed only with `--txt`.
    pub txt: BTreeMap<String, String>,
}

enum Rd {
    Ptr(String),
    Srv(u16, String),
    Txt(BTreeMap<String, String>),
    A(Ipv4Addr),
    Aaaa(Ipv6Addr),
}

fn kaputt() -> String {
    "Ein Bonjour-Paket ist abgeschnitten oder kaputt und wird ignoriert.".into()
}

/// Reads a possibly compressed name; labels stay UTF-8 (instance names have spaces).
fn name_at(b: &[u8], pos: usize) -> Result<(String, usize), String> {
    let (mut p, mut jumps, mut end) = (pos, 0, None);
    let mut labels: Vec<String> = Vec::new();
    loop {
        let l = *b.get(p).ok_or_else(kaputt)? as usize;
        match l & 0xC0 {
            0 if l == 0 => return Ok((labels.join("."), end.unwrap_or(p + 1))),
            0 => {
                let s = b.get(p + 1..p + 1 + l).ok_or_else(kaputt)?;
                labels.push(
                    String::from_utf8_lossy(s)
                        .chars()
                        .filter(|c| !c.is_control())
                        .collect(),
                );
                if labels.len() > 64 {
                    return Err(kaputt());
                }
                p += 1 + l;
            }
            0xC0 => {
                jumps += 1;
                if jumps > 10 {
                    return Err(kaputt());
                }
                end.get_or_insert(p + 2);
                p = (u16_at(b, p)? & 0x3FFF) as usize;
            }
            _ => return Err(kaputt()),
        }
    }
}

fn txt_pairs(rd: &[u8]) -> Result<BTreeMap<String, String>, String> {
    let (mut i, mut m) = (0, BTreeMap::new());
    while i < rd.len() {
        let l = rd[i] as usize;
        let part = rd.get(i + 1..i + 1 + l).ok_or_else(kaputt)?;
        i += 1 + l;
        let s = String::from_utf8_lossy(part);
        let (k, v) = s.split_once('=').unwrap_or((&s, ""));
        if !k.is_empty() {
            m.entry(k.to_string()).or_insert_with(|| v.to_string());
        }
    }
    Ok(m)
}

/// All records of a response (answers, authority, additional) that matter here.
/// Queries and records with TTL 0 (goodbyes) are skipped.
fn parse(b: &[u8]) -> Result<Vec<(String, Rd)>, String> {
    if b.len() < 12 || u16_at(b, 2)? & 0x8000 == 0 {
        return Err(kaputt());
    }
    let mut pos = 12;
    for _ in 0..u16_at(b, 4)? {
        pos = name_at(b, pos)?.1 + 4;
    }
    let total = u32::from(u16_at(b, 6)?) + u32::from(u16_at(b, 8)?) + u32::from(u16_at(b, 10)?);
    let mut out = Vec::new();
    for _ in 0..total {
        let (name, after) = name_at(b, pos)?;
        let rtype = u16_at(b, after)?;
        let ttl = (u32::from(u16_at(b, after + 4)?) << 16) | u32::from(u16_at(b, after + 6)?);
        let len = u16_at(b, after + 8)? as usize;
        let start = after + 10;
        let rd = b.get(start..start + len).ok_or_else(kaputt)?;
        pos = start + len;
        if ttl == 0 {
            continue;
        }
        let rd = match (rtype, len) {
            (dns::PTR, _) => Rd::Ptr(name_at(b, start)?.0),
            (SRV, 7..) => Rd::Srv(u16_at(b, start + 4)?, name_at(b, start + 6)?.0),
            (dns::TXT, _) => Rd::Txt(txt_pairs(rd)?),
            (dns::A, 4) => Rd::A(Ipv4Addr::new(rd[0], rd[1], rd[2], rd[3])),
            (dns::AAAA, 16) => Rd::Aaaa(Ipv6Addr::from(
                <[u8; 16]>::try_from(rd).map_err(|_| kaputt())?,
            )),
            _ => continue,
        };
        out.push((name, rd));
    }
    Ok(out)
}

/// Collects records from many packets and joins them to `Dienst`s.
#[derive(Default)]
struct Sammler {
    typen: BTreeSet<String>,
    /// lower-case full instance name -> (type, instance)
    inst: BTreeMap<String, (String, String)>,
    srv: HashMap<String, (u16, String)>,
    txt: HashMap<String, BTreeMap<String, String>>,
    v4: HashMap<String, Vec<Ipv4Addr>>,
    v6: HashMap<String, Vec<Ipv6Addr>>,
}

fn push_new<T: PartialEq>(v: &mut Vec<T>, x: T) {
    if !v.contains(&x) {
        v.push(x);
    }
}

impl Sammler {
    fn add(&mut self, recs: Vec<(String, Rd)>) {
        for (name, rd) in recs {
            let key = name.to_lowercase();
            match rd {
                Rd::Ptr(t) if key == META => {
                    self.typen.insert(t.trim_end_matches(".local").to_string());
                }
                Rd::Ptr(t) => {
                    // "Name._ipp._tcp.local" -> ("_ipp._tcp", "Name")
                    let short = t.strip_suffix(".local").unwrap_or(&t);
                    let mut p = short.rsplitn(3, '.');
                    if let (Some(proto), Some(typ), Some(inst)) = (p.next(), p.next(), p.next())
                        && typ.starts_with('_')
                        && proto.starts_with('_')
                    {
                        self.inst
                            .insert(t.to_lowercase(), (format!("{typ}.{proto}"), inst.into()));
                    }
                }
                Rd::Srv(port, host) => {
                    self.srv.insert(key, (port, host));
                }
                Rd::Txt(m) => {
                    self.txt.insert(key, m);
                }
                Rd::A(a) => push_new(self.v4.entry(key).or_default(), a),
                Rd::Aaaa(a) => push_new(self.v6.entry(key).or_default(), a),
            }
        }
    }

    /// What the answers left out, as (name, type): SRV/TXT of instances, A/AAAA of hosts.
    // ponytail: instance names with a dot inside are not asked for (labels would split).
    fn offen(&self) -> Vec<(String, u16)> {
        let mut v = Vec::new();
        for (full, (typ, inst)) in &self.inst {
            if !inst.contains('.') && !self.srv.contains_key(full) {
                let n = format!("{inst}.{typ}.local");
                v.extend([(n.clone(), SRV), (n, dns::TXT)]);
            }
        }
        for (_, host) in self.srv.values() {
            let h = host.to_lowercase();
            if !self.v4.contains_key(&h) {
                v.push((host.clone(), dns::A));
            }
            if !self.v6.contains_key(&h) {
                v.push((host.clone(), dns::AAAA));
            }
        }
        v
    }

    /// Instances without an SRV record are dropped: no port, nothing to show.
    fn dienste(&self) -> Vec<Dienst> {
        let mut out: Vec<Dienst> = self
            .inst
            .iter()
            .filter_map(|(full, (typ, instanz))| {
                let (port, host) = self.srv.get(full)?;
                let h = host.to_lowercase();
                Some(Dienst {
                    typ: typ.clone(),
                    instanz: instanz.clone(),
                    host: host.clone(),
                    port: *port,
                    ipv4: self.v4.get(&h).cloned().unwrap_or_default(),
                    ipv6: self.v6.get(&h).cloned().unwrap_or_default(),
                    txt: self.txt.get(full).cloned().unwrap_or_default(),
                })
            })
            .collect();
        out.sort_by(|a, b| (&a.typ, &a.instanz).cmp(&(&b.typ, &b.instanz)));
        out
    }
}

/// Listening is allowed everywhere; asking only in networks marked „meins“.
fn gate(meins: bool, lauschen: bool) -> Result<(), String> {
    if meins || lauschen {
        return Ok(());
    }
    Err("`netzinfo bonjour` fragt nur in einem Netz, das du mit `netzinfo merken <Name> --meins` als „meins“ markiert hast. Zum bloßen Mithören gibt es `netzinfo bonjour --lauschen 30`. Es wurde nichts gesendet.".into())
}

fn net_io(e: std::io::Error) -> String {
    net_err(crate::io_satz(&e))
}

fn net_err(e: impl std::fmt::Display) -> String {
    format!(
        "Der Bonjour-Socket ließ sich nicht öffnen ({e}). Prüfe die Netzverbindung und die Rechte für das lokale Netzwerk."
    )
}

/// A network interface that is up, can multicast and is not loopback / point-to-point.
#[derive(Debug, Default, Clone)]
struct Schnittstelle {
    index: u32,
    v4: Option<Ipv4Addr>,
    v6: bool,
}

/// All active multicast interfaces with their IPv4 address and whether they speak IPv6.
fn schnittstellen() -> Vec<Schnittstelle> {
    let (mut out, mut head): (Vec<Schnittstelle>, *mut libc::ifaddrs) =
        (Vec::new(), std::ptr::null_mut());
    if unsafe { libc::getifaddrs(&mut head) } != 0 {
        return out;
    }
    let (want, deny) = (
        libc::IFF_UP | libc::IFF_RUNNING | libc::IFF_MULTICAST,
        libc::IFF_LOOPBACK | libc::IFF_POINTOPOINT,
    );
    let mut p = head;
    while !p.is_null() {
        let a = unsafe { &*p };
        p = a.ifa_next;
        let f = a.ifa_flags as i32;
        if f & want != want || f & deny != 0 || a.ifa_addr.is_null() {
            continue;
        }
        let index = unsafe { libc::if_nametoindex(a.ifa_name) };
        if index == 0 {
            continue;
        }
        let pos = out
            .iter()
            .position(|s| s.index == index)
            .unwrap_or_else(|| {
                out.push(Schnittstelle {
                    index,
                    ..Default::default()
                });
                out.len() - 1
            });
        match i32::from(unsafe { (*a.ifa_addr).sa_family }) {
            libc::AF_INET => {
                let sin = unsafe { &*(a.ifa_addr as *const libc::sockaddr_in) };
                let ip = Ipv4Addr::from(u32::from_be(sin.sin_addr.s_addr));
                out[pos].v4.get_or_insert(ip);
            }
            libc::AF_INET6 => out[pos].v6 = true,
            _ => {}
        }
    }
    unsafe { libc::freeifaddrs(head) };
    out
}

/// Port 5353 is shared with the system, hence address and port reuse.
fn mdns_socket(domain: Domain) -> Result<Socket, String> {
    let s = Socket::new(domain, Type::DGRAM, None).map_err(net_io)?;
    s.set_reuse_address(true).map_err(net_io)?;
    #[cfg(unix)]
    s.set_reuse_port(true).map_err(net_io)?;
    Ok(s)
}

fn kurz(s: Socket) -> Result<UdpSocket, String> {
    let u = UdpSocket::from(s);
    u.set_read_timeout(Some(Duration::from_millis(25)))
        .map_err(net_io)?;
    Ok(u)
}

/// Joins 224.0.0.251 on every interface that has an IPv4 address.
fn hoerer4(ifs: &[Schnittstelle]) -> Result<UdpSocket, String> {
    let s = mdns_socket(Domain::IPV4)?;
    s.bind(&SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, PORT).into())
        .map_err(net_io)?;
    let mut ok = false;
    for a in ifs.iter().filter_map(|i| i.v4) {
        ok |= s.join_multicast_v4(&GROUP, &a).is_ok();
    }
    if !ok {
        return Err(net_err("keine Schnittstelle mit IPv4"));
    }
    kurz(s)
}

/// Joins ff02::fb on every interface that has an IPv6 address.
fn hoerer6(ifs: &[Schnittstelle]) -> Result<UdpSocket, String> {
    let s = mdns_socket(Domain::IPV6)?;
    s.set_only_v6(true).map_err(net_io)?;
    s.bind(&SocketAddrV6::new(Ipv6Addr::UNSPECIFIED, PORT, 0, 0).into())
        .map_err(net_io)?;
    let mut ok = false;
    for i in ifs.iter().filter(|i| i.v6) {
        ok |= s.join_multicast_v6(&GROUP6, i.index).is_ok();
    }
    if !ok {
        return Err(net_err("keine Schnittstelle mit IPv6"));
    }
    kurz(s)
}

/// Own ports, so responders answer us directly (with QU even when the system holds 5353).
struct Frager {
    v4: Option<UdpSocket>,
    v6: Option<UdpSocket>,
    ifs: Vec<Schnittstelle>,
}

impl Frager {
    fn neu(ifs: &[Schnittstelle]) -> Result<Self, String> {
        let offen = |d: Domain, a: &str| -> Result<UdpSocket, String> {
            kurz(
                Socket::new(d, Type::DGRAM, None)
                    .map_err(net_io)
                    .and_then(|s| {
                        s.bind(&a.parse::<std::net::SocketAddr>().map_err(net_err)?.into())
                            .map_err(net_io)?;
                        Ok(s)
                    })?,
            )
        };
        let f = Frager {
            v4: offen(Domain::IPV4, "0.0.0.0:0").ok(),
            v6: offen(Domain::IPV6, "[::]:0").ok(),
            ifs: ifs.to_vec(),
        };
        if f.v4.is_none() && f.v6.is_none() {
            return Err(net_err("kein UDP-Socket verfügbar"));
        }
        Ok(f)
    }

    fn sockets(&self) -> impl Iterator<Item = &UdpSocket> {
        self.v4.iter().chain(self.v6.iter())
    }

    /// Sends one question out of every interface; fails only if no interface could send.
    fn fragen(&self, name: &str, qtype: u16) -> Result<(), String> {
        let q = frage_bytes(name, qtype)?;
        let (mut ok, mut letzter) = (false, "Netzwerkfehler".to_string());
        let mut zaehlen = |r: std::io::Result<()>| match r {
            Ok(()) => ok = true,
            Err(e) => letzter = crate::io_satz(&e),
        };
        for i in &self.ifs {
            if let (Some(s), Some(a)) = (&self.v4, i.v4) {
                let sr = SockRef::from(s);
                zaehlen(
                    sr.set_multicast_if_v4(&a)
                        .and_then(|()| s.send_to(&q, (GROUP, PORT)).map(drop)),
                );
            }
            if let (Some(s), true) = (&self.v6, i.v6) {
                let sr = SockRef::from(s);
                zaehlen(sr.set_multicast_if_v6(i.index).and_then(|()| {
                    s.send_to(&q, SocketAddrV6::new(GROUP6, PORT, 0, i.index))
                        .map(drop)
                }));
            }
        }
        if ok {
            return Ok(());
        }
        Err(format!(
            "Die Bonjour-Anfrage ließ sich nicht senden ({letzter}). Prüfe, ob dein Rechner im WLAN oder LAN ist und die Rechte für das lokale Netzwerk gesetzt sind."
        ))
    }
}

/// One mDNS question (ID 0, no flags). Labels stay UTF-8: `dns::build_query` is ASCII only,
/// but instance names carry spaces and umlauts (RFC 6763, 4.1.3).
fn frage_bytes(name: &str, qtype: u16) -> Result<Vec<u8>, String> {
    let mut p = vec![0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0];
    for label in name.split('.') {
        if label.is_empty() || label.len() > 63 || name.len() > 253 {
            return Err(format!(
                "„{name}“ ist kein gültiger Bonjour-Name (Teile von 1 bis 63 Byte)."
            ));
        }
        p.push(label.len() as u8);
        p.extend_from_slice(label.as_bytes());
    }
    p.push(0);
    p.extend_from_slice(&qtype.to_be_bytes());
    p.extend_from_slice(&IN_QU.to_be_bytes());
    Ok(p)
}

/// Listens for `sekunden`; with `fragen` it also asks for all types, then for each type found,
/// then for what the answers left out (SRV/TXT of instances, A/AAAA of hosts).
pub fn sammeln(sekunden: u64, fragen: bool) -> Result<Vec<Dienst>, String> {
    let ifs = schnittstellen();
    let (mut hear, mut fehler) = (Vec::new(), String::new());
    for r in [hoerer4(&ifs), hoerer6(&ifs)] {
        match r {
            Ok(u) => hear.push(u),
            Err(e) => fehler = e,
        }
    }
    if hear.is_empty() {
        return Err(fehler);
    }
    let frager = if fragen {
        Some(Frager::neu(&ifs)?)
    } else {
        None
    };
    let (end, mut next) = (
        Instant::now() + Duration::from_secs(sekunden),
        Instant::now(),
    );
    let (mut s, mut gefragt) = (Sammler::default(), BTreeSet::new());
    let mut buf = [0u8; 9000];
    while Instant::now() < end {
        if let Some(f) = frager.as_ref().filter(|_| Instant::now() >= next) {
            f.fragen(META, dns::PTR)?;
            let typen = s.typen.iter().map(|t| (format!("{t}.local"), dns::PTR));
            for (n, t) in typen.chain(s.offen()) {
                if gefragt.insert((n.clone(), t)) {
                    f.fragen(&n, t)?;
                }
            }
            next += Duration::from_millis(1200);
        }
        for sock in hear.iter().chain(frager.iter().flat_map(Frager::sockets)) {
            if let Ok(n) = sock.recv(&mut buf)
                && let Ok(recs) = parse(&buf[..n])
            {
                s.add(recs);
            }
        }
    }
    Ok(s.dienste())
}

/// Puts services onto stored devices: by IPv4, IPv6, then by MAC via the neighbour table.
/// A device with no service this time keeps its old list (silence is not proof).
/// Returns the services that matched no device.
fn zuordnen(
    geraete: &mut [crate::geraete::Device],
    dienste: &[Dienst],
    tabelle: &[crate::nachbarn::Nachbar],
) -> Vec<Dienst> {
    let mut neu: BTreeMap<usize, Vec<Dienst>> = BTreeMap::new();
    let mut ohne = Vec::new();
    for d in dienste {
        let mac = tabelle
            .iter()
            .find(|n| d.ipv4.iter().any(|a| n.ipv4.contains(a)))
            .map(|n| &n.mac);
        let hit = geraete.iter().position(|g| {
            d.ipv4.contains(&g.ip)
                || d.ipv6.iter().any(|a| g.ipv6.contains(a))
                || (mac.is_some() && g.mac.as_ref() == mac)
        });
        match hit {
            Some(i) => neu.entry(i).or_default().push(d.clone()),
            None => ohne.push(d.clone()),
        }
    }
    for (i, list) in neu {
        let host = list.iter().map(|d| d.host.as_str()).find(|h| !h.is_empty());
        geraete[i].mdns_name = host.map(|h| h.trim_end_matches(".local").to_string());
        geraete[i].dienste = list;
    }
    ohne
}

fn hint(typ: &str) -> Option<&'static str> {
    Some(match typ {
        "_ipp._tcp" | "_printer._tcp" | "_pdl-datastream._tcp" => "Hier steht ein Drucker.",
        "_smb._tcp" => {
            "Dieses Gerät bietet Dateifreigabe (SMB) an. Wenn du es nicht brauchst, schalte sie am Gerät ab."
        }
        "_airplay._tcp" | "_raop._tcp" => {
            "AirPlay: Bild oder Ton lässt sich auf dieses Gerät schicken."
        }
        "_googlecast._tcp" => "Chromecast oder ein Gerät mit Google Cast.",
        "_hap._tcp" => "HomeKit-Zubehör.",
        "_ssh._tcp" | "_sftp-ssh._tcp" => "Fernzugang per SSH ist angeboten.",
        _ => return None,
    })
}

pub const OPTIONEN: &[&str] = &["--txt", "--lauschen"];

pub fn run(args: &[String], explain_on: bool) -> Result<(), String> {
    crate::optionen::nur(args, OPTIONEN, "bonjour")?;
    let e = |t: &str| crate::explain(explain_on, t);
    let (mut lauschen, mut txt) = (None, false);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--txt" => txt = true,
            "--lauschen" => {
                lauschen = it
                    .next()
                    .and_then(|s| s.parse::<u64>().ok())
                    .filter(|s| (1..=600).contains(s))
                    .map(Some)
                    .ok_or("Nach `--lauschen` fehlt die Zeit in Sekunden (1 bis 600), z. B. `netzinfo bonjour --lauschen 30`.")?;
            }
            s => {
                return Err(format!(
                    "Unbekannte Option „{s}“. Es gibt nur `--lauschen <Sekunden>`, `--txt` und `--erklaer`."
                ));
            }
        }
    }
    let meins = netze::current_is_meins();
    gate(meins, lauschen.is_some())?;
    e(
        "Bonjour ist ein Rundruf im Netz: Geräte melden dort, was sie anbieten. `_ipp` heißt zum Beispiel: Hier steht ein Drucker. Ich scanne keinen Port, ich höre nur zu und stelle Fragen an die Rundruf-Adressen 224.0.0.251 (IPv4) und ff02::fb (IPv6), auf jeder aktiven Schnittstelle.",
    );
    let fragen = lauschen.is_none();
    let sek = lauschen.unwrap_or(SAMMELZEIT);
    if fragen {
        e(
            "Dieses Netz ist „meins“, darum darf ich fragen: erst nach allen Diensttypen, dann je Typ nach den Geräten, danach nach allem, was die Antworten ausgelassen haben (Port, Adressen). Die Fragen tragen das Bit „bitte direkt antworten“, so erreichen mich die Antworten auch, wenn dein Mac selbst auf Port 5353 lauscht.",
        );
    } else {
        e(
            "Mit `--lauschen` sende ich nichts. Ich höre nur mit, was Geräte von sich aus oder auf fremde Fragen melden. Das geht in jedem Netz, dauert aber, bis sich jemand meldet.",
        );
    }
    println!(
        "Bonjour: {sek} Sekunden {}",
        if fragen {
            "fragen und zuhören"
        } else {
            "zuhören"
        }
    );
    let dienste = sammeln(sek, fragen)?;

    let mut ohne = Vec::new();
    if meins {
        ohne = speichern(&dienste)?;
        e(
            "Gespeichert in geraete.json bei dem Gerät, das zur IP (oder zur MAC) passt. Die TXT-Werte stehen dort im Klartext; sie können Seriennummern oder Namen enthalten.",
        );
    }
    println!("Dienste: {}", dienste.len());
    if dienste.is_empty() {
        crate::satz(
            "Es hat sich niemand gemeldet. Viele Geräte schweigen; versuche es später noch einmal.",
        );
    }
    for d in &dienste {
        let ips: Vec<String> = d
            .ipv4
            .iter()
            .map(|a| a.to_string())
            .chain(d.ipv6.iter().map(|a| a.to_string()))
            .collect();
        println!(
            "  {:<22} {}  {}:{}  {}",
            d.typ,
            d.instanz,
            d.host,
            d.port,
            ips.join(" ")
        );
        if let Some(h) = hint(&d.typ) {
            e(h);
        }
        if txt {
            for (k, v) in &d.txt {
                println!("      {k}={v}");
            }
        } else if !d.txt.is_empty() {
            let keys: Vec<&str> = d.txt.keys().map(String::as_str).collect();
            println!("      TXT: {}", keys.join(", "));
        }
    }
    if !txt && dienste.iter().any(|d| !d.txt.is_empty()) {
        crate::satz(
            "TXT-Werte sind ausgeblendet (können Namen oder Seriennummern enthalten). Mit `--txt` anzeigen.",
        );
    }
    if !ohne.is_empty() {
        crate::satz(&format!(
            "{} Dienste passen zu keinem bekannten Gerät. Starte `netzinfo geraete --nachbarn`, dann ordne ich sie zu.",
            ohne.len()
        ));
    }
    e(
        "Grenzen: Du siehst nur Geräte im selben Netzsegment. Viele Geräte schweigen (Windows-PCs, einfache IoT-Geräte), und jeder kann beliebige Dienste ankündigen: Ein Treffer beweist keine Identität.",
    );
    Ok(())
}

fn speichern(dienste: &[Dienst]) -> Result<Vec<Dienst>, String> {
    let info = crate::hier::collect()?;
    let fp = netze::fingerprint(&info)?;
    let path = netze::data_path()?.with_file_name("geraete.json");
    let mut store: BTreeMap<String, Vec<crate::geraete::Device>> = netze::load_json(&path)?;
    let tabelle = crate::nachbarn::lesen(&info.interface).unwrap_or_default();
    let ohne = zuordnen(store.entry(fp.key()).or_default(), dienste, &tabelle);
    netze::save(&path, &store)?;
    Ok(ohne)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gate_refusal_is_nicht_meins() {
        let e = gate(false, false).unwrap_err();
        assert_eq!(
            crate::netze::fehlerart(&e),
            crate::netze::Fehlerart::NichtMeins
        );
    }

    fn nm(s: &str) -> Vec<u8> {
        let mut v = Vec::new();
        for l in s.split('.') {
            v.push(l.len() as u8);
            v.extend_from_slice(l.as_bytes());
        }
        v.push(0);
        v
    }

    fn rr(name: &[u8], t: u16, ttl: u32, rd: &[u8]) -> Vec<u8> {
        let mut v = name.to_vec();
        v.extend_from_slice(&t.to_be_bytes());
        v.extend_from_slice(&0x8001u16.to_be_bytes());
        v.extend_from_slice(&ttl.to_be_bytes());
        v.extend_from_slice(&(rd.len() as u16).to_be_bytes());
        v.extend_from_slice(rd);
        v
    }

    /// Response: PTR in the answers, SRV/TXT/A/AAAA in additional. Instance and host names
    /// after their first appearance are compression pointers.
    fn sample() -> Vec<u8> {
        let inst = nm("Wohn Drucker._ipp._tcp.local");
        let ptr_name = nm("_ipp._tcp.local");
        let c = |off: usize| (0xC000 | off as u16).to_be_bytes().to_vec();
        let mut p = vec![0, 0, 0x84, 0, 0, 0, 0, 1, 0, 0, 0, 4];
        p.extend(rr(&ptr_name, dns::PTR, 120, &inst));
        let inst_off = 12 + ptr_name.len() + 10;
        let srv_at = 12 + ptr_name.len() + 10 + inst.len();
        let mut srv = vec![0, 0, 0, 0, 0x02, 0x77];
        srv.extend(nm("drucker.local"));
        p.extend(rr(&c(inst_off), SRV, 120, &srv));
        p.extend(rr(
            &c(inst_off),
            dns::TXT,
            120,
            b"\x07fn=Mias\x06rp=ipp\x03flg",
        ));
        let host_off = srv_at + 2 + 10 + 6;
        p.extend(rr(&c(host_off), dns::A, 120, &[192, 0, 2, 7]));
        let v6 = "2001:db8::7".parse::<Ipv6Addr>().unwrap().octets();
        p.extend(rr(&c(host_off), dns::AAAA, 120, &v6));
        p
    }

    #[test]
    fn parses_and_joins_service() {
        let mut s = Sammler::default();
        s.add(parse(&sample()).unwrap());
        let d = s.dienste();
        assert_eq!(d.len(), 1);
        assert_eq!(
            (d[0].typ.as_str(), d[0].instanz.as_str()),
            ("_ipp._tcp", "Wohn Drucker")
        );
        assert_eq!((d[0].host.as_str(), d[0].port), ("drucker.local", 631));
        assert_eq!(d[0].ipv4, vec![Ipv4Addr::new(192, 0, 2, 7)]);
        assert_eq!(d[0].ipv6, vec!["2001:db8::7".parse::<Ipv6Addr>().unwrap()]);
        assert_eq!(d[0].txt["fn"], "Mias");
        assert_eq!(d[0].txt["rp"], "ipp");
        assert_eq!(d[0].txt["flg"], "");
    }

    #[test]
    fn truncated_packets_are_errors_not_panics() {
        let p = sample();
        for n in 0..p.len() {
            assert!(parse(&p[..n]).is_err(), "cut at {n}");
        }
    }

    #[test]
    fn rejects_queries_and_pointer_loops() {
        let mut q = sample();
        q[2] = 0; // QR bit cleared: a query, not a response
        assert!(parse(&q).is_err());
        let mut l = vec![0, 0, 0x84, 0, 0, 0, 0, 1, 0, 0, 0, 0];
        l.extend([0xC0, 12]); // name points at itself
        l.extend([0; 10]);
        assert!(parse(&l).is_err());
    }

    #[test]
    fn meta_answer_lists_types_and_goodbye_is_skipped() {
        let mut p = vec![0, 0, 0x84, 0, 0, 0, 0, 2, 0, 0, 0, 0];
        p.extend(rr(&nm(META), dns::PTR, 4500, &nm("_smb._tcp.local")));
        p.extend(rr(&nm(META), dns::PTR, 0, &nm("_old._tcp.local")));
        let mut s = Sammler::default();
        s.add(parse(&p).unwrap());
        assert_eq!(s.typen.into_iter().collect::<Vec<_>>(), vec!["_smb._tcp"]);
    }

    /// Recorded response: PTR "Küche Drucker._ipp._tcp.local", SRV (port 631, host
    /// drucker.local), AAAA 2001:db8::7 and fe80::1 (hex of the wire bytes).
    const IPV6_ANTWORT: &str = "\
        000084000000000100000003045f697070045f746370056c6f63616c00000c80\
        010000007800200e4bc3bc63686520447275636b6572045f697070045f746370\
        056c6f63616c00c0270021800100000078001500000000027707647275636b65\
        72056c6f63616c00c059001c800100000078001020010db80000000000000000\
        00000007c059001c8001000000780010fe800000000000000000000000000001";

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }

    #[test]
    fn recorded_ipv6_answer_with_umlaut_instance() {
        let mut s = Sammler::default();
        s.add(parse(&hex(IPV6_ANTWORT)).unwrap());
        let d = s.dienste();
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].instanz, "Küche Drucker");
        assert_eq!((d[0].host.as_str(), d[0].port), ("drucker.local", 631));
        assert!(d[0].ipv4.is_empty());
        let v6: Vec<Ipv6Addr> = ["2001:db8::7", "fe80::1"]
            .iter()
            .map(|a| a.parse().unwrap())
            .collect();
        assert_eq!(d[0].ipv6, v6);
    }

    #[test]
    fn asks_for_what_the_answer_left_out() {
        let mut s = Sammler::default();
        let mut p = vec![0, 0, 0x84, 0, 0, 0, 0, 1, 0, 0, 0, 0];
        p.extend(rr(
            &nm("_ipp._tcp.local"),
            dns::PTR,
            120,
            &nm("Küche Drucker._ipp._tcp.local"),
        ));
        s.add(parse(&p).unwrap());
        let o = s.offen();
        let n = "Küche Drucker._ipp._tcp.local".to_string();
        assert_eq!(o, vec![(n.clone(), SRV), (n, dns::TXT)]);
        s.add(parse(&hex(IPV6_ANTWORT)).unwrap());
        assert_eq!(s.offen(), vec![("drucker.local".to_string(), dns::A)]);
    }

    #[test]
    fn query_keeps_umlauts_as_utf8() {
        let q = frage_bytes("Küche Drucker._ipp._tcp.local", SRV).unwrap();
        assert_eq!(&q[..12], [0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0]);
        assert_eq!(&q[12..27], b"\x0eK\xc3\xbcche Drucker");
        assert_eq!(q[q.len() - 4..], [0, 33, 0x80, 1]);
        assert!(frage_bytes("a..local", SRV).is_err());
        assert!(frage_bytes(&"x".repeat(64), SRV).is_err());
    }

    #[test]
    fn gate_blocks_asking_outside_meins() {
        assert!(gate(false, false).is_err());
        assert!(gate(false, true).is_ok());
        assert!(gate(true, false).is_ok());
    }

    #[test]
    fn assigns_by_ip_then_mac_and_keeps_old_list() {
        use crate::geraete::Device;
        let dev = |ip: &str, mac: Option<&str>| Device {
            ip: ip.parse().unwrap(),
            name: None,
            ports: vec![],
            first_seen: 1,
            last_seen: 1,
            mac: mac.map(Into::into),
            ip_verlauf: vec![],
            ipv6: vec![],
            hersteller: None,
            mac_zufaellig: false,
            dienste: vec![],
            mdns_name: None,
            ..Default::default()
        };
        let svc = |ip: &str| Dienst {
            typ: "_ipp._tcp".into(),
            host: "drucker.local".into(),
            ipv4: vec![ip.parse().unwrap()],
            ..Default::default()
        };
        let mut g = vec![
            dev("192.0.2.7", None),
            dev("192.0.2.9", Some("00:00:5e:00:53:09")),
        ];
        g[0].dienste = vec![svc("192.0.2.7")];
        let tabelle = [crate::nachbarn::Nachbar {
            mac: "00:00:5e:00:53:09".into(),
            ipv4: vec!["192.0.2.55".parse().unwrap()],
            ipv6: vec![],
            schnittstelle: "en0".into(),
        }];
        let ohne = zuordnen(&mut g, &[svc("192.0.2.55"), svc("192.0.2.200")], &tabelle);
        assert_eq!(ohne.len(), 1);
        assert_eq!(g[1].dienste.len(), 1);
        assert_eq!(g[1].mdns_name.as_deref(), Some("drucker"));
        assert_eq!(g[0].dienste.len(), 1); // untouched
    }
}
