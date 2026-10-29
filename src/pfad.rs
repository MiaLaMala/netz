//! `netzinfo pfad`: hop by hop path to a target, without root, see issue #30.
//! One ICMP datagram socket (no root on macOS) sends echo requests with rising TTL; routers
//! answer „Time Exceeded“. Received packets are untrusted: every read is bounds-checked.
//! IPv4 (ICMP) and IPv6 (ICMPv6, IPV6_UNICAST_HOPS, issue #75). ponytail: Linux/Windows are not built.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, UdpSocket};
use std::time::{Duration, Instant};

use serde::Serialize;
use socket2::{Domain, Protocol, Socket, Type};

use crate::{dns, explain, netze, statistik};

const WAIT: Duration = Duration::from_secs(1);
const GAP: Duration = Duration::from_millis(50);

/// All numbers of one hop over all rounds. Times in milliseconds.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Hop {
    pub ttl: u8,
    /// Every address that answered at this TTL (load balancing shows several). Empty = only stars.
    pub adressen: Vec<IpAddr>,
    /// Reverse DNS of the first address, if asked for and found.
    pub name: Option<String>,
    pub gesendet: u32,
    pub beantwortet: u32,
    pub verlust_prozent: f64,
    pub min_ms: Option<f64>,
    pub median_ms: Option<f64>,
    pub p95_ms: Option<f64>,
    /// Mean absolute difference of adjacent round trip times.
    pub jitter_ms: Option<f64>,
    /// First address is private (10/8, 172.16/12, 192.168/16, 100.64/10): own network or provider.
    pub lokal: bool,
    pub erster_oeffentlicher: bool,
    /// This hop is the target itself (it sent the echo reply).
    pub ziel: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PfadErgebnis {
    pub ziel: String,
    pub ziel_ip: IpAddr,
    pub hops: Vec<Hop>,
    pub erreicht: bool,
    /// Unix seconds.
    pub zeitpunkt: u64,
}

// ---- packets -------------------------------------------------------------------------------

fn checksum(data: &[u8]) -> u16 {
    let mut sum: u32 = data
        .chunks(2)
        .map(|c| u32::from(u16::from_be_bytes([c[0], *c.get(1).unwrap_or(&0)])))
        .sum();
    while sum > 0xffff {
        sum = (sum & 0xffff) + (sum >> 16);
    }
    !(sum as u16)
}

