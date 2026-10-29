//! Neighbour table without root (issue #26): MAC addresses of the devices around us.
//!
//! The kernel fills its ARP/NDP table itself when we send any packet to an address. We send
//! unprivileged ICMP echos (`anstossen`), then read the table via sysctl (`lesen`), the way
//! `arp` and `ndp` do. Reading is passive; sending is only done by callers behind the „meins“ gate.

use serde::Serialize;
use socket2::{Domain, Protocol, SockAddr, Socket, Type};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddrV4, SocketAddrV6};
use std::time::Duration;

/// One device on the link, identified by its MAC.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Nachbar {
    /// `aa:bb:cc:dd:ee:ff`, lower case.
    pub mac: String,
    pub ipv4: Vec<Ipv4Addr>,
    pub ipv6: Vec<Ipv6Addr>,
    pub schnittstelle: String,
}

/// One row of the kernel table. `mac` is None while the kernel still waits for an answer.
#[derive(Debug, Clone, PartialEq)]
struct Eintrag {
    ip: IpAddr,
    mac: Option<[u8; 6]>,
    index: u16,
}

/// Error text for „table empty, not even the router in it“. Checked on macOS 26: without the
/// Local Network grant the kernel hands `arp`, `netstat` and sysctl an empty IPv4 table while
/// the router answers pings; a VPN (Tailscale was connected) made no difference. The grant is
/// kept per program, so a bare command-line tool may never get asked: the app is the safe way.
pub const LEERE_TABELLE: &str = "Die Nachbartabelle ist leer, nicht einmal der Router steht darin. Meist fehlt die Freigabe „Lokales Netzwerk“: macOS zeigt dem Programm dann keine Geräte im Netz, obwohl der Router antwortet. Öffne die Netzinfo-App, such dort Geräte und erlaube den Dialog (später änderbar unter Systemeinstellungen > Datenschutz & Sicherheit > Lokales Netzwerk). Ein VPN allein erklärt es nicht. Auch `arp -an` im selben Programm bleibt dann leer; im Terminal kann es trotzdem Einträge zeigen, weil jedes Programm seine eigene Freigabe braucht.";

const RT_MSGHDR_LEN: usize = 92; // sizeof(struct rt_msghdr) on macOS
const AF_LINK: u8 = 18;
const WAIT: Duration = Duration::from_millis(300);

fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_ne_bytes(b.get(at..at + 2)?.try_into().ok()?))
}

/// Parses a sysctl `NET_RT_FLAGS` buffer: a chain of `rt_msghdr`, each followed by a
/// `sockaddr_in`/`sockaddr_in6` (destination) and a `sockaddr_dl` (gateway = MAC).
/// Every read is bounds checked; a damaged message ends the parse.
fn parse(buf: &[u8]) -> Vec<Eintrag> {
    let mut out = Vec::new();
    let mut off = 0;
    while let Some(len) = u16_at(buf, off).map(usize::from) {
        let Some(msg) = buf.get(off..off + len).filter(|_| len >= RT_MSGHDR_LEN) else {
            break;
        };
        off += len;
        let Some(addrs) = msg.get(12..16) else {
            break;
        };
        let addrs = i32::from_ne_bytes([addrs[0], addrs[1], addrs[2], addrs[3]]);
        // sockaddrs follow in bit order of rtm_addrs, each padded to 4 bytes
        let (mut dst, mut gw) = (None, None);
        let mut pos = RT_MSGHDR_LEN;
        for bit in 0..8 {
            if addrs & (1 << bit) == 0 {
                continue;
            }
            let Some(&sa_len) = msg.get(pos) else { break };
            let sa_len = usize::from(sa_len);
            let sa = msg.get(pos..pos + sa_len);
            match bit {
                0 => dst = sa,
                1 => gw = sa,
                _ => {}
            }
            pos += if sa_len == 0 {
                4
            } else {
                sa_len.div_ceil(4) * 4
            };
        }
        let (Some(dst), Some(gw)) = (dst, gw) else {
            continue;
        };
        let ip = match (dst.get(1), dst.get(4..8), dst.get(8..24)) {
            (Some(2), Some(a), _) => IpAddr::from([a[0], a[1], a[2], a[3]]),
            (Some(30), _, Some(a)) => {
                let mut a: [u8; 16] = a.try_into().unwrap_or_default();
                // KAME stores the scope id inside link-local addresses; remove it
                if a[0] == 0xfe && a[1] & 0xc0 == 0x80 {
                    a[2] = 0;
                    a[3] = 0;
                }
                IpAddr::from(a)
            }
            _ => continue,
        };
        // sockaddr_dl: family @1, nlen @5, alen @6, data @8 = name, then link address
        let mac = (gw.get(1) == Some(&AF_LINK))
            .then(|| (gw.get(5), gw.get(6)))
            .and_then(|(n, a)| Some((usize::from(*n?), *a?)))
            .filter(|(_, alen)| *alen == 6)
            .and_then(|(nlen, _)| gw.get(8 + nlen..8 + nlen + 6))
            .and_then(|m| <[u8; 6]>::try_from(m).ok());
        // rtm_index is 1 for these rows; the interface is sdl_index of the gateway address
        let Some(index) = u16_at(gw, 2) else { continue };
        out.push(Eintrag { ip, mac, index });
    }
    out
}

