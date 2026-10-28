//! `netz merken`, `netz netze`, `netz vergessen` and the recognition in `netz hier` (issue #5).
//!
//! The store functions take the file path as a parameter; only `data_path` reads `NETZ_DATEN`.

use crate::explain;
use crate::hier::{self, NetInfo};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::net::Ipv4Addr;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// What identifies a network: the DHCP data the router hands out.
// ponytail: two networks with the same factory router (two FRITZ!Boxes on 192.168.178.1 with
// domain fritz.box) look identical. Upgrade path: also compare the DNS/TLS results from #6
// or the router's HTTPS certificate.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Fingerprint {
    pub router: Ipv4Addr,
    pub dhcp_server: Ipv4Addr,
    pub domain: Option<String>,
    /// Network address and prefix, e.g. "192.168.178.0/24".
    pub subnet: String,
}

impl Fingerprint {
    /// Stable string key for per-network data in other files (survives renaming a network).
    pub fn key(&self) -> String {
        let domain = self.domain.as_deref().unwrap_or("");
        format!(
            "{}|{}|{}|{}",
            self.router, self.dhcp_server, domain, self.subnet
        )
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Netz {
    pub name: String,
    pub fingerprint: Fingerprint,
    pub meins: bool,
    /// Unix seconds.
    pub first_seen: u64,
    /// Unix seconds.
    pub last_seen: u64,
}

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Fingerprint of the current network, or an error saying why there is none.
pub fn fingerprint(n: &NetInfo) -> Result<Fingerprint, String> {
    match (&n.router, &n.dhcp_server, &n.subnet) {
        (Some(router), Some(dhcp_server), Some(s)) => Ok(Fingerprint {
            router: *router,
            dhcp_server: *dhcp_server,
            domain: n.domain.clone(),
            subnet: format!("{}/{}", s.network, s.prefix),
        }),
        _ => Err("Dieses Netz lässt sich nicht wiedererkennen: Ohne DHCP-Antwort (z. B. bei einem VPN-Tunnel) gibt es keinen Fingerabdruck. Verbinde dich direkt mit dem WLAN oder Kabel und versuche es noch mal.".into()),
    }
}

/// `NETZ_DATEN` or `~/Library/Application Support/netz/netze.json`.
pub fn data_path() -> Result<PathBuf, String> {
    if let Some(p) = std::env::var_os("NETZ_DATEN") {
        return Ok(PathBuf::from(p));
    }
    let home = std::env::var_os("HOME")
        .ok_or("Der Ordner für gemerkte Netze ist unbekannt (HOME fehlt). Setze `NETZ_DATEN` auf eine Datei.")?;
    Ok(PathBuf::from(home).join("Library/Application Support/netz/netze.json"))
}

/// Reads the store. A missing file is an empty list. A file that cannot be read or parsed is
/// moved to `netze.json.kaputt-<secs>` and never overwritten; if that fails, nothing continues.
pub fn load(path: &Path) -> Result<Vec<Netz>, String> {
    load_json(path)
}

/// Same rules as `load` for any JSON file of netz (e.g. `data_path()?.with_file_name("x.json")`).
/// A missing file gives `T::default()`.
pub fn load_json<T: DeserializeOwned + Default>(path: &Path) -> Result<T, String> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(T::default()),
        Err(e) => return set_aside(path, &format!("nicht lesbar ({e})")),
    };
    match serde_json::from_str(&text) {
        Ok(data) => Ok(data),
        Err(e) => set_aside(path, &format!("kaputt ({e})")),
    }
}

fn set_aside<T: Default>(path: &Path, why: &str) -> Result<T, String> {
    let mut name = path.as_os_str().to_owned();
    name.push(format!(".kaputt-{}", now()));
    let aside = PathBuf::from(name);
    std::fs::rename(path, &aside).map_err(|e| {
        format!(
            "Die Datei {} ist {why}, und ich konnte sie nicht beiseitelegen ({e}). Es wurde nichts geschrieben. Prüfe die Rechte oder lege die Datei von Hand weg.",
            path.display()
        )
    })?;
    eprintln!(
        "Hinweis: Die Datei {} war {why}. Ich habe sie als {} beiseitegelegt und fange leer an.",
        path.display(),
        aside.display()
    );
    Ok(T::default())
}