/// Echo request (type 8) with 16 payload bytes.
fn echo_request(id: u16, seq: u16) -> Vec<u8> {
    let mut p = vec![8, 0, 0, 0];
    p.extend_from_slice(&id.to_be_bytes());
    p.extend_from_slice(&seq.to_be_bytes());
    p.extend_from_slice(b"netzinfo-pfad\0\0\0");
    let c = checksum(&p);
    p[2..4].copy_from_slice(&c.to_be_bytes());
    p
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Reply {
    pub(crate) seq: u16,
    /// Echo reply (the target answered) instead of „Time Exceeded“ (a router answered).
    pub(crate) echo: bool,
}

/// Skips an IPv4 header if the buffer starts with one (macOS delivers it); `None` if malformed.
fn strip_ip(b: &[u8]) -> Option<&[u8]> {
    if b.first()? >> 4 != 4 {
        return Some(b);
    }
    let ihl = usize::from(b[0] & 0x0f) * 4;
    if ihl < 20 { None } else { b.get(ihl..) }
}

fn be16(b: &[u8], i: usize) -> Option<u16> {
    Some(u16::from_be_bytes([*b.get(i)?, *b.get(i + 1)?]))
}

/// Parses type 0 (echo reply) and type 11 (time exceeded, quoting our request).
/// Anything else, truncated or garbage gives `None`. The ICMP identifier is not used: macOS
/// may rewrite it, the sequence number carries round and TTL.
fn parse_reply(buf: &[u8]) -> Option<Reply> {
    let icmp = strip_ip(buf)?;
    match (*icmp.first()?, *icmp.get(1)?) {
        (0, 0) => Some(Reply {
            seq: be16(icmp, 6)?,
            echo: true,
        }),
        (11, _) => {
            // 8 bytes ICMP header, then the quoted IP header and the first 8 bytes of our request
            let quoted = icmp.get(8..)?;
            if quoted.first()? >> 4 != 4 {
                return None;
            }
            let inner = strip_ip(quoted)?;
            (*inner.first()? == 8).then_some(Reply {
                seq: be16(inner, 6)?,
                echo: false,
            })
        }
        _ => None,
    }
}

/// ICMPv6 echo request (type 128) with 16 payload bytes. The kernel fills in the checksum.
fn echo_request6(id: u16, seq: u16) -> Vec<u8> {
    let mut p = vec![128, 0, 0, 0];
    p.extend_from_slice(&id.to_be_bytes());
    p.extend_from_slice(&seq.to_be_bytes());
    p.extend_from_slice(b"netzinfo-pfad\0\0\0");
    p
}

/// Parses ICMPv6 type 129 (echo reply) and type 3 code 0 (time exceeded, quoting our request).
/// IPv6 sockets deliver no IP header. Everything else (neighbor and router messages arrive on
/// this socket too), truncated or garbage gives `None`; the caller filters by sequence number.
fn parse_reply6(buf: &[u8]) -> Option<Reply> {
    match (*buf.first()?, *buf.get(1)?) {
        (129, 0) => Some(Reply {
            seq: be16(buf, 6)?,
            echo: true,
        }),
        (3, 0) => {
            // 8 bytes ICMPv6 header, then the quoted 40 byte IPv6 header (next header 58 = ICMPv6)
            let quoted = buf.get(8..)?;
            if quoted.first()? >> 4 != 6 || *quoted.get(6)? != 58 {
                return None;
            }
            let inner = quoted.get(40..)?;
            (*inner.first()? == 128).then_some(Reply {
                seq: be16(inner, 6)?,
                echo: false,
            })
        }
        _ => None,
    }
}

// ---- measuring -----------------------------------------------------------------------------

/// Two methods so tests can fake the network.
/// `A` is the address family (IPv4 by default, so `dyn Prober` stays the IPv4 prober).
pub(crate) trait Prober<A = Ipv4Addr> {
    fn send(&mut self, ttl: u8, seq: u16) -> Result<(), String>;
    /// Blocks up to `wait`; `None` = nothing useful arrived in that time.
    fn recv(&mut self, wait: Duration) -> Option<(A, Reply)>;
}

/// Address types `measure` works with: `Ipv4Addr` and `Ipv6Addr`.
pub(crate) trait Addr: Copy + PartialEq + Into<IpAddr> {}
impl<T: Copy + PartialEq + Into<IpAddr>> Addr for T {}

pub(crate) struct Icmp {
    sock: UdpSocket,
    to: SocketAddr,
    id: u16,
}

impl Icmp {
    pub(crate) fn open(target: Ipv4Addr) -> Result<Self, String> {
        let s = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::ICMPV4)).map_err(|e| {
            format!("Kann keinen ICMP-Socket öffnen ({e}). Auf dem Mac geht das ohne sudo; prüfe, ob eine Firewall oder ein Profil das blockt.")
        })?;
        Ok(Icmp {
            // the std type gives a safe receive buffer; the fd stays the ICMP socket
            sock: UdpSocket::from(s),
            to: SocketAddr::new(target.into(), 0),
            id: std::process::id() as u16,
        })
    }
}

impl Prober for Icmp {
    fn send(&mut self, ttl: u8, seq: u16) -> Result<(), String> {
        let fail = |e: std::io::Error| format!("Senden schlug fehl ({e}).");
        self.sock.set_ttl(u32::from(ttl)).map_err(fail)?;
        self.sock
            .send_to(&echo_request(self.id, seq), self.to)
            .map(|_| ())
            .map_err(fail)
    }

    fn recv(&mut self, wait: Duration) -> Option<(Ipv4Addr, Reply)> {
        let deadline = Instant::now() + wait;
        let mut buf = [0u8; 1500];
        loop {
            let left = deadline.checked_duration_since(Instant::now())?;
            self.sock
                .set_read_timeout(Some(left.max(Duration::from_millis(1))))
                .ok()?;
            let (n, from) = self.sock.recv_from(&mut buf).ok()?;
            if let (IpAddr::V4(ip), Some(r)) = (from.ip(), parse_reply(&buf[..n])) {
                return Some((ip, r));
            }
        }
    }
}

/// ICMPv6 twin of `Icmp`: same datagram socket idea, hop limit instead of TTL.
pub(crate) struct Icmp6 {
    sock: Socket,
    io: UdpSocket,
    to: SocketAddr,
    id: u16,
}

impl Icmp6 {
    pub(crate) fn open(target: Ipv6Addr) -> Result<Self, String> {
        let fail = |e: std::io::Error| {
            format!(
                "Kann keinen ICMPv6-Socket öffnen ({e}). Auf dem Mac geht das ohne sudo; prüfe, ob eine Firewall oder ein Profil das blockt."
            )
        };
        let sock = Socket::new(Domain::IPV6, Type::DGRAM, Some(Protocol::ICMPV6)).map_err(fail)?;
        // a clone shares the fd and its options; the std type gives a safe receive buffer
        let io = UdpSocket::from(sock.try_clone().map_err(fail)?);
        Ok(Icmp6 {
            sock,
            io,
            to: SocketAddr::new(target.into(), 0),
            id: std::process::id() as u16,
        })
    }
}