/// Groups rows by MAC. Drops incomplete rows, multicast/broadcast MACs and addresses,
/// loopback, and rows of other interfaces.
fn gruppieren(rows: &[Eintrag], index: u16, schnittstelle: &str) -> Vec<Nachbar> {
    let mut out: Vec<Nachbar> = Vec::new();
    for r in rows.iter().filter(|r| r.index == index) {
        let Some(m) = r.mac.filter(|m| m[0] & 1 == 0) else {
            continue;
        };
        let mac = m.map(|b| format!("{b:02x}")).join(":");
        let skip = match r.ip {
            IpAddr::V4(a) => a.is_multicast() || a.is_broadcast() || a.is_loopback(),
            IpAddr::V6(a) => a.is_multicast() || a.is_loopback(),
        };
        if skip {
            continue;
        }
        let i = match out.iter().position(|n| n.mac == mac) {
            Some(i) => i,
            None => {
                out.push(Nachbar {
                    mac,
                    ipv4: vec![],
                    ipv6: vec![],
                    schnittstelle: schnittstelle.into(),
                });
                out.len() - 1
            }
        };
        match r.ip {
            IpAddr::V4(a) => out[i].ipv4.push(a),
            IpAddr::V6(a) => out[i].ipv6.push(a),
        }
    }
    out.sort_by(|a, b| a.mac.cmp(&b.mac));
    out
}

/// Reads one address family of the kernel table. No program is started, no packet sent.
#[cfg(target_os = "macos")]
fn sysctl_table(family: libc::c_int) -> Result<Vec<u8>, String> {
    let mut mib = [
        libc::CTL_NET,
        libc::PF_ROUTE,
        0,
        family,
        libc::NET_RT_FLAGS,
        libc::RTF_LLINFO,
    ];
    let fail = |e: std::io::Error| {
        format!(
            "Die Nachbartabelle ließ sich nicht lesen ({}).",
            crate::io_satz(&e)
        )
    };
    // the table can grow between the size query and the read: retry a few times
    for _ in 0..4 {
        let mut len = 0usize;
        // SAFETY: mib has 6 valid ints; a null buffer only asks for the needed size in `len`.
        let r = unsafe {
            libc::sysctl(
                mib.as_mut_ptr(),
                6,
                std::ptr::null_mut(),
                &mut len,
                std::ptr::null_mut(),
                0,
            )
        };
        if r != 0 {
            return Err(fail(std::io::Error::last_os_error()));
        }
        let mut buf = vec![0u8; len + len / 8 + 1024];
        let mut len = buf.len();
        // SAFETY: buf is writable for `len` bytes; the kernel writes at most `len` bytes and
        // updates `len` to the number written.
        let r = unsafe {
            libc::sysctl(
                mib.as_mut_ptr(),
                6,
                buf.as_mut_ptr().cast(),
                &mut len,
                std::ptr::null_mut(),
                0,
            )
        };
        if r == 0 {
            buf.truncate(len);
            return Ok(buf);
        }
        let e = std::io::Error::last_os_error();
        if e.raw_os_error() != Some(libc::ENOMEM) {
            return Err(fail(e));
        }
    }
    Err(fail(std::io::Error::from_raw_os_error(libc::ENOMEM)))
}

// ponytail: Linux (netlink RTM_GETNEIGH) and Windows (GetIpNetTable2) are not built yet
#[cfg(not(target_os = "macos"))]
fn sysctl_table(_family: libc::c_int) -> Result<Vec<u8>, String> {
    Err("Die Nachbartabelle kann ich bisher nur unter macOS lesen.".into())
}

