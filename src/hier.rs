//! `netzinfo hier` und `netzinfo subnetz`: siehe Issue #1.

use crate::explain;
use crate::subnetz::{self, Subnet};
use std::net::Ipv4Addr;
use std::process::Command;

/// What we know about the current network. Fields are None when macOS has no answer.
#[derive(Debug, Default, PartialEq, serde::Serialize)]
pub struct NetInfo {
    pub interface: String,
    pub ip: Option<Ipv4Addr>,
    /// None on VPN tunnels (utun) and other interfaces without DHCP.
    pub subnet: Option<Subnet>,
    pub router: Option<Ipv4Addr>,
    pub dhcp_server: Option<Ipv4Addr>,
    /// DNS servers handed out by DHCP.
    pub dhcp_dns: Vec<Ipv4Addr>,
    /// DNS servers the system really asks first (from scutil --dns), IPv4 and IPv6.
    pub system_dns: Vec<String>,
    pub domain: Option<String>,
}

#[derive(Debug, Default, PartialEq)]
struct Route {
    gateway: Option<String>,
    interface: Option<String>,
}

#[derive(Debug, Default, PartialEq)]
struct Lease {
    yiaddr: Option<Ipv4Addr>,
    mask: Option<Ipv4Addr>,
    router: Option<Ipv4Addr>,
    server: Option<Ipv4Addr>,
    dns: Vec<Ipv4Addr>,
    domain: Option<String>,
    /// DHCP option 252 (WPAD URL), if the server sends one.
    wpad: Option<String>,
}

fn field<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    line.trim()
        .strip_prefix(key)?
        .trim_start()
        .strip_prefix(':')
        .map(str::trim)
}

fn parse_route(out: &str) -> Route {
    let mut r = Route::default();
    for line in out.lines() {
        if let Some(v) = field(line, "gateway") {
            r.gateway = Some(v.to_string());
        } else if let Some(v) = field(line, "interface") {
            r.interface = Some(v.to_string());
        }
    }
    r
}

/// First address in `{a, b}` or plain `a` after the type marker.
fn ips(value: &str) -> Vec<Ipv4Addr> {
    value
        .trim_matches(|c| c == '{' || c == '}')
        .split(',')
        .filter_map(|s| s.trim().parse().ok())
        .collect()
}

fn parse_lease(out: &str) -> Lease {
    let mut l = Lease::default();
    for line in out.lines() {
        let Some((k, v)) = line.split_once('=').or_else(|| line.split_once(':')) else {
            continue;
        };
        // option lines look like `router (ip_mult): {192.0.2.1}`
        let key = k.split('(').next().unwrap_or("").trim();
        let v = v.trim();
        match key {
            "yiaddr" => l.yiaddr = ips(v).first().copied(),
            "subnet_mask" => l.mask = ips(v).first().copied(),
            "router" => l.router = ips(v).first().copied(),
            "server_identifier" => l.server = ips(v).first().copied(),
            "domain_name_server" => l.dns = ips(v),
            "domain_name" => l.domain = Some(v.to_string()).filter(|d| !d.is_empty()),
            // ponytail: name as printed by `ipconfig getpacket` on macOS; check it on a net that sends 252
            "proxy_auto_discovery" | "wpad" => {
                l.wpad = Some(v.to_string()).filter(|d| !d.is_empty())
            }
            _ => {}
        }
    }
    l
}

/// Nameservers of resolvers without a `domain` line: those answer for every name.
/// Domain-specific resolvers (`.local`, `ts.net`) are skipped.
fn parse_scutil_dns(out: &str) -> Vec<String> {
    let mut result: Vec<String> = Vec::new();
    for block in out.split("resolver #").skip(1) {
        if block.lines().any(|l| field(l, "domain").is_some()) {
            continue;
        }
        for l in block.lines() {
            if let Some(ns) = l.trim().strip_prefix("nameserver[")
                && let Some((_, v)) = ns.split_once(':')
            {
                let v = v.trim().to_string();
                if !result.contains(&v) {
                    result.push(v);
                }
            }
        }
    }
    result
}