impl Prober<Ipv6Addr> for Icmp6 {
    fn send(&mut self, ttl: u8, seq: u16) -> Result<(), String> {
        let fail = |e: std::io::Error| format!("Senden schlug fehl ({e}).");
        self.sock
            .set_unicast_hops_v6(u32::from(ttl))
            .map_err(fail)?;
        self.io
            .send_to(&echo_request6(self.id, seq), self.to)
            .map(|_| ())
            .map_err(fail)
    }

    fn recv(&mut self, wait: Duration) -> Option<(Ipv6Addr, Reply)> {
        let deadline = Instant::now() + wait;
        let mut buf = [0u8; 1500];
        loop {
            let left = deadline.checked_duration_since(Instant::now())?;
            self.io
                .set_read_timeout(Some(left.max(Duration::from_millis(1))))
                .ok()?;
            let (n, from) = self.io.recv_from(&mut buf).ok()?;
            if let (IpAddr::V6(ip), Some(r)) = (from.ip(), parse_reply6(&buf[..n])) {
                return Some((ip, r));
            }
        }
    }
}

/// Per TTL: who answered, round trip times in order of arrival, probes sent.
#[derive(Debug)]
struct Raw<A = Ipv4Addr> {
    addrs: Vec<A>,
    rtts: Vec<f64>,
    sent: u32,
}

// not derived: a derive would demand `A: Default`, which addresses do not have
impl<A> Default for Raw<A> {
    fn default() -> Self {
        Raw {
            addrs: Vec::new(),
            rtts: Vec::new(),
            sent: 0,
        }
    }
}

/// Probes still waiting for an answer, plus what has been learned so far.
struct State<A> {
    raw: Vec<Raw<A>>,
    open: Vec<(u16, Instant)>,
    reached: Option<u8>,
    got_any: bool,
}

impl<A: Addr> State<A> {
    /// Waits up to `wait` for one reply and books it. False = nothing arrived.
    fn take(&mut self, p: &mut dyn Prober<A>, target: A, wait: Duration) -> bool {
        let Some((from, r)) = p.recv(wait) else {
            return false;
        };
        let ttl = (r.seq & 0xff) as u8;
        let Some(i) = self.open.iter().position(|(s, _)| *s == r.seq) else {
            return true; // stale or foreign sequence number
        };
        if r.echo && from != target {
            return true; // someone else claims to be the target
        }
        let (_, t0) = self.open.swap_remove(i);
        let h = &mut self.raw[usize::from(ttl) - 1];
        if !h.addrs.contains(&from) {
            h.addrs.push(from);
        }
        h.rtts.push(t0.elapsed().as_secs_f64() * 1000.0);
        self.got_any = true;
        if r.echo {
            self.reached = Some(self.reached.map_or(ttl, |t| t.min(ttl)));
        }
        true
    }
}

/// Runs `rounds` rounds. Round n sends TTL 1..=max; the sequence number is `n << 8 | ttl`.
/// Stops sending beyond the first TTL whose echo reply came from `target`.
/// Returns the raw hops up to that TTL and whether the target answered.
fn measure<A: Addr>(
    p: &mut dyn Prober<A>,
    target: A,
    max_hops: u8,
    rounds: u8,
) -> Result<(Vec<Raw<A>>, bool), String> {
    let mut st = State {
        raw: (0..max_hops).map(|_| Raw::default()).collect(),
        open: Vec::new(),
        reached: None,
        got_any: false,
    };
    let mut first_err: Option<String> = None;
    for round in 0..u16::from(rounds) {
        st.open.clear();
        for ttl in 1..=st.reached.unwrap_or(max_hops) {
            let seq = round << 8 | u16::from(ttl);
            match p.send(ttl, seq) {
                Ok(()) => {
                    st.open.push((seq, Instant::now()));
                    st.raw[usize::from(ttl) - 1].sent += 1;
                }
                Err(e) => {
                    first_err.get_or_insert(e);
                }
            }
            let end = Instant::now() + GAP;
            while let Some(left) = end.checked_duration_since(Instant::now()) {
                if !st.take(p, target, left) {
                    break;
                }
            }
        }
        let end = Instant::now() + WAIT;
        while !st.open.is_empty() {
            let Some(left) = end.checked_duration_since(Instant::now()) else {
                break;
            };
            if !st.take(p, target, left) {
                break;
            }
        }
    }
    if !st.got_any
        && let Some(e) = first_err
    {
        return Err(e);
    }
    st.raw.truncate(usize::from(st.reached.unwrap_or(max_hops)));
    Ok((st.raw, st.reached.is_some()))
}

