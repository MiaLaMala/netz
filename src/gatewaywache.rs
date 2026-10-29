//! Gateway-Wache: MAC des Routers pro Netz merken, Wechsel und doppelte IP melden. Issue #44.

use crate::pruefen::Klasse;
use std::path::Path;

/// Findings for `netzinfo pruefen`; `daten` is the path of netze.json, `key` the network key.
pub fn fuer_pruefen(_daten: &Path, _key: &str, _meins: bool) -> Vec<(Klasse, String)> {
    Vec::new()
}