fn if_index(name: &str) -> Result<u32, String> {
    let c = std::ffi::CString::new(name).map_err(|_| "Ungültiger Schnittstellenname.")?;
    // SAFETY: c is a valid NUL terminated string for the duration of the call.
    match unsafe { libc::if_nametoindex(c.as_ptr()) } {
        0 => Err(format!("Die Schnittstelle „{name}“ gibt es nicht.")),
        i => Ok(i),
    }
}

/// The devices the kernel knows on `schnittstelle`, IPv4 and IPv6 merged by MAC.
pub fn lesen(schnittstelle: &str) -> Result<Vec<Nachbar>, String> {
    let index =
        u16::try_from(if_index(schnittstelle)?).map_err(|_| "Schnittstellennummer zu groß.")?;
    let mut rows = parse(&sysctl_table(libc::AF_INET)?);
    rows.extend(parse(&sysctl_table(libc::AF_INET6)?));
    Ok(gruppieren(&rows, index, schnittstelle))
}

fn echo_request(kind: u8) -> [u8; 8] {
    let mut p = [kind, 0, 0, 0, 0x4e, 0x5a, 0, 1]; // type, code, checksum, id, seq
    if kind == 8 {
        // ICMPv4 checksum: the kernel does not fill it in; ICMPv6 it does
        let sum: u32 = p
            .chunks(2)
            .map(|c| u32::from(u16::from_be_bytes([c[0], c[1]])))
            .sum();
        let sum = (sum & 0xffff) + (sum >> 16);
        let ck = !(((sum & 0xffff) + (sum >> 16)) as u16);
        p[2..4].copy_from_slice(&ck.to_be_bytes());
    }
    p
}