// ---- numbers -------------------------------------------------------------------------------

fn is_private(ip: Ipv4Addr) -> bool {
    let o = ip.octets();
    ip.is_private() || (o[0] == 100 && (o[1] & 0xc0) == 0x40)
}

/// Own network or provider: private IPv4 (see `is_private`), IPv6 unique local fc00::/7 or
/// link-local fe80::/10.
fn is_lokal(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(a) => is_private(a),
        IpAddr::V6(a) => {
            (a.segments()[0] & 0xfe00) == 0xfc00 || (a.segments()[0] & 0xffc0) == 0xfe80
        }
    }
}

/// Kept for callers outside this file; the numbers come from `statistik`. Non-empty input.
pub(crate) fn median(v: &[f64]) -> f64 {
    statistik::median(v).unwrap_or(0.0)
}

pub(crate) fn p95(v: &[f64]) -> f64 {
    statistik::perzentil(v, 95).unwrap_or(0.0)
}

fn build_hops<A: Addr>(raw: Vec<Raw<A>>, target: A) -> Vec<Hop> {
    let mut seen_public = false;
    raw.into_iter()
        .enumerate()
        .map(|(i, r)| {
            let mut s = r.rtts.clone();
            s.sort_by(f64::total_cmp);
            let has = !s.is_empty();
            let lokal = r.addrs.first().is_some_and(|a| is_lokal((*a).into()));
            let first_public = !seen_public && !r.addrs.is_empty() && !lokal;
            seen_public |= first_public;
            Hop {
                ttl: (i + 1) as u8,
                ziel: has && r.addrs.contains(&target),
                adressen: r.addrs.iter().map(|a| (*a).into()).collect(),
                name: None,
                gesendet: r.sent,
                beantwortet: r.rtts.len() as u32,
                verlust_prozent: if r.sent == 0 {
                    0.0
                } else {
                    100.0 * (1.0 - r.rtts.len() as f64 / f64::from(r.sent))
                },
                min_ms: has.then(|| s[0]),
                median_ms: has.then(|| median(&s)),
                p95_ms: has.then(|| p95(&s)),
                jitter_ms: (r.rtts.len() > 1).then(|| {
                    r.rtts.windows(2).map(|w| (w[1] - w[0]).abs()).sum::<f64>()
                        / (r.rtts.len() - 1) as f64
                }),
                lokal,
                erster_oeffentlicher: first_public,
            }
        })
        .collect()
}

// ---- names and target ----------------------------------------------------------------------

fn arpa(ip: IpAddr) -> String {
    match ip {
        IpAddr::V4(ip) => {
            let o = ip.octets();
            format!("{}.{}.{}.{}.in-addr.arpa", o[3], o[2], o[1], o[0])
        }
        IpAddr::V6(ip) => {
            let nibbles: Vec<String> = ip
                .octets()
                .iter()
                .rev()
                .flat_map(|b| [b & 0xf, b >> 4])
                .map(|n| format!("{n:x}"))
                .collect();
            format!("{}.ip6.arpa", nibbles.join("."))
        }
    }
}

fn reverse_name(resolver: IpAddr, ip: IpAddr) -> Option<String> {
    let q = arpa(ip);
    let a = dns::query(resolver, &q, dns::PTR, dns::IN).ok()?;
    let r = a.records.iter().find(|r| r.rtype == dns::PTR)?;
    Some(r.data.trim_end_matches('.').to_string()).filter(|n| !n.is_empty())
}

/// An address as typed, else the name's A record, else its AAAA record.
fn resolve(ziel: &str) -> Result<IpAddr, String> {
    if let Ok(ip) = ziel.parse::<IpAddr>() {
        return Ok(ip);
    }
    let resolver = dns::system_resolver()?;
    [dns::A, dns::AAAA]
        .into_iter()
        .find_map(|t| {
            let a = dns::query(resolver, ziel, t, dns::IN).ok()?;
            a.records
                .iter()
                .filter(|r| r.rtype == t)
                .find_map(|r| r.data.parse().ok())
        })
        .ok_or_else(|| format!("Für „{ziel}“ gibt es keine IP-Adresse. Prüfe den Namen."))
}

// ---- public API ----------------------------------------------------------------------------