/// Writes a temp file next to `path`, then renames it over `path`.
pub fn save<T: Serialize + ?Sized>(path: &Path, list: &T) -> Result<(), String> {
    let err = |e: std::io::Error| {
        format!(
            "Die Datei {} ließ sich nicht speichern ({e}). Prüfe den Ordner und versuche es noch mal.",
            path.display()
        )
    };
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(err)?;
    }
    let json = serde_json::to_string_pretty(list)
        .map_err(|e| format!("Die gemerkten Netze ließen sich nicht umwandeln ({e})."))?;
    let mut name = path.as_os_str().to_owned();
    name.push(".tmp");
    let tmp = PathBuf::from(name);
    let write = || -> std::io::Result<()> {
        let f = std::fs::File::create(&tmp)?;
        std::io::Write::write_all(&mut &f, json.as_bytes())?;
        f.sync_all()?;
        std::fs::rename(&tmp, path)
    };
    write().map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        err(e)
    })
}

pub fn find(list: &[Netz], fp: &Fingerprint) -> Option<usize> {
    list.iter().position(|n| &n.fingerprint == fp)
}

/// Stores the network under `name`. A known fingerprint is renamed/updated, not duplicated.
/// `meins` replaces the old marking, so `merken <Name>` without `--meins` takes it back.
pub fn remember(
    list: &mut Vec<Netz>,
    name: &str,
    fp: Fingerprint,
    meins: bool,
    now: u64,
) -> Result<(), String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Der Name ist leer. Beispiel: `netz merken WG`.".into());
    }
    let known = find(list, &fp);
    if let Some(other) = list.iter().position(|n| n.name == name)
        && Some(other) != known
    {
        return Err(format!(
            "Der Name „{name}“ ist schon für ein anderes Netz vergeben. Nimm einen anderen Namen oder `netz vergessen {name}`."
        ));
    }
    match known {
        Some(i) => {
            let n = &mut list[i];
            n.name = name.to_string();
            n.meins = meins;
            n.last_seen = now;
        }
        None => list.push(Netz {
            name: name.to_string(),
            fingerprint: fp,
            meins,
            first_seen: now,
            last_seen: now,
        }),
    }
    Ok(())
}

pub fn forget(list: &mut Vec<Netz>, name: &str) -> Result<(), String> {
    let i = list.iter().position(|n| n.name == name).ok_or_else(|| {
        format!("Ein Netz „{name}“ ist nicht gemerkt. `netz netze` zeigt die Liste.")
    })?;
    list.remove(i);
    Ok(())
}

/// True if the current network is stored with `meins: true`. Any problem counts as false.
pub fn current_is_meins() -> bool {
    let known = || -> Result<bool, String> {
        let fp = fingerprint(&hier::collect()?)?;
        let list = load(&data_path()?)?;
        Ok(find(&list, &fp).is_some_and(|i| list[i].meins))
    };
    known().unwrap_or(false)
}

/// "Mo 26.10.2026, 21:30 (UTC)" from Unix seconds, std only.
// ponytail: UTC, not local time. Upgrade path: libc localtime_r if the offset matters.
pub fn format_utc(secs: u64) -> String {
    let days = (secs / 86400) as i64;
    let rest = secs % 86400;
    let wd = ["Mo", "Di", "Mi", "Do", "Fr", "Sa", "So"][((days + 3) % 7) as usize];
    // days since 1970-01-01 to y-m-d (Howard Hinnant's civil_from_days)
    let z = days + 719_468;
    let era = z / 146_097;
    let doe = z % 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{wd} {d:02}.{m:02}.{y}, {:02}:{:02} (UTC)",
        rest / 3600,
        rest % 3600 / 60
    )
}

