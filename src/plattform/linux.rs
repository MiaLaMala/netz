//! Linux: heute nur die DNS-Server aus `/etc/resolv.conf`.

#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

use crate::hier::NetInfo;

pub fn lies_netzdaten() -> Result<NetInfo, String> {
    Err("Dieses System kann Netzinfo noch nicht auslesen. Unter macOS geht es schon; unter Linux liest Netzinfo bisher nur die DNS-Server.".into())
}

pub fn system_dns() -> Vec<String> {
    std::fs::read_to_string("/etc/resolv.conf")
        .map(|t| nameserver(&t))
        .unwrap_or_default()
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

    #[test]
    fn nameserver_aus_resolv_conf() {
        assert_eq!(
            nameserver("# x\nnameserver 192.0.2.53\nsearch example.net\nnameserver 2001:db8::53\n"),
            ["192.0.2.53", "2001:db8::53"]
        );
        assert!(nameserver("nothing").is_empty());
    }
}