/// Measures the path to `ziel` (IPv4 or IPv6 address, or a name): `max_hops` 1..=30, `runden` 1..=100.
/// With `mit_namen` each hop's first address gets a reverse lookup at the system resolver.
pub fn pfad_daten(
    ziel: &str,
    max_hops: u8,
    runden: u8,
    mit_namen: bool,
) -> Result<PfadErgebnis, String> {
    if !(1..=30).contains(&max_hops) || !(1..=100).contains(&runden) {
        return Err("`max_hops` muss von 1 bis 30 gehen, `runden` von 1 bis 100.".into());
    }
    let target = resolve(ziel)?;
    let (mut hops, erreicht) = match target {
        IpAddr::V4(t) => {
            let (raw, ok) = measure(&mut Icmp::open(t)?, t, max_hops, runden)?;
            (build_hops(raw, t), ok)
        }
        IpAddr::V6(t) => {
            let (raw, ok) = measure(&mut Icmp6::open(t)?, t, max_hops, runden)?;
            (build_hops(raw, t), ok)
        }
    };
    if mit_namen && let Ok(resolver) = dns::system_resolver() {
        // lookups in parallel: the DNS client waits up to 2 s per try
        std::thread::scope(|s| {
            let firsts: Vec<_> = hops.iter().map(|h| h.adressen.first().copied()).collect();
            let jobs: Vec<_> = firsts
                .into_iter()
                .map(|a| a.map(|ip| s.spawn(move || reverse_name(resolver, ip))))
                .collect();
            for (h, j) in hops.iter_mut().zip(jobs) {
                h.name = j.and_then(|j| j.join().ok().flatten());
            }
        });
    }
    Ok(PfadErgebnis {
        ziel: ziel.to_string(),
        ziel_ip: target,
        hops,
        erreicht,
        zeitpunkt: netze::now(),
    })
}

fn ms(v: Option<f64>) -> String {
    v.map_or("-".into(), |x| format!("{x:.1}"))
}