/// First line of `netz hier`. Never fails: store problems become a note.
pub fn hier_hinweis(n: &NetInfo, explain_on: bool) {
    let fp = match fingerprint(n) {
        Ok(fp) => fp,
        Err(e) => {
            println!("Netz nicht wiedererkennbar: {e}");
            return;
        }
    };
    let path = match data_path() {
        Ok(p) => p,
        Err(e) => return println!("Hinweis: {e}"),
    };
    let mut list = match load(&path) {
        Ok(l) => l,
        Err(e) => return println!("Hinweis: {e}"),
    };
    match find(&list, &fp) {
        Some(i) => {
            println!(
                "Bekanntes Netz: {}, zuletzt am {}",
                list[i].name,
                format_utc(list[i].last_seen)
            );
            list[i].last_seen = now();
            if let Err(e) = save(&path, &list) {
                println!("Hinweis: {e}");
            }
        }
        None => println!("Neues Netz. Mit `netz merken <Name>` merkst du es dir."),
    }
    explain(
        explain_on,
        "Ein Fingerabdruck besteht aus Router-IP, DHCP-Server, Domain und Subnetz. Diese Werte hat der DHCP-Server des Netzes vergeben, sie bleiben bei jedem Besuch gleich und unterscheiden sich meist von Netz zu Netz. So erkenne ich ein Netz wieder, ohne WLAN-Name oder MAC-Adresse zu brauchen.",
    );
}

pub fn run_merken(args: &[String], explain_on: bool) -> Result<(), String> {
    let meins = args.iter().any(|a| a == "--meins");
    let mut names = args.iter().filter(|a| *a != "--meins");
    let (Some(name), None) = (names.next(), names.next()) else {
        return Err("Es passt genau ein Name, z. B. `netz merken WG --meins`.".into());
    };
    if name.starts_with("--") {
        return Err(format!(
            "Unbekannte Option „{name}“. `netz hilfe` zeigt alle Optionen."
        ));
    }
    let fp = fingerprint(&hier::collect()?)?;
    explain(
        explain_on,
        "Ich lese Router, DHCP-Server, Domain und Subnetz des aktuellen Netzes. Zusammen sind sie der Fingerabdruck, unter dem das Netz gespeichert wird.",
    );
    let path = data_path()?;
    let mut list = load(&path)?;
    remember(&mut list, name, fp, meins, now())?;
    save(&path, &list)?;
    println!(
        "Netz „{}“ gemerkt{}.",
        name.trim(),
        if meins { " (meins)" } else { "" }
    );
    explain(
        explain_on,
        "„meins“ erlaubt `netz ports` in diesem Netz, ohne dass du jedes Mal `--meins` tippst. Markiere nur Netze, die dir gehören.",
    );
    Ok(())
}

pub fn run_netze(_args: &[String], explain_on: bool) -> Result<(), String> {
    let list = load(&data_path()?)?;
    if list.is_empty() {
        println!("Noch kein Netz gemerkt. Mit `netz merken <Name>` fängst du an.");
        return Ok(());
    }
    for n in &list {
        println!(
            "{}: meins {}, zuletzt am {}, Router {}, Domain {}",
            n.name,
            if n.meins { "ja" } else { "nein" },
            format_utc(n.last_seen),
            n.fingerprint.router,
            n.fingerprint.domain.as_deref().unwrap_or("unbekannt"),
        );
    }
    explain(
        explain_on,
        "Die Liste liegt als JSON-Datei auf deinem Mac. Pfad ändern geht mit der Umgebungsvariable NETZ_DATEN.",
    );
    Ok(())
}

