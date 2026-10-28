//! netz als Mac-App: dünne Tauri-Hülle um die Bibliothek `netz`.
//!
//! Jeder Befehl gibt Daten oder einen deutschen Satz zurück; die Oberfläche (app/ui) zeigt den Satz
//! dort an, wo es passiert ist.

use netz::hier::{self, NetInfo};
use netz::netze::{self, Fingerprint, Netz};
use serde::Serialize;
use std::path::Path;

/// Was „Dieses Netz“ anzeigt.
#[derive(Debug, Serialize)]
struct DiesesNetz {
    info: NetInfo,
    /// Der gemerkte Eintrag, mit `last_seen` vom Besuch davor.
    netz: Option<Netz>,
    /// Warum sich das Netz nicht wiedererkennen lässt (z. B. VPN ohne DHCP).
    nicht_wiedererkennbar: Option<String>,
    /// Probleme mit der Liste, die die Anzeige nicht verhindern.
    hinweis: Option<String>,
}

/// Sucht das Netz in der Liste und setzt `last_seen` auf jetzt. Gibt den Eintrag von vorher zurück.
fn gesehen(
    path: &Path,
    fp: &Fingerprint,
    jetzt: u64,
) -> Result<(Option<Netz>, Option<String>), String> {
    let mut list = netze::load(path)?;
    let Some(i) = netze::find(&list, fp) else {
        return Ok((None, None));
    };
    let vorher = list[i].clone();
    list[i].last_seen = jetzt;
    Ok((Some(vorher), netze::save(path, &list).err()))
}

#[tauri::command(async)]
fn dieses_netz() -> Result<DiesesNetz, String> {
    let info = hier::collect()?;
    let mut out = DiesesNetz {
        netz: None,
        nicht_wiedererkennbar: None,
        hinweis: None,
        info,
    };
    match netze::fingerprint(&out.info) {
        Err(e) => out.nicht_wiedererkennbar = Some(e),
        Ok(fp) => match netze::data_path().and_then(|p| gesehen(&p, &fp, netze::now())) {
            Ok((netz, hinweis)) => (out.netz, out.hinweis) = (netz, hinweis),
            Err(e) => out.hinweis = Some(e),
        },
    }
    Ok(out)
}

// ponytail: kein Sperren der Datei; App und Kommandozeile gleichzeitig schreiben ist selten.
// Upgrade: Lock-Datei neben netze.json, sobald #19 beide über dieselbe Funktion laufen lässt.
#[tauri::command(async)]
fn netz_merken(name: String, meins: bool) -> Result<(), String> {
    let fp = netze::fingerprint(&hier::collect()?)?;
    let path = netze::data_path()?;
    let mut list = netze::load(&path)?;
    netze::remember(&mut list, &name, fp, meins, netze::now())?;
    netze::save(&path, &list)
}

#[tauri::command(async)]
fn gemerkte_netze() -> Result<Vec<Netz>, String> {
    netze::load(&netze::data_path()?)
}

#[tauri::command(async)]
fn netz_vergessen(name: String) -> Result<(), String> {
    let path = netze::data_path()?;
    let mut list = netze::load(&path)?;
    netze::forget(&mut list, &name)?;
    netze::save(&path, &list)
}

fn main() {
    let r = tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            dieses_netz,
            netz_merken,
            gemerkte_netze,
            netz_vergessen
        ])
        .run(tauri::generate_context!());
    if let Err(e) = r {
        eprintln!("Die App ließ sich nicht starten: {e}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gesehen_gibt_vorher_zurueck_und_schreibt_jetzt() {
        let dir = std::env::temp_dir().join(format!("netz-app-test-{}", std::process::id()));
        let path = dir.join("netze.json");
        let fp = Fingerprint {
            router: "192.168.178.1".parse().unwrap(),
            dhcp_server: "192.168.178.1".parse().unwrap(),
            domain: Some("fritz.box".into()),
            subnet: "192.168.178.0/24".into(),
        };
        assert_eq!(gesehen(&path, &fp, 5).unwrap().0, None);
        let mut list = Vec::new();
        netze::remember(&mut list, "WG", fp.clone(), true, 1).unwrap();
        netze::save(&path, &list).unwrap();
        let (vorher, hinweis) = gesehen(&path, &fp, 9).unwrap();
        assert_eq!(vorher.map(|n| n.last_seen), Some(1));
        assert_eq!(hinweis, None);
        assert_eq!(netze::load(&path).unwrap()[0].last_seen, 9);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
