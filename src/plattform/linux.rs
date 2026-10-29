//! Linux: Default-Route aus `/proc/net/route`, Adresse über `getifaddrs`, DHCP aus
//! systemd-networkd oder NetworkManager, DNS auch hinter systemd-resolved.
//! Kein Root, keine Shell. Die Parser sind reiner Text und laufen auf jedem System.

#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

use crate::hier::NetInfo;
use crate::subnetz;
use std::net::Ipv4Addr;
use std::process::Command;

#[derive(Debug, Default, PartialEq)]
struct Lease {
    server: Option<Ipv4Addr>,
    dns: Vec<Ipv4Addr>,
    domain: Option<String>,
}

fn ips(werte: &str) -> Vec<Ipv4Addr> {
    werte
        .split_whitespace()
        .filter_map(|s| s.parse().ok())
        .collect()
}

/// Schnittstelle und Gateway der Default-Route mit der kleinsten Metrik.
fn parse_route(text: &str) -> Option<(String, Ipv4Addr)> {
    let mut beste: Option<(u32, String, Ipv4Addr)> = None;
    for z in text.lines().skip(1) {
        let s: Vec<&str> = z.split_whitespace().collect();
        let &[name, ziel, gw, flags, _, _, metrik, maske, ..] = s.as_slice() else {
            continue;
        };
        let hex = |t| u32::from_str_radix(t, 16).ok();
        let (Some(ziel), Some(gw), Some(flags), Some(maske)) =
            (hex(ziel), hex(gw), hex(flags), hex(maske))
        else {
            continue;
        };
        let Ok(metrik) = metrik.parse::<u32>() else {
            continue;
        };
        // 0x1 = UP, 0x2 = GATEWAY; die Adressen stehen in Host-Reihenfolge (Little Endian)
        if ziel != 0
            || maske != 0
            || flags & 3 != 3
            || beste.as_ref().is_some_and(|b| b.0 <= metrik)
        {
            continue;
        }
        beste = Some((metrik, name.to_string(), Ipv4Addr::from(gw.to_le_bytes())));
    }
    beste.map(|(_, n, g)| (n, g))
}

/// systemd-networkd: `/run/systemd/netif/leases/<ifindex>`, Zeilen `SCHLÜSSEL=Wert`.
fn parse_networkd(text: &str) -> Lease {
    let mut l = Lease::default();
    for (k, v) in text.lines().filter_map(|z| z.split_once('=')) {
        match k {
            "SERVER_ADDRESS" => l.server = v.trim().parse().ok(),
            "DNS" => l.dns = ips(v),
            "DOMAINNAME" => l.domain = Some(v.trim().to_string()).filter(|d| !d.is_empty()),
            _ => {}
        }
    }
    l
}

/// NetworkManager: `nmcli -t -f DHCP4 device show <if>`, Zeilen `DHCP4.OPTION[n]:name = wert`.
fn parse_nmcli(text: &str) -> Lease {
    let mut l = Lease::default();
    for z in text.lines() {
        let Some((_, opt)) = z.split_once(':') else {
            continue;
        };
        match opt.split_once(" = ") {
            Some(("dhcp_server_identifier", v)) => l.server = v.trim().parse().ok(),
            Some(("domain_name_servers", v)) => l.dns = ips(v),
            Some(("domain_name", v)) => {
                l.domain = Some(v.trim().to_string()).filter(|d| !d.is_empty())
            }
            _ => {}
        }
    }
    l
}

/// IPv4-Adresse und Präfix der Schnittstelle, `None` ohne Adresse.
fn adresse(schnittstelle: &str) -> Option<(Ipv4Addr, u8)> {
    let mut erste: *mut libc::ifaddrs = std::ptr::null_mut();
    let mut ergebnis = None;
    // SAFETY: Die Liste von getifaddrs wird nur gelesen und einmal freigegeben; Namen sind
    // NUL-terminierte C-Strings, Adresse und Maske der AF_INET-Einträge sind sockaddr_in.
    unsafe {
        if libc::getifaddrs(&mut erste) != 0 {
            return None;
        }
        let mut p = erste;
        while !p.is_null() {
            let a = &*p;
            p = a.ifa_next;
            if a.ifa_addr.is_null()
                || a.ifa_netmask.is_null()
                || i32::from((*a.ifa_addr).sa_family) != libc::AF_INET
                || std::ffi::CStr::from_ptr(a.ifa_name).to_bytes() != schnittstelle.as_bytes()
            {
                continue;
            }
            let v4 = |s: *const libc::sockaddr| {
                Ipv4Addr::from(u32::from_be(
                    (*s.cast::<libc::sockaddr_in>()).sin_addr.s_addr,
                ))
            };
            let maske = v4(a.ifa_netmask);
            ergebnis = subnetz::prefix_from_mask(maske)
                .ok()
                .map(|p| (v4(a.ifa_addr), p));
            break;
        }
        libc::freeifaddrs(erste);
    }
    ergebnis
}