/// Sends one unprivileged ICMP echo to every host (answers are ignored: the kernel's ARP
/// attempt already fills the table), and, for real ranges, an ICMPv6 echo to all nodes on the
/// link. Waits 300 ms afterwards so slow replies can land in the table.
/// Callers must have passed the „meins“ gate.
pub fn anstossen(hosts: &[Ipv4Addr], schnittstelle: &str) -> Result<(), String> {
    let s = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::ICMPV4)).map_err(|e| {
        format!("Ein Ping-Socket ließ sich nicht öffnen ({}). Ohne ihn kann ich die Nachbartabelle nicht füllen.", crate::io_satz(&e))
    })?;
    let p = echo_request(8);
    for h in hosts {
        // an unreachable host is no error here: the attempt is the point
        let _ = s.send_to(&p, &SockAddr::from(SocketAddrV4::new(*h, 0)));
    }
    if hosts.len() > 1 && !hosts[0].is_loopback() {
        // best effort: IPv6 is optional
        if let (Ok(i), Ok(s6)) = (
            if_index(schnittstelle),
            Socket::new(Domain::IPV6, Type::DGRAM, Some(Protocol::ICMPV6)),
        ) {
            let all_nodes = Ipv6Addr::new(0xff02, 0, 0, 0, 0, 0, 0, 1);
            let _ = s6.set_multicast_if_v6(i);
            let _ = s6.send_to(
                &echo_request(128),
                &SockAddr::from(SocketAddrV6::new(all_nodes, 0, 0, i)),
            );
        }
    }
    std::thread::sleep(WAIT);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One `rt_msghdr` (92 bytes) plus sockaddrs, built like the macOS kernel does.
    fn msg(index: u16, dst: &[u8], gw: &[u8]) -> Vec<u8> {
        let pad = |mut v: Vec<u8>| {
            v.resize(v.len().div_ceil(4) * 4, 0);
            v
        };
        let body = [pad(dst.to_vec()), pad(gw.to_vec())].concat();
        let mut m = vec![0u8; RT_MSGHDR_LEN];
        let total = (RT_MSGHDR_LEN + body.len()) as u16;
        m[0..2].copy_from_slice(&total.to_ne_bytes());
        let _ = index; // rtm_index stays 0 on purpose: the parser must not rely on it
        m[12..16].copy_from_slice(&3i32.to_ne_bytes()); // RTA_DST | RTA_GATEWAY
        m.extend(body);
        m
    }

    fn sin(a: [u8; 4]) -> Vec<u8> {
        let mut v = vec![16, 2, 0, 0, a[0], a[1], a[2], a[3]];
        v.resize(16, 0);
        v
    }

    fn sin6(a: [u8; 16]) -> Vec<u8> {
        let mut v = vec![28, 30, 0, 0, 0, 0, 0, 0];
        v.extend(a);
        v.resize(28, 0);
        v
    }

    /// sockaddr_dl with interface name "en0" and an optional MAC.
    fn sdl(index: u16, mac: Option<[u8; 6]>) -> Vec<u8> {
        let alen = if mac.is_some() { 6 } else { 0 };
        let mut v = vec![0, AF_LINK, 0, 0, 6, 3, alen, 0];
        v[2..4].copy_from_slice(&index.to_ne_bytes());
        v.extend(b"en0");
        v.extend(mac.unwrap_or_default().iter().take(alen as usize));
        v[0] = v.len() as u8;
        v
    }

    const M1: [u8; 6] = [2, 0, 0, 0, 0, 1];
    const M2: [u8; 6] = [2, 0, 0, 0, 0, 2];

    fn v6(last: u16) -> [u8; 16] {
        Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, last).octets()
    }

    #[test]
    fn parses_ipv4_ipv6_and_incomplete() {
        let mut fe80 = v6(0x11);
        fe80[..2].copy_from_slice(&[0xfe, 0x80]);
        fe80[2..4].copy_from_slice(&[0, 4]); // embedded scope id
        let buf = [
            msg(4, &sin([192, 168, 178, 1]), &sdl(4, Some(M1))),
            msg(4, &sin6(v6(1)), &sdl(4, Some(M1))),
            msg(4, &sin6(fe80), &sdl(4, Some(M1))),
            msg(4, &sin([192, 168, 178, 9]), &sdl(4, None)),
        ]
        .concat();
        let rows = parse(&buf);
        assert_eq!(rows.len(), 4);
        assert_eq!(rows[0].ip, "192.168.178.1".parse::<IpAddr>().unwrap());
        assert_eq!(rows[0].mac, Some(M1));
        assert_eq!(rows[1].ip, "2001:db8::1".parse::<IpAddr>().unwrap());
        assert_eq!(rows[2].ip, "fe80::11".parse::<IpAddr>().unwrap());
        assert_eq!((rows[3].mac, rows[3].index), (None, 4));
    }

    #[test]
    fn groups_by_mac_and_drops_noise() {
        let row = |ip: &str, mac: Option<[u8; 6]>, index| Eintrag {
            ip: ip.parse().unwrap(),
            mac,
            index,
        };
        let rows = [
            row("192.168.178.1", Some(M1), 4),
            row("2001:db8::1", Some(M1), 4),
            row("192.168.178.7", Some(M2), 4),
            row("192.168.178.9", None, 4), // incomplete
            row("224.0.0.251", Some([1, 0, 0x5e, 0, 0, 0xfb]), 4), // multicast MAC
            row("192.168.178.255", Some([0xff; 6]), 4), // broadcast
            row("192.168.178.5", Some(M2), 7), // other interface
        ];
        let n = gruppieren(&rows, 4, "en0");
        assert_eq!(n.len(), 2);
        assert_eq!(n[0].mac, "02:00:00:00:00:01");
        assert_eq!(n[0].ipv4, ["192.168.178.1".parse::<Ipv4Addr>().unwrap()]);
        assert_eq!(n[0].ipv6, ["2001:db8::1".parse::<Ipv6Addr>().unwrap()]);
        assert_eq!(
            (n[1].mac.as_str(), n[1].ipv4.len()),
            ("02:00:00:00:00:02", 1)
        );
    }

    #[test]
    fn damaged_buffers_never_panic() {
        let good = msg(4, &sin([192, 168, 178, 1]), &sdl(4, Some(M1)));
        for cut in 0..good.len() {
            let _ = parse(&good[..cut]);
        }
        let mut bad = good.clone();
        bad[0..2].copy_from_slice(&3u16.to_ne_bytes()); // length below the header size
        assert!(parse(&bad).is_empty());
        let mut bad = good;
        bad[RT_MSGHDR_LEN + 16 + 5] = 250; // nlen far beyond the buffer
        assert_eq!(parse(&bad)[0].mac, None);
        assert!(parse(&[]).is_empty());
    }

    #[test]
    fn echo_checksum_is_valid() {
        let p = echo_request(8);
        let sum: u32 = p
            .chunks(2)
            .map(|c| u32::from(u16::from_be_bytes([c[0], c[1]])))
            .sum();
        assert_eq!((sum & 0xffff) + (sum >> 16), 0xffff);
    }
}
