//! Menu bar guard, see issue #9: shows the state of the current network, re-checks after a network
//! change and posts a macOS notification when something is off. Closing the window keeps it running.

use netzinfo::netze;
use netzinfo::pruefen::{self, Klasse, Unterschied};
use std::sync::Mutex;
use std::time::Duration;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{TrayIcon, TrayIconBuilder};
use tauri::{App, AppHandle, Manager, Wry};

const KENNUNG: &str = "dev.mia-gruenwald.netzinfo";
const INTERVALL: Duration = Duration::from_secs(20);
/// Time for DHCP to hand out the address after the network changed.
const DHCP_WARTEN: Duration = Duration::from_secs(5);

/// One check at a time: `pruefen()` writes `pruefungen.json`.
static LAEUFT: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, Copy, PartialEq)]
enum Zustand {
    Unbekannt,
    KeinNetz,
    Unauffaellig,
    Auffaellig,
    Alarm,
}

impl Zustand {
    fn symbol(self) -> &'static str {
        match self {
            Self::Unbekannt => "?",
            Self::KeinNetz => "○",
            Self::Unauffaellig => "✓",
            Self::Auffaellig => "!",
            Self::Alarm => "‼",
        }
    }

    fn text(self) -> &'static str {
        match self {
            Self::Unbekannt => "unbekanntes Netz",
            Self::KeinNetz => "kein Netz",
            Self::Unauffaellig => "bekannt und unauffällig",
            Self::Auffaellig => "auffällig",
            Self::Alarm => "Alarm",
        }
    }
}

fn zustand(u: &[Unterschied]) -> Zustand {
    match u.iter().map(|u| u.klasse).max() {
        Some(Klasse::Alarm) => Zustand::Alarm,
        Some(Klasse::Auffaellig) => Zustand::Auffaellig,
        _ => Zustand::Unauffaellig,
    }
}

/// Title and text of the notification, `None` when nothing is off. Plain text only: the text
/// comes from the network and goes to `osascript` as an argument, never into script code.
fn mitteilung(netz: &str, u: &[Unterschied]) -> Option<(String, String)> {
    let z = zustand(u);
    let erster = u.iter().find(|u| u.klasse >= Klasse::Auffaellig)?;
    Some((
        format!("Netzinfo: {netz} ist {}", z.text()),
        erster.text.clone(),
    ))
}

fn mitteilen(titel: &str, text: &str) {
    #[cfg(target_os = "macos")]
    {
        let r = std::process::Command::new("osascript")
            .args(["-e", "on run argv"])
            .args([
                "-e",
                "display notification (item 1 of argv) with title (item 2 of argv)",
            ])
            .args(["-e", "end run", "--", text, titel])
            .status();
        if !matches!(r, Ok(s) if s.success()) {
            eprintln!(
                "Die Mitteilung ließ sich nicht anzeigen. Erlaube sie unter Systemeinstellungen > Mitteilungen."
            );
        }
    }
    #[cfg(not(target_os = "macos"))]
    eprintln!("{titel}: {text}");
}

struct Menue {
    tray: TrayIcon,
    netz: MenuItem<Wry>,
    zuletzt: MenuItem<Wry>,
}

fn anzeigen(app: &AppHandle, z: Zustand, netz: &str, zuletzt: &str) {
    let m = app.state::<Menue>();
    // Cosmetic updates: a failure here must not stop the guard.
    let _ = m.tray.set_title(Some(z.symbol()));
    let _ = m.tray.set_tooltip(Some(format!("Netzinfo: {}", z.text())));
    let _ = m.netz.set_text(format!("{netz}: {}", z.text()));
    let _ = m.zuletzt.set_text(zuletzt);
}

fn schluessel() -> Option<String> {
    let info = netzinfo::hier::collect().ok()?;
    Some(netze::fingerprint(&info).ok()?.key())
}

/// Checks the current network now and updates the icon and menu.
fn pruefe(app: &AppHandle) {
    let Ok(_sperre) = LAEUFT.try_lock() else {
        return;
    };
    if schluessel().is_none() {
        return anzeigen(app, Zustand::KeinNetz, "Kein Netz", "Nicht geprüft");
    }
    match pruefen::pruefen() {
        Ok(e) => {
            let z = zustand(&e.unterschiede);
            anzeigen(
                app,
                z,
                &e.netz,
                &format!("Geprüft: {}", netze::format_utc(e.messung.time)),
            );
            if let Some((titel, text)) = mitteilung(&e.netz, &e.unterschiede) {
                mitteilen(&titel, &text);
            }
        }
        Err(f) if f == netze::NICHT_GEMERKT_PRUEFEN => {
            anzeigen(
                app,
                Zustand::Unbekannt,
                "Neues Netz",
                "Nicht gemerkt, nicht geprüft",
            );
        }
        Err(f) => {
            eprintln!("{f}");
            anzeigen(app, Zustand::Unbekannt, "Netz", "Prüfung fehlgeschlagen");
        }
    }
}

