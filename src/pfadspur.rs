//! Pfad-Gedächtnis: Wegänderungen pro Netz erkennen. Issue #66.

use crate::pruefen::Klasse;
use std::path::Path;

/// Findings for `netzinfo pruefen`; `daten` is the path of netze.json, `key` the network key.
pub fn fuer_pruefen(_daten: &Path, _key: &str, _meins: bool) -> Vec<(Klasse, String)> {
    Vec::new()
}