fn sh(program: &str, args: &[&str]) -> Result<String, String> {
    let out = Command::new(program).args(args).output().map_err(|e| {
        format!("`{program}` ließ sich nicht starten ({e}). Läuft das auf einem Mac?")
    })?;
    if !out.status.success() {
        return Err(format!("`{program}` meldete einen Fehler."));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// WPAD URL from DHCP option 252 on `interface`, `None` without one.
pub fn wpad_dhcp(interface: &str) -> Option<String> {
    parse_lease(&sh("ipconfig", &["getpacket", interface]).ok()?).wpad
}

pub fn collect() -> Result<NetInfo, String> {
    let no_net = "Kein Netz gefunden: Es gibt keine Default-Route. Verbinde den Mac mit WLAN oder Kabel und versuche es noch mal.";
    let route =
        parse_route(&sh("route", &["-n", "get", "default"]).map_err(|_| no_net.to_string())?);
    let interface = route.interface.ok_or(no_net)?;
    // A VPN tunnel has no DHCP lease, so the lease and the mask stay empty there.
    let lease = sh("ipconfig", &["getpacket", &interface])
        .map(|o| parse_lease(&o))
        .unwrap_or_default();
    let ip = lease.yiaddr.or_else(|| {
        sh("ipconfig", &["getifaddr", &interface])
            .ok()?
            .trim()
            .parse()
            .ok()
    });
    let subnet = match (ip, lease.mask) {
        (Some(ip), Some(m)) => subnetz::calc(ip, subnetz::prefix_from_mask(m)?).ok(),
        _ => None,
    };
    let router = lease.router.or_else(|| route.gateway?.parse().ok());
    let system_dns = sh("scutil", &["--dns"])
        .map(|o| parse_scutil_dns(&o))
        .unwrap_or_default();
    Ok(NetInfo {
        interface,
        ip,
        subnet,
        router,
        dhcp_server: lease.server,
        dhcp_dns: lease.dns,
        system_dns,
        domain: lease.domain,
    })
}

fn show<T: std::fmt::Display>(v: Option<T>) -> String {
    v.map_or("unbekannt".into(), |v| v.to_string())
}

fn list<T: std::fmt::Display>(v: &[T]) -> String {
    if v.is_empty() {
        "unbekannt".into()
    } else {
        v.iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(", ")
    }
}

pub fn run(args: &[String], explain_on: bool) -> Result<(), String> {
    crate::optionen::nur(args, &[], "hier")?;
    let a = crate::netze::aktuell()?;
    crate::netze::hier_hinweis(&a, explain_on);
    let n = a.info;
    let vpn = n.interface.starts_with("utun");
    println!("Schnittstelle: {}", n.interface);
    explain(
        explain_on,
        &format!(
            "Die Default-Route sagt, über welche Schnittstelle Pakete ins Internet gehen. {}",
            if vpn {
                "„utun“ ist ein Tunnel, also ein VPN wie Tailscale oder WireGuard: Dein Verkehr geht gerade durch den Tunnel, nicht direkt ins WLAN."
            } else {
                "„en0“ ist beim Mac meistens das WLAN, andere „en“-Nummern sind Kabel oder Adapter."
            }
        ),
    );
    println!("IP-Adresse:    {}", show(n.ip));
    explain(
        explain_on,
        "Die IPv4-Adresse hat dir der DHCP-Server geliehen. Im Heimnetz ist sie privat (10.x, 172.16 bis 172.31, 192.168.x) und wird per NAT ins Internet übersetzt.",
    );
    match &n.subnet {
        Some(s) => {
            println!("Maske:         {} (/{})", s.mask, s.prefix);
            println!("Netzadresse:   {}", s.network);
            println!("Broadcast:     {}", s.broadcast);
            println!(
                "Hostbereich:   {} bis {} ({} Hosts)",
                s.first, s.last, s.hosts
            );
            explain(
                explain_on,
                "Die Maske trennt Netzanteil und Hostanteil. IP UND Maske ergibt die Netzadresse, alle Hostbits auf 1 ergeben den Broadcast. Den Rechenweg zeigt `netzinfo subnetz <IP> <Maske> --erklaer`.",
            );
        }
        None => {
            println!("Maske:         unbekannt");
            explain(
                explain_on,
                "Ohne DHCP-Antwort (z. B. bei einem VPN-Tunnel) kennt der Mac die Maske nicht, deshalb lässt sich das Subnetz nicht ausrechnen.",
            );
        }
    }
    println!("Router:        {}", show(n.router));
    explain(
        explain_on,
        "Der Router (Gateway) ist die Tür aus dem eigenen Subnetz. Alles, was nicht im Subnetz liegt, schickt der Mac an ihn.",
    );
    println!("DHCP-Server:   {}", show(n.dhcp_server));
    explain(
        explain_on,
        "Der DHCP-Server vergibt IP, Maske, Router und DNS (Ablauf DORA: Discover, Offer, Request, Ack). Im Heimnetz ist er fast immer der Router.",
    );
    println!("DNS (DHCP):    {}", list(&n.dhcp_dns));
    println!("DNS (System):  {}", list(&n.system_dns));
    let magic = n.system_dns.iter().any(|d| d == "100.100.100.100");
    explain(
        explain_on,
        if magic {
            "DNS übersetzt Namen in IP-Adressen. 100.100.100.100 ist der DNS-Dienst von Tailscale (MagicDNS), er sitzt vor dem DNS aus dem DHCP und reicht normale Fragen weiter."
        } else {
            "DNS übersetzt Namen in IP-Adressen. „System“ ist, was der Mac wirklich fragt, „DHCP“ ist, was das Netz angeboten hat. Beides sollte meist übereinstimmen."
        },
    );
    println!("Domain:        {}", show(n.domain.as_ref()));
    explain(
        explain_on,
        "Die Domain vom DHCP-Server wird an unvollständige Namen angehängt (Suchdomain).",
    );
    crate::ipv6router::zeigen(&n, explain_on);
    Ok(())
}

pub fn run_subnet(args: &[String], explain_on: bool) -> Result<(), String> {
    crate::optionen::nur(args, &[], "subnetz")?;
    let (ip, prefix) = subnetz::parse_args(args)?;
    let s = subnetz::calc(ip, prefix)?;
    println!("Adresse:       {ip}/{prefix}");
    println!("Maske:         {} ({})", s.mask, subnetz::binary(s.mask));
    println!("Netzadresse:   {}", s.network);
    println!("Broadcast:     {}", s.broadcast);
    println!("Erster Host:   {}", s.first);
    println!("Letzter Host:  {}", s.last);
    println!("Anzahl Hosts:  {}", s.hosts);
    if explain_on {
        println!();
        explain(true, "Rechenweg wie in der Prüfung:");
        println!("  IP      {}  {ip}", subnetz::binary(ip));
        println!("  Maske   {}  {}", subnetz::binary(s.mask), s.mask);
        println!("  UND     {}  {}", subnetz::binary(s.network), s.network);
        explain(
            true,
            "UND-Verknüpfung: Ein Bit bleibt nur 1, wenn es in IP und Maske 1 ist. Das Ergebnis ist die Netzadresse.",
        );
        let host = Ipv4Addr::from(u32::from(ip) & !u32::from(s.mask));
        println!("  Host    {}  {host}", subnetz::binary(host));
        explain(
            true,
            &format!(
                "Hostanteil: die letzten {} Bit der IP (IP UND invertierte Maske). Es bleibt der Teil, der das Gerät im Netz benennt.",
                32 - u32::from(prefix)
            ),
        );
        explain(
            true,
            &format!(
                "Broadcast: alle {} Hostbits auf 1 setzen, hier {}.",
                32 - u32::from(prefix),
                subnetz::binary(s.broadcast)
            ),
        );
        explain(true, &match prefix {
            32 => "Bei /32 gibt es nur diesen einen Host, kein Netz mit Broadcast.".to_string(),
            31 => "Bei /31 gibt es keine Netz- und Broadcastadresse, beide Adressen sind Hosts (RFC 3021, Punkt-zu-Punkt-Verbindung).".to_string(),
            _ => format!("Hosts: 2^{} minus 2 (Netzadresse und Broadcast sind nicht vergebbar) = {}.", 32 - u32::from(prefix), s.hosts),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROUTE: &str = "   route to: default\ndestination: default\n       mask: default\n    gateway: 192.168.178.1\n  interface: en0\n      flags: <UP,GATEWAY,DONE,STATIC,PRCLONING,GLOBAL>\n";
    const ROUTE_UTUN: &str =
        "   route to: default\ndestination: default\n    gateway: 100.64.0.1\n  interface: utun4\n";
    const PACKET: &str = "op = BOOTREPLY\nyiaddr = 192.168.178.20\nsiaddr = 0.0.0.0\nchaddr = 02:00:00:aa:bb:cc\nsname = fritz.box\noptions:\nOptions count is 11\ndhcp_message_type (uint8): ACK 0x5\nserver_identifier (ip): 192.168.178.1\nlease_time (uint32): 0x1baf80\nsubnet_mask (ip): 255.255.255.0\nrouter (ip_mult): {192.168.178.1}\ndomain_name_server (ip_mult): {192.168.178.1, 192.0.2.53}\ndomain_name (string): fritz.box\nend (none): \n";
    const SCUTIL: &str = "DNS configuration\n\nresolver #1\n  search domain[0] : example.ts.net\n  nameserver[0] : 100.100.100.100\n  nameserver[1] : fd7a:115c:a1e0::53\n  if_index : 22 (utun9)\n  flags    : Supplemental\n\nresolver #2\n  nameserver[0] : fe80::1%en0\n  nameserver[1] : 192.168.178.1\n  if_index : 8 (en0)\n\nresolver #3\n  domain   : fritz.box.\n  nameserver[0] : 100.100.100.100\n\nresolver #4\n  domain   : local\n  options  : mdns\n";

    #[test]
    fn route_wifi_and_utun() {
        let r = parse_route(ROUTE);
        assert_eq!(r.interface.as_deref(), Some("en0"));
        assert_eq!(r.gateway.as_deref(), Some("192.168.178.1"));
        assert_eq!(parse_route(ROUTE_UTUN).interface.as_deref(), Some("utun4"));
    }

    #[test]
    fn route_without_default_is_empty() {
        assert_eq!(
            parse_route("route: writing to routing socket: not in table\n"),
            Route::default()
        );
    }

    #[test]
    fn lease_fields() {
        let l = parse_lease(PACKET);
        assert_eq!(l.yiaddr, "192.168.178.20".parse().ok());
        assert_eq!(l.mask, "255.255.255.0".parse().ok());
        assert_eq!(l.router, "192.168.178.1".parse().ok());
        assert_eq!(l.server, "192.168.178.1".parse().ok());
        assert_eq!(l.dns.len(), 2);
        assert_eq!(l.domain.as_deref(), Some("fritz.box"));
    }

    #[test]
    fn lease_wpad_option_252() {
        let l = parse_lease(
            "yiaddr = 192.0.2.20\nproxy_auto_discovery (string): http://wpad.example.net/wpad.dat\n",
        );
        assert_eq!(l.wpad.as_deref(), Some("http://wpad.example.net/wpad.dat"));
        assert_eq!(parse_lease("router (ip_mult): {192.0.2.1}\n").wpad, None);
    }

    #[test]
    fn lease_empty_input() {
        assert_eq!(parse_lease(""), Lease::default());
    }

    #[test]
    fn scutil_keeps_only_global_resolvers() {
        assert_eq!(
            parse_scutil_dns(SCUTIL),
            vec![
                "100.100.100.100",
                "fd7a:115c:a1e0::53",
                "fe80::1%en0",
                "192.168.178.1"
            ]
        );
        assert!(parse_scutil_dns("").is_empty());
    }
}