fn fenster_oeffnen(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

/// Builds the tray icon and starts the guard thread. Called once from `setup`.
pub fn starten(app: &mut App) -> tauri::Result<()> {
    let netz = MenuItem::with_id(app, "netz", "Kein Netz", false, None::<&str>)?;
    let zuletzt = MenuItem::with_id(app, "zuletzt", "Noch nicht geprüft", false, None::<&str>)?;
    let jetzt = MenuItem::with_id(app, "jetzt", "Jetzt prüfen", true, None::<&str>)?;
    let fenster = MenuItem::with_id(app, "fenster", "Fenster öffnen", true, None::<&str>)?;
    let beenden = MenuItem::with_id(app, "beenden", "Beenden", true, None::<&str>)?;
    let menue = Menu::with_items(
        app,
        &[
            &netz,
            &zuletzt,
            &PredefinedMenuItem::separator(app)?,
            &jetzt,
            &fenster,
            &beenden,
        ],
    )?;
    let mut tray = TrayIconBuilder::with_id("wache")
        .menu(&menue)
        .title(Zustand::Unbekannt.symbol())
        .on_menu_event(|app, ev| match ev.id().as_ref() {
            "jetzt" => {
                let app = app.clone();
                std::thread::spawn(move || pruefe(&app));
            }
            "fenster" => fenster_oeffnen(app),
            "beenden" => app.exit(0),
            _ => {}
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    let tray = tray.build(app)?;
    app.manage(Menue {
        tray,
        netz,
        zuletzt,
    });

    let app = app.handle().clone();
    std::thread::spawn(move || {
        // `None`: nothing seen yet, so the first round checks too.
        let mut letzter: Option<Option<String>> = None;
        loop {
            let jetzt = schluessel();
            if letzter.as_ref() != Some(&jetzt) {
                if jetzt.is_some() {
                    std::thread::sleep(DHCP_WARTEN);
                }
                pruefe(&app);
                letzter = Some(schluessel());
            }
            std::thread::sleep(INTERVALL);
        }
    });
    Ok(())
}

fn xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn plist(exe: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\">\n<dict>\n  <key>Label</key><string>{KENNUNG}</string>\n  <key>ProgramArguments</key><array><string>{}</string></array>\n  <key>RunAtLoad</key><true/>\n</dict>\n</plist>\n",
        xml(exe)
    )
}

fn autostart_pfad() -> Result<std::path::PathBuf, String> {
    let home = std::env::var_os("HOME").ok_or("Der Ordner des Benutzers ist unbekannt (HOME fehlt). Starte die App aus dem Programme-Ordner.")?;
    Ok(std::path::Path::new(&home).join(format!("Library/LaunchAgents/{KENNUNG}.plist")))
}

pub fn autostart_lesen() -> Result<bool, String> {
    Ok(autostart_pfad()?.exists())
}

/// Writes or removes the login item. Takes effect at the next login.
pub fn autostart_setzen(an: bool) -> Result<bool, String> {
    let pfad = autostart_pfad()?;
    if an {
        let exe = std::env::current_exe()
            .map_err(|e| format!("Der Pfad der App ließ sich nicht lesen ({e}). Starte die App aus dem Programme-Ordner."))?;
        let ordner = pfad
            .parent()
            .ok_or("Der Ordner für Anmeldeobjekte fehlt.")?;
        std::fs::create_dir_all(ordner)
            .and_then(|()| std::fs::write(&pfad, plist(&exe.to_string_lossy())))
            .map_err(|e| format!("Der Start mit der Anmeldung ließ sich nicht einrichten ({e}). Prüfe die Rechte auf {}.", ordner.display()))?;
    } else if let Err(e) = std::fs::remove_file(&pfad)
        && e.kind() != std::io::ErrorKind::NotFound
    {
        return Err(format!(
            "Der Start mit der Anmeldung ließ sich nicht abschalten ({e}). Lösche {} von Hand.",
            pfad.display()
        ));
    }
    Ok(an)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u(klasse: Klasse, text: &str) -> Unterschied {
        Unterschied {
            klasse,
            text: text.into(),
        }
    }

    #[test]
    fn zustand_nimmt_das_schlimmste() {
        assert_eq!(zustand(&[]), Zustand::Unauffaellig);
        assert_eq!(zustand(&[u(Klasse::Harmlos, "a")]), Zustand::Unauffaellig);
        let beide = [u(Klasse::Auffaellig, "a"), u(Klasse::Alarm, "b")];
        assert_eq!(zustand(&beide), Zustand::Alarm);
    }

    #[test]
    fn mitteilung_nur_bei_auffaelligem() {
        assert_eq!(mitteilung("WG", &[u(Klasse::Harmlos, "x")]), None);
        let (t, b) = mitteilung(
            "WG",
            &[u(Klasse::Harmlos, "x"), u(Klasse::Auffaellig, "DNS neu")],
        )
        .unwrap();
        assert_eq!(
            (t.as_str(), b.as_str()),
            ("Netzinfo: WG ist auffällig", "DNS neu")
        );
    }

    #[test]
    fn plist_maskiert_den_pfad() {
        let p = plist("/Apps/A&B <x>/Netzinfo");
        assert!(p.contains("<string>/Apps/A&amp;B &lt;x&gt;/Netzinfo</string>"));
        assert!(p.contains(KENNUNG) && p.contains("RunAtLoad"));
    }
}