pub fn run_vergessen(args: &[String], explain_on: bool) -> Result<(), String> {
    let [name] = args else {
        return Err("Es passt genau ein Name, z. B. `netz vergessen WG`.".into());
    };
    let path = data_path()?;
    let mut list = load(&path)?;
    forget(&mut list, name)?;
    save(&path, &list)?;
    println!("Netz „{name}“ vergessen.");
    explain(
        explain_on,
        "Beim nächsten Besuch gilt das Netz wieder als neu, und die Markierung „meins“ ist weg.",
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::subnetz;

    fn info() -> NetInfo {
        NetInfo {
            interface: "en0".into(),
            ip: "192.168.178.20".parse().ok(),
            subnet: subnetz::calc("192.168.178.20".parse().unwrap(), 24).ok(),
            router: "192.168.178.1".parse().ok(),
            dhcp_server: "192.168.178.1".parse().ok(),
            domain: Some("fritz.box".into()),
            ..NetInfo::default()
        }
    }

    fn tmp_dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("netz-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn fingerprint_from_dhcp_data() {
        let fp = fingerprint(&info()).unwrap();
        assert_eq!(fp.router, "192.168.178.1".parse::<Ipv4Addr>().unwrap());
        assert_eq!(fp.subnet, "192.168.178.0/24");
        assert_eq!(fp.domain.as_deref(), Some("fritz.box"));
    }

    #[test]
    fn no_dhcp_means_no_fingerprint() {
        let vpn = NetInfo {
            interface: "utun4".into(),
            router: "100.64.0.1".parse().ok(),
            ..NetInfo::default()
        };
        assert!(fingerprint(&vpn).unwrap_err().contains("Fingerabdruck"));
    }

    #[test]
    fn json_round_trip_and_atomic_save() {
        let dir = tmp_dir("save");
        let path = dir.join("sub/netze.json");
        let mut list = Vec::new();
        remember(
            &mut list,
            "WG",
            fingerprint(&info()).unwrap(),
            true,
            1793050200,
        )
        .unwrap();
        save(&path, &list).unwrap();
        assert_eq!(load(&path).unwrap(), list);
        // no temp file left behind
        let names: Vec<_> = std::fs::read_dir(path.parent().unwrap())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, vec![std::ffi::OsString::from("netze.json")]);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn missing_file_is_empty_list() {
        let dir = tmp_dir("missing");
        assert!(load(&dir.join("netze.json")).unwrap().is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn corrupt_file_is_moved_aside_not_overwritten() {
        let dir = tmp_dir("corrupt");
        let path = dir.join("netze.json");
        std::fs::write(&path, "{ das ist kein json").unwrap();
        assert!(load(&path).unwrap().is_empty());
        assert!(!path.exists());
        let aside: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        assert_eq!(aside.len(), 1);
        assert!(aside[0].to_string_lossy().contains("netze.json.kaputt-"));
        assert_eq!(
            std::fs::read_to_string(&aside[0]).unwrap(),
            "{ das ist kein json"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn corrupt_file_that_cannot_be_moved_is_an_error() {
        // a read-only directory makes the rename impossible
        use std::os::unix::fs::PermissionsExt;
        let dir = tmp_dir("ro");
        let path = dir.join("netze.json");
        std::fs::write(&path, "kaputt").unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o500)).unwrap();
        let r = load(&path);
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        if r.is_ok() {
            // running as root ignores permissions; nothing to assert then
            return;
        }
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "kaputt");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn names_are_unique_and_non_empty() {
        let mut list = Vec::new();
        let fp = fingerprint(&info()).unwrap();
        assert!(remember(&mut list, "  ", fp.clone(), false, 1).is_err());
        remember(&mut list, "WG", fp.clone(), false, 1).unwrap();
        let mut other = fp.clone();
        other.subnet = "10.0.0.0/24".into();
        assert!(remember(&mut list, "WG", other.clone(), false, 2).is_err());
        remember(&mut list, "Arbeit", other, false, 2).unwrap();
        // same fingerprint again: renamed and updated, not duplicated
        remember(&mut list, "WG neu", fp, true, 3).unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!((list[0].name.as_str(), list[0].meins), ("WG neu", true));
        assert_eq!((list[0].first_seen, list[0].last_seen), (1, 3));
        // renaming onto an existing name of another network is refused
        let fp0 = list[0].fingerprint.clone();
        assert!(remember(&mut list, "Arbeit", fp0, true, 4).is_err());
    }

    #[test]
    fn forget_removes_one() {
        let mut list = Vec::new();
        remember(&mut list, "WG", fingerprint(&info()).unwrap(), false, 1).unwrap();
        assert!(forget(&mut list, "Schule").is_err());
        forget(&mut list, "WG").unwrap();
        assert!(list.is_empty());
    }

    #[test]
    fn utc_date_format() {
        assert_eq!(format_utc(1793050200), "Mo 26.10.2026, 21:30 (UTC)");
        assert_eq!(format_utc(0), "Do 01.01.1970, 00:00 (UTC)");
        assert_eq!(format_utc(951782405), "Di 29.02.2000, 00:00 (UTC)");
    }
}
