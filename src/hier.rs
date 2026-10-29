//! `netzinfo hier` und `netzinfo subnetz`: siehe Issue #1.

use crate::explain;
use crate::subnetz::{self, Subnet};
use std::net::Ipv4Addr;

/// What we know about the current network. Fields are None when the system has no answer.
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
    /// DNS servers the system really asks first (scutil --dns on macOS, resolv.conf on Linux), IPv4 and IPv6.
    pub system_dns: Vec<String>,
    pub domain: Option<String>,
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

/// Reads the current network from the system, see `plattform`.
pub fn collect() -> Result<NetInfo, String> {
    crate::plattform::lies_netzdaten()
}

pub fn run(args: &[String], explain_on: bool) -> Result<(), String> {
    crate::optionen::nur(args, &[], "hier")?;
    let a = crate::netze::aktuell()?;
    crate::netze::hier_hinweis(&a, explain_on);
    let n = a.info;
    let vpn = ["utun", "tun", "wg", "tailscale"]
        .iter()
        .any(|p| n.interface.starts_with(p));
    println!("Schnittstelle: {}", n.interface);
    explain(
        explain_on,
        &format!(
            "Die Default-Route sagt, über welche Schnittstelle Pakete ins Internet gehen. {}",
            if vpn {
                "Diese Schnittstelle ist ein Tunnel, also ein VPN wie Tailscale oder WireGuard: Dein Verkehr geht gerade durch den Tunnel, nicht direkt ins Netz."
            } else {
                "Beim Mac ist „en0“ meistens das WLAN, andere „en“-Nummern sind Kabel oder Adapter. Unter Linux steht „eth“ oder „enp“ für Kabel, „wlan“ oder „wlp“ für WLAN."
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
                "Ohne DHCP-Antwort (z. B. bei einem VPN-Tunnel) kennt der Rechner die Maske nicht, deshalb lässt sich das Subnetz nicht ausrechnen.",
            );
        }
    }
    println!("Router:        {}", show(n.router));
    explain(
        explain_on,
        "Der Router (Gateway) ist die Tür aus dem eigenen Subnetz. Alles, was nicht im Subnetz liegt, schickt der Rechner an ihn.",
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
            "DNS übersetzt Namen in IP-Adressen. „System“ ist, was der Rechner wirklich fragt, „DHCP“ ist, was das Netz angeboten hat. Beides sollte meist übereinstimmen."
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