fn dhcp(schnittstelle: &str) -> Lease {
    let networkd = std::fs::read_to_string(format!("/sys/class/net/{schnittstelle}/ifindex"))
        .ok()
        .and_then(|i| {
            std::fs::read_to_string(format!("/run/systemd/netif/leases/{}", i.trim())).ok()
        });
    if let Some(t) = networkd {
        return parse_networkd(&t);
    }
    // Kein nmcli oder keine Antwort ist kein Fehler.
    Command::new("nmcli")
        .args(["-t", "-f", "DHCP4", "device", "show", schnittstelle])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| parse_nmcli(&String::from_utf8_lossy(&o.stdout)))
        .unwrap_or_default()
}

pub fn lies_netzdaten() -> Result<NetInfo, String> {
    let (interface, gateway) = std::fs::read_to_string("/proc/net/route")
        .ok()
        .and_then(|t| parse_route(&t))
        .ok_or("Kein Netz gefunden: Es gibt keine Default-Route. Verbinde den Rechner mit dem Netz und versuche es noch mal.")?;
    let adresse = adresse(&interface);
    let lease = dhcp(&interface);
    Ok(NetInfo {
        ip: adresse.map(|a| a.0),
        subnet: adresse.and_then(|(ip, p)| subnetz::calc(ip, p).ok()),
        router: Some(gateway),
        dhcp_server: lease.server,
        dhcp_dns: lease.dns,
        system_dns: system_dns(),
        domain: lease.domain,
        interface,
    })
}

/// DNS-Server, die das System fragt. Steht in `/etc/resolv.conf` nur der Stub von
/// systemd-resolved, gelten die echten Server aus `/run/systemd/resolve/resolv.conf`.
pub fn system_dns() -> Vec<String> {
    system_dns_aus(
        &std::fs::read_to_string("/etc/resolv.conf").unwrap_or_default(),
        || std::fs::read_to_string("/run/systemd/resolve/resolv.conf").ok(),
    )
}

fn system_dns_aus(resolv: &str, echt: impl FnOnce() -> Option<String>) -> Vec<String> {
    let liste = nameserver(resolv);
    let stub = !liste.is_empty() && liste.iter().all(|s| s == "127.0.0.53" || s == "127.0.0.54");
    if !stub {
        return liste;
    }
    match echt().map(|t| nameserver(&t)) {
        Some(echt) if !echt.is_empty() => echt,
        _ => liste,
    }
}

pub fn wpad_dhcp(_schnittstelle: &str) -> Option<String> {
    None
}

fn nameserver(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|l| Some(l.trim().strip_prefix("nameserver ")?.trim().to_string()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROUTE: &str = include_str!("../../testdata/linux/proc_net_route");
    const NETWORKD: &str = include_str!("../../testdata/linux/networkd_lease");
    const NMCLI: &str = include_str!("../../testdata/linux/nmcli_dhcp4");
    const STUB: &str = include_str!("../../testdata/linux/resolv_stub.conf");
    const ECHT: &str = include_str!("../../testdata/linux/resolved_resolv.conf");

    #[test]
    fn nameserver_aus_resolv_conf() {
        assert_eq!(
            nameserver("# x\nnameserver 192.0.2.53\nsearch example.net\nnameserver 2001:db8::53\n"),
            ["192.0.2.53", "2001:db8::53"]
        );
        assert!(nameserver("nothing").is_empty());
    }

    #[test]
    fn default_route_mit_kleinster_metrik() {
        // wlan0 (600) und eth0 (100) sind Default-Routen, tun0 hat kein Gateway-Flag
        assert_eq!(
            parse_route(ROUTE),
            Some(("eth0".into(), "192.0.2.1".parse().unwrap()))
        );
    }

    #[test]
    fn keine_default_route() {
        let nur_netz = ROUTE
            .lines()
            .filter(|l| !l.contains("\t0003\t"))
            .collect::<Vec<_>>();
        assert_eq!(parse_route(&nur_netz.join("\n")), None);
        assert_eq!(parse_route(""), None);
    }

    #[test]
    fn lease_networkd_und_nmcli_gleich() {
        let erwartet = Lease {
            server: "192.0.2.2".parse().ok(),
            dns: vec![
                "192.0.2.53".parse().unwrap(),
                "198.51.100.53".parse().unwrap(),
            ],
            domain: Some("example.net".into()),
        };
        assert_eq!(parse_networkd(NETWORKD), erwartet);
        assert_eq!(parse_nmcli(NMCLI), erwartet);
        assert_eq!(parse_networkd(""), Lease::default());
        assert_eq!(parse_nmcli("Fehler"), Lease::default());
    }

    #[test]
    fn stub_von_systemd_resolved_wird_ersetzt() {
        let echt = || Some(ECHT.to_string());
        assert_eq!(system_dns_aus(STUB, echt), ["192.0.2.53", "2001:db8::53"]);
        // echte Server in resolv.conf bleiben, die Ersatzdatei wird nicht gebraucht
        assert_eq!(
            system_dns_aus("nameserver 192.0.2.1\n", || panic!("nicht lesen")),
            ["192.0.2.1"]
        );
        // fehlt die Ersatzdatei, bleibt der Stub
        assert_eq!(system_dns_aus(STUB, || None), ["127.0.0.53"]);
    }
}
