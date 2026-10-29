//! `netzinfo messen` and `netzinfo bericht` (issue #7): latency and loss over time, stored per network.

use crate::explain;
use crate::hier;
use crate::netze::{self, Netz};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::net::{IpAddr, SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const TARGETS: [&str; 3] = ["Router (DNS)", "1.1.1.1 (DNS)", "1.1.1.1 (TCP 443)"];
/// Sentinels inside `Sample::v`; everything else is the time in microseconds.
const LOSS: u32 = u32::MAX;
const SKIP: u32 = u32::MAX - 1;
/// Raw samples older than this are merged into hourly values.
const RAW_KEEP_SECS: u64 = 48 * 3600;
/// A pause longer than this between two samples ends an outage run (the measuring had stopped).
const GAP_SECS: u64 = 120;
const TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Sample {
    /// Unix seconds.
    t: u64,
    /// Per target in `TARGETS` order: microseconds, `LOSS` or `SKIP` (not measured).
    v: [u32; 3],
}

/// One hour of one network, all targets. Median does not merge, so it keeps the sum for a mean.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Hour {
    /// Unix seconds, start of the hour.
    h: u64,
    /// Measured samples, losses included.
    n: [u32; 3],
    lost: [u32; 3],
    /// Sum of the successful times in microseconds.
    sum: [u64; 3],
    max: [u32; 3],
}

impl Hour {
    fn new(h: u64) -> Hour {
        Hour {
            h,
            n: [0; 3],
            lost: [0; 3],
            sum: [0; 3],
            max: [0; 3],
        }
    }

    fn add(&mut self, s: &Sample) {
        for k in 0..3 {
            match s.v[k] {
                SKIP => {}
                LOSS => {
                    self.n[k] += 1;
                    self.lost[k] += 1;
                }
                us => {
                    self.n[k] += 1;
                    self.sum[k] += u64::from(us);
                    self.max[k] = self.max[k].max(us);
                }
            }
        }
    }

    fn merge(&mut self, o: &Hour) {
        for k in 0..3 {
            self.n[k] += o.n[k];
            self.lost[k] += o.lost[k];
            self.sum[k] += o.sum[k];
            self.max[k] = self.max[k].max(o.max[k]);
        }
    }

