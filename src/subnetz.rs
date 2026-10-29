//! Pure IPv4 subnet maths. No I/O, so everything here is unit-tested.

use std::net::Ipv4Addr;

#[derive(Debug, PartialEq, Eq, serde::Serialize)]
pub struct Subnet {
    pub ip: Ipv4Addr,
    pub prefix: u8,
    pub mask: Ipv4Addr,
    pub network: Ipv4Addr,
    pub broadcast: Ipv4Addr,
    /// First/last usable host. Equal to network/broadcast for /31 and /32.
    pub first: Ipv4Addr,
    pub last: Ipv4Addr,
    pub hosts: u64,
}

pub fn mask_from_prefix(prefix: u8) -> u32 {
    u32::MAX.checked_shl(32 - u32::from(prefix)).unwrap_or(0)
}

/// Dotted mask to prefix length; rejects masks with holes like 255.0.255.0.
pub fn prefix_from_mask(mask: Ipv4Addr) -> Result<u8, String> {
    let m = u32::from(mask);
    if m.leading_ones() + m.trailing_zeros() == 32 || m == 0 {
        Ok(m.leading_ones() as u8)
    } else {
        Err(format!(
            "„{mask}“ ist keine gültige Subnetzmaske, denn die Einsen müssen am Stück links stehen."
        ))
    }
}

pub fn calc(ip: Ipv4Addr, prefix: u8) -> Result<Subnet, String> {
    if prefix > 32 {
        return Err(format!(
            "Das Präfix /{prefix} gibt es nicht, erlaubt ist 0 bis 32."
        ));
    }
    let m = mask_from_prefix(prefix);
    let net = u32::from(ip) & m;
    let bc = net | !m;
    let (first, last, hosts) = match prefix {
        32 => (net, net, 1),
        31 => (net, bc, 2), // RFC 3021 point-to-point link: no network/broadcast address
        _ => (net + 1, bc - 1, u64::from(bc - net) - 1),
    };
    Ok(Subnet {
        ip,
        prefix,
        mask: Ipv4Addr::from(m),
        network: Ipv4Addr::from(net),
        broadcast: Ipv4Addr::from(bc),
        first: Ipv4Addr::from(first),
        last: Ipv4Addr::from(last),
        hosts,
    })
}

/// Accepts `["172.16.0.0/16"]` or `["192.0.2.20", "255.255.255.0"]`.
pub fn parse_args(args: &[String]) -> Result<(Ipv4Addr, u8), String> {
    let usage =
        "Aufruf: netzinfo subnetz 172.16.0.0/16 oder netzinfo subnetz 192.0.2.20 255.255.255.0";
    let ip_of = |s: &str| {
        s.parse::<Ipv4Addr>()
            .map_err(|_| format!("„{s}“ ist keine gültige IPv4-Adresse. {usage}"))
    };
    match args {
        [one] => {
            let (ip, p) = one
                .split_once('/')
                .ok_or_else(|| format!("„{one}“ hat kein /Präfix. {usage}"))?;
            let prefix =
                p.parse::<u8>().ok().filter(|p| *p <= 32).ok_or_else(|| {
                    format!("„{p}“ ist kein gültiges Präfix, erlaubt ist 0 bis 32.")
                })?;
            Ok((ip_of(ip)?, prefix))
        }
        [ip, mask] => Ok((ip_of(ip)?, prefix_from_mask(ip_of(mask)?)?)),
        _ => Err(usage.into()),
    }
}

/// 32 bits as four dotted 8-bit groups, e.g. `11000000.10101000.10110010.00010100`.
pub fn binary(a: Ipv4Addr) -> String {
    let o = a.octets();
    format!("{:08b}.{:08b}.{:08b}.{:08b}", o[0], o[1], o[2], o[3])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(s: &str) -> Ipv4Addr {
        s.parse().unwrap()
    }
    fn args(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn slash24_from_host_ip() {
        let s = calc(ip("192.168.178.20"), 24).unwrap();
        assert_eq!(s.network, ip("192.168.178.0"));
        assert_eq!(s.broadcast, ip("192.168.178.255"));
        assert_eq!(s.first, ip("192.168.178.1"));
        assert_eq!(s.last, ip("192.168.178.254"));
        assert_eq!(s.hosts, 254);
        assert_eq!(s.mask, ip("255.255.255.0"));
    }

    #[test]
    fn slash16() {
        let s = calc(ip("172.16.0.0"), 16).unwrap();
        assert_eq!(s.broadcast, ip("172.16.255.255"));
        assert_eq!(s.hosts, 65534);
    }

    #[test]
    fn slash0_covers_everything() {
        let s = calc(ip("10.0.0.1"), 0).unwrap();
        assert_eq!(s.network, ip("0.0.0.0"));
        assert_eq!(s.broadcast, ip("255.255.255.255"));
        assert_eq!(s.hosts, 4_294_967_294);
        assert_eq!(s.mask, ip("0.0.0.0"));
    }

    #[test]
    fn slash31_has_two_hosts() {
        let s = calc(ip("10.0.0.5"), 31).unwrap();
        assert_eq!(
            (s.first, s.last, s.hosts),
            (ip("10.0.0.4"), ip("10.0.0.5"), 2)
        );
    }

    #[test]
    fn slash32_has_one_host() {
        let s = calc(ip("203.0.113.7"), 32).unwrap();
        assert_eq!(
            (s.first, s.last, s.hosts),
            (ip("203.0.113.7"), ip("203.0.113.7"), 1)
        );
        assert_eq!(s.network, s.broadcast);
    }

    #[test]
    fn slash30() {
        let s = calc(ip("10.0.0.6"), 30).unwrap();
        assert_eq!(
            (s.first, s.last, s.hosts),
            (ip("10.0.0.5"), ip("10.0.0.6"), 2)
        );
    }

    #[test]
    fn prefix_over_32_is_error() {
        assert!(calc(ip("10.0.0.1"), 33).is_err());
    }

    #[test]
    fn mask_conversion() {
        assert_eq!(prefix_from_mask(ip("255.255.255.0")), Ok(24));
        assert_eq!(prefix_from_mask(ip("255.255.255.255")), Ok(32));
        assert_eq!(prefix_from_mask(ip("0.0.0.0")), Ok(0));
        assert_eq!(prefix_from_mask(ip("255.255.255.252")), Ok(30));
        assert!(prefix_from_mask(ip("255.0.255.0")).is_err());
        assert!(prefix_from_mask(ip("0.255.255.255")).is_err());
    }

    #[test]
    fn parse_cidr_and_mask_forms() {
        assert_eq!(
            parse_args(&args(&["172.16.0.0/16"])),
            Ok((ip("172.16.0.0"), 16))
        );
        assert_eq!(
            parse_args(&args(&["192.0.2.20", "255.255.255.0"])),
            Ok((ip("192.0.2.20"), 24))
        );
    }

    #[test]
    fn parse_rejects_garbage() {
        for bad in [
            &[][..],
            &["10.0.0.1"],
            &["10.0.0.1/33"],
            &["10.0.0.1/-1"],
            &["10.0.0.1/"],
            &["10.0.0.256/24"],
            &["hallo/24"],
            &["10.0.0.1", "255.0.255.0"],
            &["10.0.0.1", "x"],
            &["10.0.0.1", "255.255.255.0", "x"],
        ] {
            assert!(parse_args(&args(bad)).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn binary_groups() {
        assert_eq!(
            binary(ip("192.168.178.20")),
            "11000000.10101000.10110010.00010100"
        );
    }
}