pub fn run(args: &[String], explain_on: bool) -> Result<(), String> {
    let mut ziel = None;
    let (mut max_hops, mut runden, mut namen) = (20u8, 10u8, true);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut number = |flag: &str, max: u8| {
            it.next()
                .and_then(|v| v.parse::<u8>().ok())
                .filter(|n| (1..=max).contains(n))
                .ok_or(format!(
                    "Nach `{flag}` gehört eine ganze Zahl von 1 bis {max}."
                ))
        };
        match a.as_str() {
            "--max-hops" => max_hops = number("--max-hops", 30)?,
            "--runden" => runden = number("--runden", 100)?,
            "--ohne-namen" => namen = false,
            f if f.starts_with("--") => return Err(format!("Unbekannte Option „{f}“.")),
            _ if ziel.is_none() => ziel = Some(a.as_str()),
            _ => return Err("Es passt nur ein Ziel: `netzinfo pfad <Ziel>`.".into()),
        }
    }
    let ziel = ziel.unwrap_or("1.1.1.1");
    println!(
        "Messe den Weg zu {ziel}: {runden} Runden, bis zu {max_hops} Hops. Das dauert bis zu {} s.",
        u64::from(runden) * (u64::from(max_hops) / 20 + 2)
    );
    explain(
        explain_on,
        "Ich schicke kleine Ping-Pakete (ICMP Echo, bei IPv6 ICMPv6) mit steigender Lebenszeit, dem TTL (bei IPv6 Hop Limit). Jeder Router zieht 1 davon ab; bei 0 wirft er das Paket weg und meldet sich mit „Time Exceeded“. So melden sich die Geräte auf dem Weg nacheinander. Das braucht kein sudo.",
    );
    let r = pfad_daten(ziel, max_hops, runden, namen)?;
    println!("Ziel {}\n", r.ziel_ip);
    let breit = r
        .hops
        .iter()
        .flat_map(|h| &h.adressen)
        .map(|a| a.to_string().len())
        .fold(16, usize::max);
    println!(
        "{:>3}  {:<breit$} {:>8} {:>7} {:>7} {:>7} {:>7}  Art",
        "Nr", "Adresse", "Verlust", "Min", "Median", "P95", "Jitter"
    );
    for h in &r.hops {
        let a = h.adressen.first().map_or("*".into(), |a| a.to_string());
        let art = match (h.adressen.is_empty(), h.ziel, h.lokal) {
            (true, ..) => "keine Antwort",
            (_, true, _) => "Ziel",
            (_, _, true) => "lokal oder Anbieter-intern",
            _ => "öffentlich",
        };
        println!(
            "{:>3}  {:<breit$} {:>6.0} % {:>7} {:>7} {:>7} {:>7}  {art}{}{}",
            h.ttl,
            a,
            h.verlust_prozent,
            ms(h.min_ms),
            ms(h.median_ms),
            ms(h.p95_ms),
            ms(h.jitter_ms),
            if h.erster_oeffentlicher {
                " (erster öffentlicher Hop)"
            } else {
                ""
            },
            h.name.as_ref().map_or(String::new(), |n| format!("  {n}")),
        );
        for extra in h.adressen.iter().skip(1) {
            println!("     + {extra}");
        }
    }
    if !r.erreicht {
        println!(
            "\nDas Ziel hat nicht geantwortet. Entweder blockt es Ping, oder der Weg endet früher."
        );
    } else if let Some(last) = r.hops.last().filter(|h| h.verlust_prozent > 0.0) {
        println!(
            "\nAm Ziel gehen {:.0} % der Pakete verloren. Das zählt.",
            last.verlust_prozent
        );
    } else if r.hops.iter().any(|h| h.verlust_prozent > 0.0) {
        println!(
            "\nVerlust gibt es nur auf Zwischenhops, am Ziel kommt alles an. Das ist meist harmlos."
        );
    }
    explain(
        explain_on,
        "Jede Zeile ist ein Gerät auf dem Weg ins Internet. Die erste ist dein Router, die späteren gehören deinem Anbieter.",
    );
    explain(
        explain_on,
        "Verlust nur auf einer Zeile in der Mitte ist meist harmlos, solange die Zeilen danach antworten: Viele Router beantworten Ping nur langsam oder gar nicht, leiten aber trotzdem alles weiter. Nur Verlust, der bis zum Ziel bleibt, zählt. Ein Stern heißt: von diesem Hop kam nichts zurück.",
    );
    explain(
        explain_on,
        "Min ist die schnellste Laufzeit, Median die mittlere, P95 die, unter der 95 von 100 Paketen lagen. Jitter ist, wie stark die Laufzeiten von Paket zu Paket schwanken. Hin- und Rückweg können verschiedene Wege nehmen, die Zeit pro Hop ist deshalb nur eine Näherung.",
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(s: &str) -> Ipv4Addr {
        s.parse().unwrap()
    }

    #[test]
    fn checksum_known_values() {
        // RFC 1071 example
        assert_eq!(
            checksum(&[0x00, 0x01, 0xf2, 0x03, 0xf4, 0xf5, 0xf6, 0xf7]),
            !0xddf2
        );
        assert_eq!(checksum(&[0xff]), 0x00ff); // odd length is padded with zero
    }

    #[test]
    fn echo_request_layout_and_valid_checksum() {
        let p = echo_request(0x1234, 0x0305);
        assert_eq!(p.len(), 24);
        assert_eq!(&p[..2], &[8, 0]);
        assert_eq!(&p[4..8], &[0x12, 0x34, 0x03, 0x05]);
        assert_eq!(checksum(&p), 0); // a correct packet sums to zero
    }

    /// 20 byte IPv4 header (protocol 1) in front of `rest`.
    fn with_ip(rest: &[u8]) -> Vec<u8> {
        let mut v = vec![
            0x45, 0, 0, 0, 0, 0, 0, 0, 64, 1, 0, 0, 192, 0, 2, 1, 192, 0, 2, 2,
        ];
        v.extend_from_slice(rest);
        v
    }

    fn time_exceeded(seq: u16) -> Vec<u8> {
        let mut icmp = vec![11, 0, 0, 0, 0, 0, 0, 0];
        icmp.extend_from_slice(&with_ip(&echo_request(1, seq)[..8]));
        icmp
    }

    #[test]
    fn parses_echo_reply_with_and_without_ip_header() {
        let reply = [0, 0, 0, 0, 0x12, 0x34, 0x00, 0x07, 1, 2, 3];
        let want = Some(Reply { seq: 7, echo: true });
        assert_eq!(parse_reply(&reply), want);
        assert_eq!(parse_reply(&with_ip(&reply)), want);
    }

    #[test]
    fn parses_time_exceeded_with_and_without_ip_header() {
        let want = Some(Reply {
            seq: 0x0203,
            echo: false,
        });
        assert_eq!(parse_reply(&time_exceeded(0x0203)), want);
        assert_eq!(parse_reply(&with_ip(&time_exceeded(0x0203))), want);
    }

    #[test]
    fn rejects_truncated_and_garbage_packets() {
        let full = with_ip(&time_exceeded(5));
        // every cut must give None without panicking (the sequence number is the last read)
        for n in 0..full.len() {
            assert_eq!(parse_reply(&full[..n]), None, "len {n}");
        }
        assert_eq!(parse_reply(&[0x4f]), None);
        assert_eq!(parse_reply(&[0xff; 64]), None);
        assert_eq!(parse_reply(&[0x40; 64]), None); // IHL 0
        assert_eq!(parse_reply(&[3, 1, 0, 0, 0, 0, 0, 0]), None); // other type
        // time exceeded quoting something that is not an echo request
        let mut other = time_exceeded(5);
        other[8 + 20] = 3;
        assert_eq!(parse_reply(&other), None);
        // quoted header claims a huge IHL
        let mut big = time_exceeded(5);
        big[8] = 0x4f;
        assert_eq!(parse_reply(&big), None);
    }

    /// 40 byte IPv6 header (next header 58) from 2001:db8::1 to 2001:db8::2, then `rest`.
    fn with_ip6(rest: &[u8]) -> Vec<u8> {
        let mut v = vec![0x60, 0, 0, 0, 0, 16, 58, 64];
        v.extend_from_slice(&[0x20, 0x01, 0x0d, 0xb8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]);
        v.extend_from_slice(&[0x20, 0x01, 0x0d, 0xb8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2]);
        v.extend_from_slice(rest);
        v
    }

    fn time_exceeded6(seq: u16) -> Vec<u8> {
        let mut icmp = vec![3, 0, 0, 0, 0, 0, 0, 0];
        icmp.extend_from_slice(&with_ip6(&echo_request6(1, seq)));
        icmp
    }

    #[test]
    fn parses_icmpv6_echo_reply_type_129() {
        let reply = [129, 0, 0xab, 0xcd, 0x12, 0x34, 0x00, 0x07, 1, 2, 3];
        assert_eq!(parse_reply6(&reply), Some(Reply { seq: 7, echo: true }));
        assert_eq!(
            &echo_request6(0x1234, 9)[..8],
            &[128, 0, 0, 0, 0x12, 0x34, 0, 9]
        );
    }

    #[test]
    fn parses_icmpv6_time_exceeded_type_3() {
        let want = Some(Reply {
            seq: 0x0203,
            echo: false,
        });
        assert_eq!(parse_reply6(&time_exceeded6(0x0203)), want);
    }

    #[test]
    fn rejects_foreign_truncated_and_garbage_icmpv6() {
        let full = time_exceeded6(5);
        // 8 + 40 + 8 bytes are enough (the sequence number is the last read)
        for n in 0..56 {
            assert_eq!(parse_reply6(&full[..n]), None, "len {n}");
        }
        // neighbor advertisement (135), router advertisement (134), echo request (128)
        for t in [135, 134, 128] {
            assert_eq!(parse_reply6(&[t, 0, 0, 0, 0, 0, 0, 1, 2, 3]), None);
        }
        assert_eq!(parse_reply6(&[129, 1, 0, 0, 0, 0, 0, 7]), None); // wrong code
        let mut other = full.clone();
        other[8 + 40] = 129; // quoted packet is not our echo request
        assert_eq!(parse_reply6(&other), None);
        let mut v4 = full.clone();
        v4[8] = 0x45; // quoted header is not IPv6
        assert_eq!(parse_reply6(&v4), None);
        let mut udp = full;
        udp[8 + 6] = 17; // quoted next header is not ICMPv6
        assert_eq!(parse_reply6(&udp), None);
    }

    #[test]
    fn ipv6_path_through_the_same_measure() {
        struct Fake6(Vec<(Ipv6Addr, Reply)>);
        impl Prober<Ipv6Addr> for Fake6 {
            fn send(&mut self, ttl: u8, seq: u16) -> Result<(), String> {
                let (a, echo) = match ttl {
                    1 => ("fe80::1", false),
                    _ => ("2001:db8::9", true),
                };
                self.0.push((a.parse().unwrap(), Reply { seq, echo }));
                Ok(())
            }
            fn recv(&mut self, _: Duration) -> Option<(Ipv6Addr, Reply)> {
                (!self.0.is_empty()).then(|| self.0.remove(0))
            }
        }
        let t: Ipv6Addr = "2001:db8::9".parse().unwrap();
        let (raw, reached) = measure(&mut Fake6(vec![]), t, 5, 1).unwrap();
        assert!(reached);
        let hops = build_hops(raw, t);
        assert_eq!(hops.len(), 2);
        assert!(hops[0].lokal && !hops[0].erster_oeffentlicher);
        assert!(hops[1].ziel && hops[1].erster_oeffentlicher);
        let ip: IpAddr = "2001:db8::1".parse().unwrap();
        assert_eq!(
            arpa(ip),
            "1.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.8.b.d.0.1.0.0.2.ip6.arpa"
        );
    }

    #[test]
    fn private_ranges_and_first_public_hop() {
        for s in [
            "10.1.2.3",
            "172.16.0.1",
            "172.31.255.1",
            "192.168.178.1",
            "100.64.0.1",
            "100.127.255.254",
        ] {
            assert!(is_private(ip(s)), "{s}");
        }
        for s in [
            "100.63.255.255",
            "100.128.0.1",
            "172.32.0.1",
            "192.0.2.1",
            "198.51.100.7",
        ] {
            assert!(!is_private(ip(s)), "{s}");
        }
        let at = |a: &str| Raw {
            addrs: vec![ip(a)],
            rtts: vec![1.0],
            sent: 1,
        };
        let hops = build_hops(
            vec![
                at("192.168.178.1"),
                at("100.64.0.1"),
                at("198.51.100.1"),
                at("198.51.100.2"),
            ],
            ip("198.51.100.2"),
        );
        let flags: Vec<_> = hops
            .iter()
            .map(|h| (h.lokal, h.erster_oeffentlicher, h.ziel))
            .collect();
        assert_eq!(
            flags,
            [
                (true, false, false),
                (true, false, false),
                (false, true, false),
                (false, false, true)
            ]
        );
    }

    #[test]
    fn statistics_from_sample_values() {
        let raw = Raw {
            addrs: vec![ip("192.0.2.1"), ip("192.0.2.9")],
            rtts: vec![10.0, 30.0, 20.0, 40.0],
            sent: 5,
        };
        let h = &build_hops(vec![raw], ip("203.0.113.1"))[0];
        assert_eq!((h.gesendet, h.beantwortet), (5, 4));
        assert!((h.verlust_prozent - 20.0).abs() < 1e-9);
        assert_eq!(
            (h.min_ms, h.median_ms, h.p95_ms),
            (Some(10.0), Some(25.0), Some(40.0))
        );
        // |30-10| + |20-30| + |40-20| = 50 over 3 pairs
        assert!((h.jitter_ms.unwrap() - 50.0 / 3.0).abs() < 1e-9);
        assert_eq!(h.adressen.len(), 2);
        let silent = Raw {
            sent: 3,
            ..Raw::default()
        };
        let none = &build_hops(vec![silent], ip("203.0.113.1"))[0];
        assert_eq!(
            (none.median_ms, none.jitter_ms, none.verlust_prozent),
            (None, None, 100.0)
        );
    }

    /// Hops 1 to 3: 192.0.2.1, silent, 203.0.113.9 = target.
    struct Fake {
        queue: Vec<(Ipv4Addr, Reply)>,
        sent: Vec<u8>,
    }

    impl Prober for Fake {
        fn send(&mut self, ttl: u8, seq: u16) -> Result<(), String> {
            self.sent.push(ttl);
            match ttl {
                1 => self
                    .queue
                    .push((ip("192.0.2.1"), Reply { seq, echo: false })),
                2 => {}
                _ => self
                    .queue
                    .push((ip("203.0.113.9"), Reply { seq, echo: true })),
            }
            Ok(())
        }
        fn recv(&mut self, _: Duration) -> Option<(Ipv4Addr, Reply)> {
            (!self.queue.is_empty()).then(|| self.queue.remove(0))
        }
    }

    #[test]
    fn fake_path_with_silent_hop_stops_at_target() {
        let mut f = Fake {
            queue: vec![],
            sent: vec![],
        };
        let (raw, reached) = measure(&mut f, ip("203.0.113.9"), 8, 2).unwrap();
        assert!(reached);
        assert_eq!(raw.len(), 3);
        // round 1 sends TTL 1..=8, round 2 only up to the TTL that reached the target
        assert_eq!(f.sent, [1, 2, 3, 4, 5, 6, 7, 8, 1, 2, 3]);
        let hops = build_hops(raw, ip("203.0.113.9"));
        assert_eq!(hops[0].adressen, [IpAddr::V4(ip("192.0.2.1"))]);
        assert_eq!((hops[1].gesendet, hops[1].beantwortet), (2, 0));
        assert!(hops[2].ziel);
        assert_eq!(hops[2].beantwortet, 2);
    }

    #[test]
    fn echo_reply_from_wrong_address_is_ignored() {
        struct Spoof(Option<Reply>);
        impl Prober for Spoof {
            fn send(&mut self, _: u8, seq: u16) -> Result<(), String> {
                self.0 = Some(Reply { seq, echo: true });
                Ok(())
            }
            fn recv(&mut self, _: Duration) -> Option<(Ipv4Addr, Reply)> {
                self.0.take().map(|r| (ip("198.51.100.66"), r))
            }
        }
        let (_, reached) = measure(&mut Spoof(None), ip("203.0.113.9"), 1, 1).unwrap();
        assert!(!reached);
    }
}