    fn mean(&self, k: usize) -> Option<u32> {
        let ok = u64::from(self.n[k] - self.lost[k]);
        (ok > 0).then(|| (self.sum[k] / ok) as u32)
    }
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
struct NetData {
    samples: Vec<Sample>,
    hours: Vec<Hour>,
}

/// Fingerprint key to data. Lives next to the network list as `qualitaet.json`.
type Store = BTreeMap<String, NetData>;

fn store_path() -> Result<PathBuf, String> {
    Ok(netze::data_path()?.with_file_name("qualitaet.json"))
}

fn hour_of(t: u64) -> u64 {
    t / 3600 * 3600
}

/// Merges raw samples older than 48 h into the hourly values.
fn compact(d: &mut NetData, now: u64) {
    let cutoff = now.saturating_sub(RAW_KEEP_SECS);
    let (old, keep): (Vec<Sample>, Vec<Sample>) = d.samples.drain(..).partition(|s| s.t < cutoff);
    d.samples = keep;
    for s in &old {
        let h = hour_of(s.t);
        match d.hours.iter_mut().find(|x| x.h == h) {
            Some(x) => x.add(s),
            None => {
                let mut x = Hour::new(h);
                x.add(s);
                d.hours.push(x);
            }
        }
    }
    d.hours.sort_by_key(|x| x.h);
}

/// Sorted values: middle element, or the mean of the two middle ones.
fn median(v: &[u32]) -> Option<u32> {
    let n = v.len();
    match n {
        0 => None,
        _ if n % 2 == 1 => Some(v[n / 2]),
        _ => Some(((u64::from(v[n / 2 - 1]) + u64::from(v[n / 2])) / 2) as u32),
    }
}

/// Nearest-rank percentile of sorted values.
fn percentile(v: &[u32], p: usize) -> Option<u32> {
    (!v.is_empty()).then(|| v[(v.len() * p).div_ceil(100).max(1) - 1])
}

/// Longest run of lost samples in seconds: first loss until the next success.
/// A run that ends the data (or hits a pause in measuring) lasts until its last loss.
fn longest_outage(samples: &[Sample], k: usize) -> u64 {
    let mut best = 0;
    let mut run: Option<(u64, u64)> = None; // (first loss, last loss)
    for s in samples.iter().filter(|s| s.v[k] != SKIP) {
        if let Some((st, last)) = run
            && s.t.saturating_sub(last) > GAP_SECS
        {
            best = best.max(last - st);
            run = None;
        }
        if s.v[k] == LOSS {
            run = Some((run.map_or(s.t, |r| r.0), s.t));
        } else if let Some((st, _)) = run.take() {
            best = best.max(s.t.saturating_sub(st));
        }
    }
    if let Some((st, last)) = run {
        best = best.max(last - st);
    }
    best
}

#[derive(Debug, PartialEq)]
struct Stats {
    n: u32,
    lost: u32,
    median: Option<u32>,
    p95: Option<u32>,
    max: Option<u32>,
    outage: u64,
    /// (hour start, loss in percent, mean in microseconds): highest loss, then highest mean.
    worst: Option<(u64, f64, Option<u32>)>,
}

impl Stats {
    fn loss_pct(&self) -> f64 {
        f64::from(self.lost) * 100.0 / f64::from(self.n)
    }
}

fn stats(d: &NetData, k: usize) -> Option<Stats> {
    let (mut n, mut lost, mut max) = (0, 0, 0);
    let mut vals = Vec::new();
    let mut hours: BTreeMap<u64, Hour> = BTreeMap::new();
    // ponytail: hourly values only have a mean, so old hours enter median and p95 as their
    // mean, once per successful sample. Upgrade path: keep a small histogram per hour.
    for h in &d.hours {
        n += h.n[k];
        lost += h.lost[k];
        max = max.max(h.max[k]);
        if let Some(m) = h.mean(k) {
            vals.extend(std::iter::repeat_n(m, (h.n[k] - h.lost[k]) as usize));
        }
        hours.entry(h.h).or_insert_with(|| Hour::new(h.h)).merge(h);
    }
    for s in d.samples.iter().filter(|s| s.v[k] != SKIP) {
        n += 1;
        if s.v[k] == LOSS {
            lost += 1;
        } else {
            vals.push(s.v[k]);
            max = max.max(s.v[k]);
        }
        hours
            .entry(hour_of(s.t))
            .or_insert_with(|| Hour::new(hour_of(s.t)))
            .add(s);
    }
    if n == 0 {
        return None;
    }
    vals.sort_unstable();
    let worst = hours
        .values()
        .filter(|h| h.n[k] > 0)
        .max_by_key(|h| {
            (
                u64::from(h.lost[k]) * 10_000 / u64::from(h.n[k]),
                h.mean(k).unwrap_or(0),
            )
        })
        .map(|h| {
            (
                h.h,
                f64::from(h.lost[k]) * 100.0 / f64::from(h.n[k]),
                h.mean(k),
            )
        });
    Some(Stats {
        n,
        lost,
        median: median(&vals),
        p95: percentile(&vals, 95),
        max: (max > 0).then_some(max),
        outage: longest_outage(&d.samples, k),
        worst,
    })
}

/// Newest raw samples shown as a curve.
const SERIES_SECS: u64 = 6 * 3600;

/// Numbers of one target over all stored time. Times in milliseconds.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ZielStatistik {
    pub ziel: &'static str,
    pub messungen: u32,
    pub verloren: u32,
    pub verlust_prozent: f64,
    pub median_ms: Option<f64>,
    pub p95_ms: Option<f64>,
    pub max_ms: Option<f64>,
    /// Longest run of lost samples in seconds, 0 if none.
    pub laengster_aussetzer_s: u64,
    pub schlechteste_stunde: Option<SchlechtesteStunde>,
}

/// The hour with the highest loss, then the highest mean.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SchlechtesteStunde {
    /// Unix seconds, start of the hour.
    pub stunde: u64,
    pub verlust_prozent: f64,
    pub durchschnitt_ms: Option<f64>,
}

