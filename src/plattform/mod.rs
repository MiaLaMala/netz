//! Systemabhängiges Auslesen von Netzdaten. `cfg(target_os)` steht nur hier.
//! Jedes System liefert dieselben Funktionen:
//! - `lies_netzdaten() -> Result<NetInfo, String>`
//! - `system_dns() -> Vec<String>` (Server, die das System zuerst fragt)
//! - `wpad_dhcp(schnittstelle) -> Option<String>`

// Die Parser in macos.rs und linux.rs sind reiner Text und laufen auf jedem System in den Tests.
mod linux;
mod macos;

#[cfg(target_os = "linux")]
pub use linux::{lies_netzdaten, system_dns, wpad_dhcp};
#[cfg(target_os = "macos")]
pub use macos::{lies_netzdaten, system_dns, wpad_dhcp};

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub use anderes::*;

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
mod anderes {
    use crate::hier::NetInfo;

    pub fn lies_netzdaten() -> Result<NetInfo, String> {
        Err("Dieses System kann Netzinfo noch nicht auslesen. Unter macOS geht es schon.".into())
    }

    pub fn system_dns() -> Vec<String> {
        Vec::new()
    }

    pub fn wpad_dhcp(_schnittstelle: &str) -> Option<String> {
        None
    }
}
