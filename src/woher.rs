//! Wer hat dieses Programm gestartet: Prozesskette bis zum Auslöser. Issue #56.

use serde::Serialize;
use std::collections::HashSet;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Prozess {
    pub pid: u32,
    pub ppid: u32,
    pub name: String,
    pub pfad: Option<String>,
    /// Start as Unix seconds.
    pub start: Option<u64>,
    pub nutzer: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub enum Quelle {
    Terminal,
    Systemdienst,
    Anmeldeobjekt,
    Unbekannt,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Herkunft {
    /// The program first, launchd (pid 1) last if the chain got there.
    pub kette: Vec<Prozess>,
    pub quelle: Quelle,
    /// Name of the launchd plist that starts the program, if found.
    pub eintrag: Option<String>,
    /// The chain stops before launchd: parent gone or not readable.
    pub abgeschnitten: bool,
    pub satz: String,
}

const SHELLS: [&str; 7] = ["zsh", "bash", "fish", "sh", "dash", "tcsh", "ksh"];
const SYSTEM: [&str; 4] = ["/System/", "/usr/libexec/", "/usr/sbin/", "/sbin/"];

/// Walks up from `pid` with `info`; stops at pid 1, at a loop, or where a parent is unreadable.
fn kette_aus(pid: u32, info: impl Fn(u32) -> Option<Prozess>) -> Vec<Prozess> {
    let (mut out, mut seen) = (Vec::new(), HashSet::new());
    let mut next = Some(pid);
    // 64 levels is far deeper than any real chain.
    while let Some(p) = next.filter(|p| out.len() < 64 && seen.insert(*p)) {
        let Some(pr) = info(p) else { break };
        next = (p != 1).then_some(pr.ppid);
        out.push(pr);
    }
    out
}

/// `find(path)` returns (is system-wide daemon, plist name) for a launchd entry that starts `path`.
fn einstufen(
    kette: &[Prozess],
    find: impl Fn(&str) -> Option<(bool, String)>,
) -> (Quelle, Option<String>) {
    let Some(oben) = kette.iter().rev().find(|p| p.pid != 1) else {
        return (Quelle::Unbekannt, None);
    };
    let unter_launchd = kette.last().is_some_and(|p| p.pid == 1);
    if unter_launchd && let Some((daemon, name)) = oben.pfad.as_deref().and_then(find) {
        let q = if daemon {
            Quelle::Systemdienst
        } else {
            Quelle::Anmeldeobjekt
        };
        return (q, Some(name));
    }
    if kette
        .iter()
        .skip(1)
        .any(|p| SHELLS.contains(&p.name.as_str()))
    {
        return (Quelle::Terminal, None);
    }
    let system = oben
        .pfad
        .as_deref()
        .is_some_and(|f| SYSTEM.iter().any(|s| f.starts_with(s)));
    if unter_launchd && system {
        return (Quelle::Systemdienst, None);
    }
    (Quelle::Unbekannt, None)
}

fn vor(sek: u64) -> String {
    let (n, e, m) = match sek {
        0..=89 => return "gerade eben".into(),
        90..=3569 => ((sek + 30) / 60, "Minute", "Minuten"),
        3570..=129_599 => ((sek + 1800) / 3600, "Stunde", "Stunden"),
        _ => ((sek + 43_200) / 86_400, "Tag", "Tagen"),
    };
    format!("vor {n} {}", if n == 1 { e } else { m })
}

fn satz(h: &Herkunft, jetzt: u64) -> String {
    let wann = h.kette[0]
        .start
        .map(|s| format!(", {}", vor(jetzt.saturating_sub(s))))
        .unwrap_or_default();
    let eintrag = h.eintrag.as_deref().unwrap_or("");
    match h.quelle {
        Quelle::Terminal => format!(
            "Dieses Programm wurde von deinem Terminal gestartet{wann}. Wenn du das Terminal schließt, endet es wahrscheinlich."
        ),
        Quelle::Anmeldeobjekt => format!(
            "Dieses Programm startet bei jeder Anmeldung von selbst, über einen Eintrag in deinen Anmeldeobjekten ({eintrag}). Gestartet {}. Die Zuordnung ist eine Näherung.",
            wann.trim_start_matches(", ")
        ),
        Quelle::Systemdienst => format!(
            "Dieses Programm ist ein Systemdienst, den macOS selbst über launchd startet{}{wann}. Du hast ihn nicht bewusst gestartet.",
            if eintrag.is_empty() {
                String::new()
            } else {
                format!(" (Eintrag {eintrag})")
            }
        ),
        Quelle::Unbekannt if h.abgeschnitten => "Die Kette bricht ab: Der Elternprozess ist beendet oder für dich nicht lesbar. Wer das Programm gestartet hat, lässt sich so nicht sagen.".into(),
        Quelle::Unbekannt => format!(
            "launchd hat das Programm gestartet{wann}, aber ich finde keinen Eintrag dafür. Wahrscheinlich ein App-Start per Doppelklick, oder der ursprüngliche Elternprozess ist beendet und launchd hat übernommen."
        ),
    }
}

fn jetzt() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// launchd plist (user, global, system daemon) whose text mentions `pfad`.
// ponytail: substring search in the plist bytes (works for XML and binary plists);
// a plist parser would be exact, add when false hits show up.
fn plist_suchen(pfad: &str) -> Option<(bool, String)> {
    let home = std::env::var("HOME").unwrap_or_default();
    let dirs = [
        (false, format!("{home}/Library/LaunchAgents")),
        (false, "/Library/LaunchAgents".to_string()),
        (true, "/Library/LaunchDaemons".to_string()),
    ];
    let nadel = pfad.as_bytes();
    for (daemon, dir) in dirs {
        let Ok(rd) = std::fs::read_dir(dir) else {
            continue;
        };
        for e in rd.flatten() {
            let Ok(inhalt) = std::fs::read(e.path()) else {
                continue;
            };
            if inhalt.windows(nadel.len()).any(|w| w == nadel) {
                return Some((daemon, e.file_name().to_string_lossy().into_owned()));
            }
        }
    }
    None
}

#[cfg(target_os = "macos")]
fn info(pid: u32) -> Option<Prozess> {
    use std::ffi::{CStr, c_int, c_void};
    // SAFETY: zeroed plain C struct; proc_pidinfo writes at most `size` bytes into it.
    let mut b: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_bsdinfo>() as c_int;
    let n = unsafe {
        libc::proc_pidinfo(
            pid as c_int,
            libc::PROC_PIDTBSDINFO,
            0,
            (&raw mut b).cast::<c_void>(),
            size,
        )
    };
    if n != size {
        // macOS may refuse to describe launchd itself; its role is fixed, so end the chain there.
        return (pid == 1).then(|| Prozess {
            pid,
            ppid: 0,
            name: "launchd".into(),
            pfad: Some("/sbin/launchd".into()),
            start: None,
            nutzer: "root".into(),
        });
    }
    let mut buf = [0u8; 4096];
    // SAFETY: proc_pidpath writes at most buf.len() bytes.
    let len =
        unsafe { libc::proc_pidpath(pid as c_int, buf.as_mut_ptr().cast(), buf.len() as u32) };
    let pfad = (len > 0).then(|| String::from_utf8_lossy(&buf[..len as usize]).into_owned());
    let name = match pfad.as_deref().and_then(|f| f.rsplit('/').next()) {
        Some(n) => n.to_string(),
        // SAFETY: the kernel NUL-terminates this fixed-size name field.
        None => unsafe { CStr::from_ptr(b.pbi_comm.as_ptr()) }
            .to_string_lossy()
            .into_owned(),
    };
    Some(Prozess {
        pid,
        ppid: b.pbi_ppid,
        name,
        pfad,
        start: Some(b.pbi_start_tvsec),
        nutzer: nutzername(b.pbi_uid),
    })
}

#[cfg(target_os = "macos")]
fn nutzername(uid: u32) -> String {
    // SAFETY: getpwuid_r fills `pw` and `buf`; the name is copied before both go away.
    unsafe {
        let (mut pw, mut res) = (std::mem::zeroed::<libc::passwd>(), std::ptr::null_mut());
        let mut buf = [0 as libc::c_char; 1024];
        if libc::getpwuid_r(uid, &mut pw, buf.as_mut_ptr(), buf.len(), &mut res) == 0
            && !res.is_null()
        {
            return std::ffi::CStr::from_ptr(pw.pw_name)
                .to_string_lossy()
                .into_owned();
        }
    }
    uid.to_string()
}

#[cfg(not(target_os = "macos"))]
fn info(_pid: u32) -> Option<Prozess> {
    None
}

/// Where did the program with this PID come from.
pub fn herkunft(pid: u32) -> Result<Herkunft, String> {
    if cfg!(not(target_os = "macos")) {
        return Err("`netzinfo woher` gibt es bisher nur unter macOS (über libproc).".into());
    }
    let kette = kette_aus(pid, info);
    if kette.is_empty() {
        return Err(format!(
            "Prozess {pid} nicht gefunden oder nicht lesbar. Fremde Prozesse sind ohne Root nur teilweise sichtbar."
        ));
    }
    let abgeschnitten = kette.last().is_none_or(|p| p.pid != 1);
    let (quelle, eintrag) = einstufen(&kette, plist_suchen);
    let mut h = Herkunft {
        kette,
        quelle,
        eintrag,
        abgeschnitten,
        satz: String::new(),
    };
    h.satz = satz(&h, jetzt());
    Ok(h)
}

/// PIDs for a target: with `als_pid` the number is a PID, otherwise a number is a port and
/// anything else a program name. `lauscher` is (port, pid, program).
fn pids_fuer(
    ziel: &str,
    als_pid: bool,
    lauscher: &[(u16, u32, String)],
) -> Result<Vec<u32>, String> {
    let mut v: Vec<u32> = if als_pid {
        vec![ziel.parse().map_err(|_| {
            format!("„{ziel}“ ist keine PID. Beispiel: `netzinfo woher --pid 4242`.")
        })?]
    } else if let Ok(port) = ziel.parse::<u16>() {
        lauscher
            .iter()
            .filter(|l| l.0 == port)
            .map(|l| l.1)
            .collect()
    } else {
        let n = ziel.to_lowercase();
        lauscher
            .iter()
            .filter(|l| l.2.to_lowercase().contains(&n))
            .map(|l| l.1)
            .collect()
    };
    v.sort_unstable();
    v.dedup();
    if v.is_empty() {
        return Err(format!(
            "Kein Programm von dir lauscht auf „{ziel}“. Ein Programm ohne offenen Port erreichst du mit `netzinfo woher --pid <PID>`."
        ));
    }
    Ok(v)
}

pub const OPTIONEN: &[&str] = &["--json", "--pid"];

pub fn run(args: &[String], explain_on: bool) -> Result<(), String> {
    crate::optionen::nur(args, OPTIONEN, "woher")?;
    let (mut json, mut als_pid, mut ziel) = (false, false, None);
    for a in args {
        match a.as_str() {
            "--json" => json = true,
            "--pid" => als_pid = true,
            o if o.starts_with("--") => {
                return Err(format!(
                    "Unbekannte Option „{o}“. `netzinfo woher` kennt `--json` und `--pid`."
                ));
            }
            _ if ziel.is_none() => ziel = Some(a.as_str()),
            _ => return Err("Nur ein Ziel auf einmal. Beispiel: `netzinfo woher 8080`.".into()),
        }
    }
    let ziel = ziel.ok_or(
        "Es fehlt das Ziel. Beispiel: `netzinfo woher 8080` oder `netzinfo woher <Programm>`.",
    )?;
    let e = |t: &str| crate::explain(explain_on, t);
    e(
        "Ich suche das Programm, das den Port hält, und frage macOS (libproc) bei jedem Prozess nach seinem Elternprozess. So entsteht die Kette bis launchd, dem ersten Prozess des Systems. Das ist nur Lesen, nichts geht ins Netz und es braucht kein Root.",
    );
    let pids = if als_pid {
        pids_fuer(ziel, true, &[])?
    } else {
        let l: Vec<_> = crate::offen::offen()?
            .into_iter()
            .map(|l| (l.port, l.pid, l.programm))
            .collect();
        pids_fuer(ziel, false, &l)?
    };
    let liste = pids
        .into_iter()
        .map(herkunft)
        .collect::<Result<Vec<_>, _>>()?;
    if json {
        let s = serde_json::to_string_pretty(&liste).map_err(|e| format!("JSON-Fehler: {e}"))?;
        println!("{s}");
        return Ok(());
    }
    e(
        "Endet ein Programm direkt unter launchd, hat es entweder ein Eintrag gestartet oder sein Elternprozess ist schon beendet. Dann übernimmt launchd automatisch. Die Zuordnung zu einem Eintrag ist eine Näherung, deshalb steht da „wahrscheinlich“.",
    );
    for h in &liste {
        for (i, p) in h.kette.iter().rev().enumerate() {
            println!("{}{} (PID {}, {})", "  ".repeat(i), p.name, p.pid, p.nutzer);
        }
        println!("{}\n", h.satz);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(pid: u32, ppid: u32, name: &str, pfad: &str) -> Prozess {
        Prozess {
            pid,
            ppid,
            name: name.into(),
            pfad: Some(pfad.into()),
            start: Some(1_000),
            nutzer: "mia".into(),
        }
    }

    fn tabelle() -> Vec<Prozess> {
        vec![
            p(1, 0, "launchd", "/sbin/launchd"),
            p(
                100,
                1,
                "Terminal",
                "/System/Applications/Utilities/Terminal.app/x",
            ),
            p(200, 100, "zsh", "/bin/zsh"),
            p(300, 200, "server", "/Users/x/server"),
            p(400, 1, "mdnsd", "/usr/libexec/mdnsd"),
            p(500, 1, "agent", "/Users/x/bin/agent"),
            p(600, 999, "waise", "/Users/x/waise"),
            p(700, 701, "a", "/a"),
            p(701, 700, "b", "/b"),
        ]
    }

    fn kette(pid: u32) -> Vec<Prozess> {
        kette_aus(pid, |q| tabelle().into_iter().find(|t| t.pid == q))
    }

    fn pids(k: &[Prozess]) -> Vec<u32> {
        k.iter().map(|x| x.pid).collect()
    }

    fn nichts(_: &str) -> Option<(bool, String)> {
        None
    }

    #[test]
    fn chain_ends_at_launchd() {
        assert_eq!(pids(&kette(300)), [300, 200, 100, 1]);
    }

    #[test]
    fn loop_and_missing_parent_stop_the_chain() {
        assert_eq!(pids(&kette(700)), [700, 701]);
        assert_eq!(pids(&kette(600)), [600]);
        assert!(kette(12345).is_empty());
    }

    #[test]
    fn sources_are_classified() {
        assert_eq!(einstufen(&kette(300), nichts).0, Quelle::Terminal);
        assert_eq!(einstufen(&kette(400), nichts).0, Quelle::Systemdienst);
        assert_eq!(einstufen(&kette(500), nichts).0, Quelle::Unbekannt);
        assert_eq!(einstufen(&kette(600), nichts).0, Quelle::Unbekannt);
        let agent = |f: &str| {
            (f == "/Users/x/bin/agent").then(|| (false, "de.example.agent.plist".to_string()))
        };
        assert_eq!(
            einstufen(&kette(500), agent),
            (Quelle::Anmeldeobjekt, Some("de.example.agent.plist".into()))
        );
        let daemon = |_: &str| Some((true, "de.example.d.plist".to_string()));
        assert_eq!(einstufen(&kette(500), daemon).0, Quelle::Systemdienst);
    }

    #[test]
    fn every_source_has_a_sentence() {
        let h = |k, abgeschnitten| Herkunft {
            kette: kette(if abgeschnitten { 600 } else { 300 }),
            quelle: k,
            eintrag: Some("x.plist".into()),
            abgeschnitten,
            satz: String::new(),
        };
        let s = |k, a| satz(&h(k, a), 1_000 + 3 * 3600);
        assert!(s(Quelle::Terminal, false).contains("vor 3 Stunden"));
        assert!(s(Quelle::Anmeldeobjekt, false).contains("bei jeder Anmeldung"));
        assert!(s(Quelle::Systemdienst, false).contains("Systemdienst"));
        assert!(s(Quelle::Unbekannt, true).contains("Die Kette bricht ab"));
        assert!(s(Quelle::Unbekannt, false).contains("launchd hat"));
    }

    #[test]
    fn target_resolves_port_name_and_pid() {
        let l = [
            (8080, 7, "Python".to_string()),
            (8080, 7, "Python".into()),
            (53, 9, "mdnsd".into()),
        ];
        assert_eq!(pids_fuer("8080", false, &l), Ok(vec![7]));
        assert_eq!(pids_fuer("pyth", false, &l), Ok(vec![7]));
        assert_eq!(pids_fuer("9", true, &l), Ok(vec![9]));
        assert!(pids_fuer("81", false, &l).is_err());
        assert!(pids_fuer("x", true, &l).is_err());
    }

    #[test]
    fn age_text() {
        assert_eq!(vor(10), "gerade eben");
        assert_eq!(vor(3599), "vor 1 Stunde");
        assert_eq!(vor(3000), "vor 50 Minuten");
        assert_eq!(vor(3 * 3600), "vor 3 Stunden");
        assert_eq!(vor(86_400 * 2), "vor 2 Tagen");
    }
}