/// One point of the curve. Both `ms` and `verlust` empty never occurs: skipped targets leave
/// out the point.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Punkt {
    /// Unix seconds.
    pub zeit: u64,
    pub ms: Option<f64>,
    pub verlust: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Reihe {
    pub ziel: &'static str,
    pub punkte: Vec<Punkt>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Bericht {
    pub netz: String,
    /// First and last sample time, unix seconds. 0 when there is no data.
    pub von: u64,
    pub bis: u64,
    /// Empty: no measurements for this network yet.
    pub ziele: Vec<ZielStatistik>,
    /// Raw series of the last 6 hours, one per target in the order of `TARGETS`.
    pub reihen: Vec<Reihe>,
}

/// One measurement of all targets right now. Per target `ms`, `verlust`, or both empty
/// (not measured: the router is not a DNS server of this network).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Probe {
    /// Unix seconds.
    pub zeit: u64,
    pub werte: Vec<Wert>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Wert {
    pub ziel: &'static str,
    pub ms: Option<f64>,
    pub verlust: bool,
}

fn us_ms(us: u32) -> Option<f64> {
    (us < SKIP).then(|| f64::from(us) / 1000.0)
}

fn opt_ms(us: Option<u32>) -> Option<f64> {
    us.and_then(us_ms)
}

fn zielstatistik(ziel: &'static str, s: &Stats) -> ZielStatistik {
    ZielStatistik {
        ziel,
        messungen: s.n,
        verloren: s.lost,
        verlust_prozent: s.loss_pct(),
        median_ms: opt_ms(s.median),
        p95_ms: opt_ms(s.p95),
        max_ms: opt_ms(s.max),
        laengster_aussetzer_s: s.outage,
        schlechteste_stunde: s.worst.map(|(h, loss, mean)| SchlechtesteStunde {
            stunde: h,
            verlust_prozent: loss,
            durchschnitt_ms: opt_ms(mean),
        }),
    }
}

fn build_report(name: &str, d: &NetData, now: u64) -> Option<Bericht> {
    let ziele: Vec<_> = (0..3)
        .filter_map(|k| stats(d, k).map(|s| zielstatistik(TARGETS[k], &s)))
        .collect();
    if ziele.is_empty() {
        return None;
    }
    let from = d
        .hours
        .first()
        .map(|h| h.h)
        .or(d.samples.first().map(|s| s.t))?;
    let to = d.samples.last().map_or(from + 3600, |s| s.t);
    let since = now.saturating_sub(SERIES_SECS);
    let reihen = (0..3)
        .map(|k| Reihe {
            ziel: TARGETS[k],
            punkte: d
                .samples
                .iter()
                .filter(|s| s.t >= since && s.v[k] != SKIP)
                .map(|s| Punkt {
                    zeit: s.t,
                    ms: us_ms(s.v[k]),
                    verlust: s.v[k] == LOSS,
                })
                .collect(),
        })
        .collect();
    Some(Bericht {
        netz: name.to_string(),
        von: from,
        bis: to,
        ziele,
        reihen,
    })
}

fn ms(v: Option<f64>) -> String {
    v.map_or("-".into(), |v| format!("{v:.1} ms").replace('.', ","))
}

fn pct(p: f64) -> String {
    format!("{p:.1} %").replace('.', ",")
}

fn dur(secs: u64) -> String {
    match secs {
        0 => "keiner".into(),
        1..=59 => format!("{secs} s"),
        _ => format!("{} min {} s", secs / 60, secs % 60),
    }
}

fn worst_text(s: &ZielStatistik) -> String {
    s.schlechteste_stunde.as_ref().map_or("-".into(), |w| {
        format!(
            "{}, Verlust {}, Durchschnitt {}",
            netze::format_utc(w.stunde),
            pct(w.verlust_prozent),
            ms(w.durchschnitt_ms)
        )
    })
}

fn report_text(r: &Bericht) -> String {
    let mut out = format!(
        "Qualität von „{}“, {} bis {}\n",
        r.netz,
        netze::format_utc(r.von),
        netze::format_utc(r.bis)
    );
    for s in &r.ziele {
        out += &format!(
            "\n{}\n  Messungen: {}, Verlust: {}\n  Median: {}, 95. Perzentil: {}, Maximum: {}\n  Längster Aussetzer: {}\n  Schlechteste Stunde: {}\n",
            s.ziel,
            s.messungen,
            pct(s.verlust_prozent),
            ms(s.median_ms),
            ms(s.p95_ms),
            ms(s.max_ms),
            dur(s.laengster_aussetzer_s),
            worst_text(s)
        );
    }
    out
}

fn report_markdown(r: &Bericht) -> String {
    let mut out = format!(
        "# Netzqualität: {}\n\nZeitraum: {} bis {}\n\nGemessen mit `netzinfo messen` (DNS-Anfragen und TCP-Verbindungen, kein Root). Ein Verlust ist eine Anfrage ohne Antwort innerhalb von 2 Sekunden.\n\n| Ziel | Messungen | Verlust | Median | 95. Perzentil | Maximum | Längster Aussetzer |\n|---|---|---|---|---|---|---|\n",
        r.netz,
        netze::format_utc(r.von),
        netze::format_utc(r.bis)
    );
    for s in &r.ziele {
        out += &format!(
            "| {} | {} | {} | {} | {} | {} | {} |\n",
            s.ziel,
            s.messungen,
            pct(s.verlust_prozent),
            ms(s.median_ms),
            ms(s.p95_ms),
            ms(s.max_ms),
            dur(s.laengster_aussetzer_s)
        );
    }
    out += "\n## Schlechteste Stunde je Ziel\n\n";
    for s in &r.ziele {
        out += &format!("- {}: {}\n", s.ziel, worst_text(s));
    }
    out
}

/// The stored network of the current connection.
fn current_net() -> Result<(Netz, hier::NetInfo), String> {
    let info = hier::collect()?;
    let fp = netze::fingerprint(&info)?;
    let list = netze::load(&netze::data_path()?)?;
    let i = netze::find(&list, &fp).ok_or(netze::NICHT_GEMERKT_MESSEN)?;
    Ok((list[i].clone(), info))
}

fn micros(start: Instant, ok: bool) -> u32 {
    if ok {
        start.elapsed().as_micros().min(u128::from(SKIP - 1)) as u32
    } else {
        LOSS
    }
}

fn measure(router: Option<IpAddr>) -> [u32; 3] {
    let one = IpAddr::from([1, 1, 1, 1]);
    let dns = |server: IpAddr| {
        let t = Instant::now();
        let ok = crate::dns::query(server, "example.com", 1, 1).is_ok();
        micros(t, ok)
    };
    let tcp = {
        let t = Instant::now();
        let ok = TcpStream::connect_timeout(&SocketAddr::new(one, 443), TIMEOUT).is_ok();
        micros(t, ok)
    };
    [router.map_or(SKIP, dns), dns(one), tcp]
}

/// The router is only measured if it is one of the DNS servers of the network.
fn router_of(info: &hier::NetInfo) -> Option<IpAddr> {
    info.router
        .filter(|r| info.dhcp_dns.contains(r))
        .map(IpAddr::V4)
}

/// Measures once and appends the sample to the store under `key`.
fn ablegen_messung(path: &Path, key: &str, router: Option<IpAddr>) -> Result<Sample, String> {
    let s = Sample {
        t: netze::now(),
        v: measure(router),
    };
    ablegen(path, key, &s)?;
    Ok(s)
}

fn ablegen(path: &Path, key: &str, s: &Sample) -> Result<(), String> {
    let mut store: Store = netze::load_json(path)?;
    let d = store.entry(key.to_string()).or_default();
    d.samples.push(s.clone());
    compact(d, s.t);
    // ponytail: the whole file is rewritten per sample, pretty JSON, a few MB at most
    // (48 h at 5 s). Upgrade path: append-only lines file plus compaction on start.
    netze::save(path, &store)
}

/// Measures the current (stored) network once and stores the sample. The app calls this on
/// its own timer, about every 5 s.
pub fn messen_einmal() -> Result<Probe, String> {
    let (net, info) = current_net()?;
    let s = ablegen_messung(&store_path()?, &net.fingerprint.key(), router_of(&info))?;
    Ok(Probe {
        zeit: s.t,
        werte: (0..3)
            .map(|k| Wert {
                ziel: TARGETS[k],
                ms: us_ms(s.v[k]),
                verlust: s.v[k] == LOSS,
            })
            .collect(),
    })
}

/// Statistics and the 6 h curve of the named network, or of the current one.
/// No measurements yet: `ziele` is empty.
pub fn bericht_daten(name: Option<&str>) -> Result<Bericht, String> {
    let (net_name, key) = match name {
        Some(n) => {
            let list = netze::load(&netze::data_path()?)?;
            let net = list.iter().find(|x| x.name == n).ok_or_else(|| {
                format!("Ein Netz „{n}“ ist nicht gemerkt. `netzinfo netze` zeigt die Liste.")
            })?;
            (net.name.clone(), net.fingerprint.key())
        }
        None => {
            let (net, _) = current_net()?;
            (net.name.clone(), net.fingerprint.key())
        }
    };
    bericht_aus(&store_path()?, &net_name, &key, netze::now())
}

fn bericht_aus(path: &Path, name: &str, key: &str, now: u64) -> Result<Bericht, String> {
    let store: Store = netze::load_json(path)?;
    Ok(store
        .get(key)
        .and_then(|d| build_report(name, d, now))
        .unwrap_or_else(|| Bericht {
            netz: name.to_string(),
            von: 0,
            bis: 0,
            ziele: vec![],
            reihen: vec![],
        }))
}

fn cell(us: u32) -> String {
    match us {
        LOSS => "Verlust".into(),
        SKIP => "übersprungen".into(),
        us => ms(us_ms(us)),
    }
}

fn num(args: &[String], flag: &str, default: u64, max: u64) -> Result<u64, String> {
    let Some(i) = args.iter().position(|a| a == flag) else {
        return Ok(default);
    };
    match args.get(i + 1).and_then(|v| v.parse::<u64>().ok()) {
        Some(n) if (1..=max).contains(&n) => Ok(n),
        _ => Err(format!(
            "Nach `{flag}` gehört eine ganze Zahl von 1 bis {max}."
        )),
    }
}

pub fn run_messen(args: &[String], explain_on: bool) -> Result<(), String> {
    let minutes = num(args, "--minuten", 1, 1440)?;
    let takt = num(args, "--takt", 5, 3600)?;
    let (net, info) = current_net()?;
    let key = net.fingerprint.key();
    let router = router_of(&info);
    if router.is_none() {
        println!(
            "Der Router ist nicht einer der DNS-Server dieses Netzes. Ich überspringe die DNS-Messung zum Router und messe nur 1.1.1.1."
        );
    }
    let total = (minutes * 60 / takt).max(1);
    println!(
        "Messe im Netz „{}“: {total} Messungen, alle {takt} s. Mit Ctrl-C brichst du ab, bisherige Werte bleiben gespeichert.",
        net.name
    );
    explain(
        explain_on,
        "Pro Messung stelle ich eine DNS-Anfrage an den Router und eine an 1.1.1.1 und baue eine TCP-Verbindung zu 1.1.1.1 auf Port 443 auf. Ich messe nur, wie lange die Antwort dauert. Kommt innerhalb von 2 Sekunden nichts zurück, zählt das als Verlust.",
    );
    let path = store_path()?;
    let mut session = Vec::new();
    for i in 1..=total {
        let started = Instant::now();
        let s = ablegen_messung(&path, &key, router)?;
        println!(
            "{i}/{total}  Router {} | DNS 1.1.1.1 {} | TCP 1.1.1.1 {}",
            cell(s.v[0]),
            cell(s.v[1]),
            cell(s.v[2])
        );
        session.push(s);
        if i < total
            && let Some(left) = Duration::from_secs(takt).checked_sub(started.elapsed())
        {
            std::thread::sleep(left);
        }
    }
    let d = NetData {
        samples: session,
        hours: vec![],
    };
    if let Some(r) = build_report(&net.name, &d, netze::now()) {
        println!("\nZusammenfassung dieser Messung\n{}", report_text(&r));
    }
    explain(
        explain_on,
        "Eine Messung sagt wenig, viele Messungen über Tage sagen viel. Starte `netzinfo messen --minuten 1440` im Problemnetz und lies danach `netzinfo bericht`.",
    );
    explain_numbers(explain_on);
    Ok(())
}

fn explain_numbers(on: bool) {
    explain(
        on,
        "Latenz ist die Zeit, bis eine Antwort kommt. Verlust heißt, es kommt gar keine. Beides ist unabhängig: Ein Netz kann schnell antworten und trotzdem Pakete verlieren, und umgekehrt.",
    );
    explain(
        on,
        "Ich nehme den Median statt des Durchschnitts: Der Median ist der mittlere Wert der sortierten Messungen. Ein einziger Ausreißer von 3 Sekunden zieht den Durchschnitt stark hoch, den Median kaum. Das 95. Perzentil sagt: 95 von 100 Messungen waren schneller. So sieht man, wie schlecht die schlechten Momente sind.",
    );
    explain(
        on,
        "Die DNS-Zeit zum Router zeigt, ob dein WLAN oder Kabel bis zum Router gut ist. Die DNS-Zeit zu 1.1.1.1 und die TCP-Zeit zeigen den Weg ins Internet. Ist nur der Router schnell, liegt das Problem dahinter, in der Firma oder beim Anbieter. Der TCP-Connect (SYN, SYN-ACK, ACK) braucht eine volle Runde hin und zurück und misst deshalb die reine Laufzeit. Eine DNS-Antwort kann dagegen schon im Zwischenspeicher des Servers liegen.",
    );
}

pub fn run_bericht(args: &[String], explain_on: bool) -> Result<(), String> {
    let mut name = None;
    let mut md = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == "--markdown" {
            md = Some(it.next().ok_or(
                "Nach `--markdown` fehlt der Dateiname, z. B. `netzinfo bericht --markdown bericht.md`.",
            )?);
        } else if a.starts_with("--") || name.is_some() {
            return Err(format!(
                "„{a}“ passt hier nicht. Aufruf: `netzinfo bericht [Name] [--markdown <Datei>]`."
            ));
        } else {
            name = Some(a.as_str());
        }
    }
    let r = bericht_daten(name)?;
    let wlan = crate::wlanverlauf::abschnitt(&r.netz);
    if r.ziele.is_empty() {
        println!(
            "Für „{}“ gibt es noch keine Messungen. Starte `netzinfo messen`, am besten für mehrere Stunden mit `--minuten 240`.",
            r.netz
        );
        if let Some(w) = &wlan {
            println!("\nWLAN\n{w}");
        }
        return Ok(());
    }
    print!("{}", report_text(&r));
    if let Some(w) = &wlan {
        println!("\nWLAN\n{w}");
    }
    if let Some(file) = md {
        let w = wlan.map_or(String::new(), |w| format!("\n## WLAN\n\n{w}"));
        std::fs::write(file, report_markdown(&r) + &w).map_err(|e| {
            format!(
                "Die Datei {file} ließ sich nicht schreiben ({e}). Prüfe den Pfad und versuche es noch mal."
            )
        })?;
        println!("\nMarkdown-Bericht geschrieben: {file}");
    }
    explain(
        explain_on,
        "Der Bericht rechnet über alle gespeicherten Messungen dieses Netzes. Rohwerte der letzten 48 Stunden sind einzeln gespeichert, ältere nur als Stundenwerte, damit die Datei klein bleibt. Median und Perzentil älterer Zeiten sind deshalb eine Näherung.",
    );
    explain_numbers(explain_on);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(t: u64, v: [u32; 3]) -> Sample {
        Sample { t, v }
    }

    /// Only target 1 varies in these fixtures; the others succeed with 1 ms.
    fn one(t: u64, v1: u32) -> Sample {
        s(t, [1000, v1, 1000])
    }

    #[test]
    fn median_and_percentile() {
        assert_eq!(median(&[]), None);
        assert_eq!(median(&[5]), Some(5));
        assert_eq!(median(&[1, 2, 100]), Some(2));
        assert_eq!(median(&[1, 2, 3, 100]), Some(2));
        let v: Vec<u32> = (1..=100).collect();
        assert_eq!(percentile(&v, 95), Some(95));
        assert_eq!(percentile(&v, 100), Some(100));
        assert_eq!(percentile(&[7], 95), Some(7));
        assert_eq!(percentile(&[], 95), None);
    }

    #[test]
    fn outage_runs() {
        let a = [
            one(0, 5),
            one(5, 5),
            one(10, LOSS),
            one(15, LOSS),
            one(20, 5),
            one(25, LOSS),
            one(30, 5),
        ];
        assert_eq!(longest_outage(&a, 1), 10);
        assert_eq!(longest_outage(&a, 0), 0);
        // a loss at the very end counts to its last loss only
        assert_eq!(
            longest_outage(&[one(0, 5), one(5, LOSS), one(10, LOSS)], 1),
            5
        );
        // a pause in measuring ends the run
        assert_eq!(longest_outage(&[one(0, LOSS), one(1000, LOSS)], 1), 0);
        let sk = [s(0, [SKIP; 3]), s(5, [SKIP, LOSS, 1]), s(10, [SKIP, 3, 1])];
        assert_eq!(longest_outage(&sk, 0), 0);
        assert_eq!(longest_outage(&sk, 1), 5);
    }

    #[test]
    fn compaction_into_hours() {
        let now = 10 * 24 * 3600;
        let old = now - 3 * 24 * 3600;
        let mut d = NetData {
            samples: vec![
                one(old, 2000),
                one(old + 10, 4000),
                one(old + 20, LOSS),
                one(old + 3600, 1000),
                s(old + 30, [SKIP, 1000, 1000]),
                one(now - 100, 3000),
            ],
            hours: vec![],
        };
        compact(&mut d, now);
        assert_eq!(d.samples, vec![one(now - 100, 3000)]);
        assert_eq!(d.hours.len(), 2);
        let h = &d.hours[0];
        assert_eq!(h.h, old);
        assert_eq!((h.n[1], h.lost[1], h.sum[1], h.max[1]), (4, 1, 7000, 4000));
        // router was skipped once: 3 samples count
        assert_eq!(h.n[0], 3);
        assert_eq!(h.mean(1), Some(2333));
        let before = d.clone();
        compact(&mut d, now);
        assert_eq!(d, before);
        // a later run merges into the existing hour
        d.samples.push(one(old + 40, 6000));
        compact(&mut d, now + 1);
        assert_eq!(d.hours.len(), 2);
        assert_eq!(d.hours[0].n[1], 5);
    }

    fn fixture() -> NetData {
        let base = 100 * 3600;
        let mut samples = Vec::new();
        // hour 1: 1 to 10 ms
        for i in 0..10 {
            samples.push(one(base + i * 5, 1000 * (i as u32 + 1)));
        }
        // hour 2: outage of 10 s, one slow answer
        samples.push(one(base + 3600, 2000));
        samples.push(one(base + 3605, LOSS));
        samples.push(one(base + 3610, LOSS));
        samples.push(one(base + 3615, 90_000));
        NetData {
            samples,
            hours: vec![],
        }
    }

    #[test]
    fn stats_from_fixed_data() {
        let st = stats(&fixture(), 1).unwrap();
        assert_eq!((st.n, st.lost), (14, 2));
        // 12 values: 1..10 ms, 2 ms, 90 ms; sorted middle is 5 and 6
        assert_eq!(st.median, Some(5500));
        assert_eq!(st.p95, Some(90_000));
        assert_eq!(st.max, Some(90_000));
        assert_eq!(st.outage, 10);
        let (h, loss, mean) = st.worst.unwrap();
        assert_eq!(h, 101 * 3600);
        assert!((loss - 50.0).abs() < 1e-9);
        assert_eq!(mean, Some(46_000));
        assert_eq!(stats(&fixture(), 0).unwrap().lost, 0);
        assert!(stats(&NetData::default(), 0).is_none());
    }

    #[test]
    fn old_hours_count_with_their_mean() {
        let mut d = fixture();
        d.hours.push(Hour {
            h: 50 * 3600,
            n: [0, 4, 0],
            lost: [0, 1, 0],
            sum: [0, 9000, 0],
            max: [0, 5000, 0],
        });
        let st = stats(&d, 1).unwrap();
        assert_eq!((st.n, st.lost), (18, 3));
        assert_eq!(st.max, Some(90_000));
        assert_eq!(build_report("X", &d, 0).unwrap().von, 50 * 3600);
    }

    #[test]
    fn report_texts() {
        let r = build_report("Firma", &fixture(), 0).unwrap();
        let t = report_text(&r);
        assert!(t.starts_with("Qualität von „Firma“"));
        assert!(t.contains("1.1.1.1 (DNS)\n  Messungen: 14, Verlust: 14,3 %"));
        assert!(t.contains("Median: 5,5 ms, 95. Perzentil: 90,0 ms, Maximum: 90,0 ms"));
        assert!(t.contains("Längster Aussetzer: 10 s"));
        assert!(t.contains("Router (DNS)\n  Messungen: 14, Verlust: 0,0 %"));
        assert!(t.contains("Verlust 50,0 %"));
        let m = report_markdown(&r);
        assert!(m.starts_with("# Netzqualität: Firma"));
        assert!(m.contains("| 1.1.1.1 (DNS) | 14 | 14,3 % | 5,5 ms | 90,0 ms | 90,0 ms | 10 s |"));
        assert!(m.contains("- 1.1.1.1 (TCP 443): "));
        assert!(!t.contains('\u{2014}') && !m.contains('\u{2014}') && !m.contains('\u{2013}'));
    }

    #[test]
    fn report_without_data() {
        assert!(build_report("X", &NetData::default(), 0).is_none());
        let d = NetData {
            samples: vec![s(0, [SKIP; 3])],
            hours: vec![],
        };
        assert!(build_report("X", &d, 0).is_none());
    }

    #[test]
    fn series_covers_last_six_hours_only() {
        let now = 100 * 3600 + 7 * 3600;
        let d = NetData {
            samples: vec![
                one(now - 6 * 3600 - 1, 1000),
                one(now - 6 * 3600, 2000),
                s(now - 10, [SKIP, LOSS, 3000]),
            ],
            hours: vec![],
        };
        let r = build_report("X", &d, now).unwrap();
        assert_eq!(r.reihen.len(), 3);
        // too old sample is out, skipped router leaves no point
        assert_eq!(r.reihen[0].punkte.len(), 1);
        assert_eq!(r.reihen[1].punkte.len(), 2);
        assert_eq!(r.reihen[1].punkte[0].zeit, now - 6 * 3600);
        assert_eq!(r.reihen[1].punkte[0].ms, Some(2.0));
        assert!(r.reihen[1].punkte[1].verlust && r.reihen[1].punkte[1].ms.is_none());
        // the statistics still use all samples
        assert_eq!(r.ziele[1].messungen, 3);
        assert_eq!(r.ziele[1].median_ms, Some(1.5));
        assert!(
            serde_json::to_string(&r)
                .unwrap()
                .contains("laengster_aussetzer_s")
        );
    }

    #[test]
    fn store_and_report_from_file() {
        let dir = std::env::temp_dir().join(format!("netz-messen-{}", std::process::id()));
        let path = dir.join("qualitaet.json");
        let now = netze::now();
        let empty = bericht_aus(&path, "WG", "k", now).unwrap();
        assert!(empty.ziele.is_empty() && empty.reihen.is_empty() && empty.netz == "WG");
        ablegen(&path, "k", &s(now - 5, [SKIP, 4000, LOSS])).unwrap();
        ablegen(&path, "k", &s(now, [SKIP, 6000, 9000])).unwrap();
        let r = bericht_aus(&path, "WG", "k", now).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(r.ziele.len(), 2);
        assert_eq!(r.ziele[0].ziel, TARGETS[1]);
        assert_eq!(r.reihen[2].punkte.len(), 2);
        assert_eq!(r.bis, now);
    }

    #[test]
    fn json_round_trip() {
        let mut store = Store::new();
        let mut d = fixture();
        d.samples.push(s(1, [SKIP, LOSS, 7]));
        d.hours.push(Hour::new(3600));
        store.insert(
            "192.168.178.1|192.168.178.1|fritz.box|192.168.178.0/24".into(),
            d,
        );
        let json = serde_json::to_string(&store).unwrap();
        assert_eq!(serde_json::from_str::<Store>(&json).unwrap(), store);
    }

    #[test]
    fn number_arguments() {
        let a = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert_eq!(num(&a(&[]), "--minuten", 1, 1440), Ok(1));
        assert_eq!(num(&a(&["--minuten", "90"]), "--minuten", 1, 1440), Ok(90));
        assert!(num(&a(&["--minuten", "1441"]), "--minuten", 1, 1440).is_err());
        assert!(num(&a(&["--minuten", "0"]), "--minuten", 1, 1440).is_err());
        assert!(num(&a(&["--minuten"]), "--minuten", 1, 1440).is_err());
        assert!(num(&a(&["--takt", "x"]), "--takt", 5, 3600).is_err());
    }
}
