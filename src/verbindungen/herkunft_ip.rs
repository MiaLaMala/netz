//! Land und Anbieter eines Ziels aus einer Offline-Datenbank (Issue #57). Gesucht wird nur in zwei
//! `.mmdb`-Dateien (`land.mmdb`, `asn.mmdb`), nie im Netz. Fehlen sie, bleiben die Felder leer.

use maxminddb::Reader;
use serde::{Deserialize, Serialize};
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

type Leser = Reader<Vec<u8>>;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Ziel {
    /// Deutscher Ländername (bei unbekanntem Code der Code selbst).
    pub land: Option<String>,
    pub asn: Option<u32>,
    pub anbieter: Option<String>,
}

#[derive(Deserialize)]
struct LandSatz {
    country_code: String,
}

#[derive(Deserialize)]
struct AsnSatz {
    autonomous_system_number: u32,
    autonomous_system_organization: String,
}

pub struct Datenbank {
    land: Option<Leser>,
    asn: Option<Leser>,
}

impl Datenbank {
    /// Liest `land.mmdb` und `asn.mmdb` aus `ordner`; was fehlt oder kaputt ist, bleibt leer.
    pub fn laden(ordner: &Path) -> Datenbank {
        // ponytail: die ganze Datei im Speicher (zusammen etwa 17 MB); mmap bräuchte `unsafe`.
        let lesen = |n: &str| Reader::open_readfile(ordner.join(n)).ok();
        Datenbank {
            land: lesen("land.mmdb"),
            asn: lesen("asn.mmdb"),
        }
    }

    /// Mindestens eine der beiden Dateien ist da.
    pub fn da(&self) -> bool {
        self.land.is_some() || self.asn.is_some()
    }

    /// Bauzeit der älteren Datei als `JJJJ-MM`.
    pub fn stand(&self) -> Option<String> {
        let epoche = [&self.land, &self.asn]
            .into_iter()
            .flatten()
            .map(|r| r.metadata().build_epoch)
            .min()?;
        Some(jahr_monat(epoche / 86_400))
    }

    /// None für nicht öffentliche Adressen und ohne Treffer in beiden Dateien.
    pub fn nachschlagen(&self, ip: IpAddr) -> Option<Ziel> {
        let ip = ip.to_canonical();
        if !oeffentlich(ip) {
            return None;
        }
        let land = self
            .land
            .as_ref()
            .and_then(|r| r.lookup(ip).ok()?.decode::<LandSatz>().ok()?)
            .map(|s| laendername(&s.country_code));
        let asn = self
            .asn
            .as_ref()
            .and_then(|r| r.lookup(ip).ok()?.decode::<AsnSatz>().ok()?);
        if land.is_none() && asn.is_none() {
            return None;
        }
        Some(Ziel {
            land,
            asn: asn.as_ref().map(|a| a.autonomous_system_number),
            anbieter: asn.map(|a| a.autonomous_system_organization),
        })
    }
}

/// Die Datenbank dieses Prozesses, einmal geladen. Suchorte: `NETZ_IPDB` (Ordner), neben dem
/// Programm (`ipdb/`), im Mac-Bündel (`../Resources/ipdb/`), unter Linux `../lib/Netzinfo/ipdb/`.
pub fn datenbank() -> &'static Datenbank {
    static DB: OnceLock<Datenbank> = OnceLock::new();
    DB.get_or_init(|| {
        let neben = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(Path::to_path_buf));
        let mut orte: Vec<PathBuf> = std::env::var_os("NETZ_IPDB")
            .map(PathBuf::from)
            .into_iter()
            .collect();
        if let Some(n) = neben {
            orte.extend([
                n.join("ipdb"),
                n.join("../Resources/ipdb"),
                n.join("../lib/Netzinfo/ipdb"),
            ]);
        }
        orte.iter()
            .map(|o| Datenbank::laden(o))
            .find(Datenbank::da)
            .unwrap_or(Datenbank {
                land: None,
                asn: None,
            })
    })
}

/// Nur Adressen, die im Internet vergeben werden, werden nachgeschlagen: nicht privat, lokal,
/// link-local, Multicast, CGNAT, Dokumentation, reserviert.
pub fn oeffentlich(ip: IpAddr) -> bool {
    match ip.to_canonical() {
        IpAddr::V4(a) => {
            let o = a.octets();
            !(a.is_private()
                || a.is_loopback()
                || a.is_link_local()
                || a.is_multicast()
                || a.is_unspecified()
                || a.is_broadcast()
                || a.is_documentation()
                || o[0] == 0
                || o[0] >= 240
                || (o[0] == 100 && o[1] & 0xc0 == 64))
        }
        IpAddr::V6(a) => {
            let s = a.segments();
            !(a.is_loopback()
                || a.is_unspecified()
                || a.is_multicast()
                || s[0] & 0xfe00 == 0xfc00
                || s[0] & 0xffc0 == 0xfe80
                || (s[0] == 0x2001 && s[1] == 0x0db8))
        }
    }
}

/// Tage seit 1970 als `JJJJ-MM` (Verfahren von Howard Hinnant, wie `router::datum`).
fn jahr_monat(tage: u64) -> String {
    let z = tage as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let t = z.rem_euclid(146_097);
    let j = (t - t / 1460 + t / 36_524 - t / 146_096) / 365;
    let mp = (5 * (t - (365 * j + j / 4 - j / 100)) + 2) / 153;
    let monat = if mp < 10 { mp + 3 } else { mp - 9 };
    format!("{:04}-{monat:02}", j + era * 400 + i64::from(monat <= 2))
}

fn laendername(code: &str) -> String {
    LAENDER
        .iter()
        .find(|(c, _)| c.eq_ignore_ascii_case(code))
        .map_or_else(|| code.to_string(), |(_, n)| n.to_string())
}

const LAENDER: &[(&str, &str)] = &[
    ("AD", "Andorra"),
    ("AE", "Vereinigte Arabische Emirate"),
    ("AF", "Afghanistan"),
    ("AG", "Antigua und Barbuda"),
    ("AI", "Anguilla"),
    ("AL", "Albanien"),
    ("AM", "Armenien"),
    ("AO", "Angola"),
    ("AQ", "Antarktis"),
    ("AR", "Argentinien"),
    ("AS", "Amerikanisch-Samoa"),
    ("AT", "Österreich"),
    ("AU", "Australien"),
    ("AW", "Aruba"),
    ("AX", "Åland"),
    ("AZ", "Aserbaidschan"),
    ("BA", "Bosnien und Herzegowina"),
    ("BB", "Barbados"),
    ("BD", "Bangladesch"),
    ("BE", "Belgien"),
    ("BF", "Burkina Faso"),
    ("BG", "Bulgarien"),
    ("BH", "Bahrain"),
    ("BI", "Burundi"),
    ("BJ", "Benin"),
    ("BL", "Saint-Barthélemy"),
    ("BM", "Bermuda"),
    ("BN", "Brunei"),
    ("BO", "Bolivien"),
    ("BQ", "Bonaire, Sint Eustatius und Saba"),
    ("BR", "Brasilien"),
    ("BS", "Bahamas"),
    ("BT", "Bhutan"),
    ("BV", "Bouvetinsel"),
    ("BW", "Botswana"),
    ("BY", "Belarus"),
    ("BZ", "Belize"),
    ("CA", "Kanada"),
    ("CC", "Kokosinseln"),
    ("CD", "Demokratische Republik Kongo"),
    ("CF", "Zentralafrikanische Republik"),
    ("CG", "Republik Kongo"),
    ("CH", "Schweiz"),
    ("CI", "Elfenbeinküste"),
    ("CK", "Cookinseln"),
    ("CL", "Chile"),
    ("CM", "Kamerun"),
    ("CN", "China"),
    ("CO", "Kolumbien"),
    ("CR", "Costa Rica"),
    ("CU", "Kuba"),
    ("CV", "Kap Verde"),
    ("CW", "Curaçao"),
    ("CX", "Weihnachtsinsel"),
    ("CY", "Zypern"),
    ("CZ", "Tschechien"),
    ("DE", "Deutschland"),
    ("DJ", "Dschibuti"),
    ("DK", "Dänemark"),
    ("DM", "Dominica"),
    ("DO", "Dominikanische Republik"),
    ("DZ", "Algerien"),
    ("EC", "Ecuador"),
    ("EE", "Estland"),
    ("EG", "Ägypten"),
    ("EH", "Westsahara"),
    ("ER", "Eritrea"),
    ("ES", "Spanien"),
    ("ET", "Äthiopien"),
    ("FI", "Finnland"),
    ("FJ", "Fidschi"),
    ("FK", "Falklandinseln"),
    ("FM", "Mikronesien"),
    ("FO", "Färöer"),
    ("FR", "Frankreich"),
    ("GA", "Gabun"),
    ("GB", "Vereinigtes Königreich"),
    ("GD", "Grenada"),
    ("GE", "Georgien"),
    ("GF", "Französisch-Guayana"),
    ("GG", "Guernsey"),
    ("GH", "Ghana"),
    ("GI", "Gibraltar"),
    ("GL", "Grönland"),
    ("GM", "Gambia"),
    ("GN", "Guinea"),
    ("GP", "Guadeloupe"),
    ("GQ", "Äquatorialguinea"),
    ("GR", "Griechenland"),
    ("GS", "Südgeorgien und die Südlichen Sandwichinseln"),
    ("GT", "Guatemala"),
    ("GU", "Guam"),
    ("GW", "Guinea-Bissau"),
    ("GY", "Guyana"),
    ("HK", "Hongkong"),
    ("HM", "Heard und McDonaldinseln"),
    ("HN", "Honduras"),
    ("HR", "Kroatien"),
    ("HT", "Haiti"),
    ("HU", "Ungarn"),
    ("ID", "Indonesien"),
    ("IE", "Irland"),
    ("IL", "Israel"),
    ("IM", "Isle of Man"),
    ("IN", "Indien"),
    ("IO", "Britisches Territorium im Indischen Ozean"),
    ("IQ", "Irak"),
    ("IR", "Iran"),
    ("IS", "Island"),
    ("IT", "Italien"),
    ("JE", "Jersey"),
    ("JM", "Jamaika"),
    ("JO", "Jordanien"),
    ("JP", "Japan"),
    ("KE", "Kenia"),
    ("KG", "Kirgisistan"),
    ("KH", "Kambodscha"),
    ("KI", "Kiribati"),
    ("KM", "Komoren"),
    ("KN", "St. Kitts und Nevis"),
    ("KP", "Nordkorea"),
    ("KR", "Südkorea"),
    ("KW", "Kuwait"),
    ("KY", "Kaimaninseln"),
    ("KZ", "Kasachstan"),
    ("LA", "Laos"),
    ("LB", "Libanon"),
    ("LC", "St. Lucia"),
    ("LI", "Liechtenstein"),
    ("LK", "Sri Lanka"),
    ("LR", "Liberia"),
    ("LS", "Lesotho"),
    ("LT", "Litauen"),
    ("LU", "Luxemburg"),
    ("LV", "Lettland"),
    ("LY", "Libyen"),
    ("MA", "Marokko"),
    ("MC", "Monaco"),
    ("MD", "Moldau"),
    ("ME", "Montenegro"),
    ("MF", "Saint-Martin"),
    ("MG", "Madagaskar"),
    ("MH", "Marshallinseln"),
    ("MK", "Nordmazedonien"),
    ("ML", "Mali"),
    ("MM", "Myanmar"),
    ("MN", "Mongolei"),
    ("MO", "Macau"),
    ("MP", "Nördliche Marianen"),
    ("MQ", "Martinique"),
    ("MR", "Mauretanien"),
    ("MS", "Montserrat"),
    ("MT", "Malta"),
    ("MU", "Mauritius"),
    ("MV", "Malediven"),
    ("MW", "Malawi"),
    ("MX", "Mexiko"),
    ("MY", "Malaysia"),
    ("MZ", "Mosambik"),
    ("NA", "Namibia"),
    ("NC", "Neukaledonien"),
    ("NE", "Niger"),
    ("NF", "Norfolkinsel"),
    ("NG", "Nigeria"),
    ("NI", "Nicaragua"),
    ("NL", "Niederlande"),
    ("NO", "Norwegen"),
    ("NP", "Nepal"),
    ("NR", "Nauru"),
    ("NU", "Niue"),
    ("NZ", "Neuseeland"),
    ("OM", "Oman"),
    ("PA", "Panama"),
    ("PE", "Peru"),
    ("PF", "Französisch-Polynesien"),
    ("PG", "Papua-Neuguinea"),
    ("PH", "Philippinen"),
    ("PK", "Pakistan"),
    ("PL", "Polen"),
    ("PM", "Saint-Pierre und Miquelon"),
    ("PN", "Pitcairninseln"),
    ("PR", "Puerto Rico"),
    ("PS", "Palästina"),
    ("PT", "Portugal"),
    ("PW", "Palau"),
    ("PY", "Paraguay"),
    ("QA", "Katar"),
    ("RE", "Réunion"),
    ("RO", "Rumänien"),
    ("RS", "Serbien"),
    ("RU", "Russland"),
    ("RW", "Ruanda"),
    ("SA", "Saudi-Arabien"),
    ("SB", "Salomonen"),
    ("SC", "Seychellen"),
    ("SD", "Sudan"),
    ("SE", "Schweden"),
    ("SG", "Singapur"),
    ("SH", "St. Helena"),
    ("SI", "Slowenien"),
    ("SJ", "Spitzbergen und Jan Mayen"),
    ("SK", "Slowakei"),
    ("SL", "Sierra Leone"),
    ("SM", "San Marino"),
    ("SN", "Senegal"),
    ("SO", "Somalia"),
    ("SR", "Suriname"),
    ("SS", "Südsudan"),
    ("ST", "São Tomé und Príncipe"),
    ("SV", "El Salvador"),
    ("SX", "Sint Maarten"),
    ("SY", "Syrien"),
    ("SZ", "Eswatini"),
    ("TC", "Turks- und Caicosinseln"),
    ("TD", "Tschad"),
    ("TF", "Französische Süd- und Antarktisgebiete"),
    ("TG", "Togo"),
    ("TH", "Thailand"),
    ("TJ", "Tadschikistan"),
    ("TK", "Tokelau"),
    ("TL", "Osttimor"),
    ("TM", "Turkmenistan"),
    ("TN", "Tunesien"),
    ("TO", "Tonga"),
    ("TR", "Türkei"),
    ("TT", "Trinidad und Tobago"),
    ("TV", "Tuvalu"),
    ("TW", "Taiwan"),
    ("TZ", "Tansania"),
    ("UA", "Ukraine"),
    ("UG", "Uganda"),
    ("UM", "Amerikanische Überseeinseln"),
    ("US", "USA"),
    ("UY", "Uruguay"),
    ("UZ", "Usbekistan"),
    ("VA", "Vatikanstadt"),
    ("VC", "St. Vincent und die Grenadinen"),
    ("VE", "Venezuela"),
    ("VG", "Britische Jungferninseln"),
    ("VI", "Amerikanische Jungferninseln"),
    ("VN", "Vietnam"),
    ("VU", "Vanuatu"),
    ("WF", "Wallis und Futuna"),
    ("WS", "Samoa"),
    ("XK", "Kosovo"),
    ("YE", "Jemen"),
    ("YT", "Mayotte"),
    ("ZA", "Südafrika"),
    ("ZM", "Sambia"),
    ("ZW", "Simbabwe"),
];

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> Datenbank {
        Datenbank::laden(&Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata/ipdb"))
    }

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    #[test]
    fn finds_country_and_provider_in_the_test_files() {
        let d = db();
        assert!(d.da());
        let z = d.nachschlagen(ip("1.1.1.1")).unwrap();
        assert_eq!(z.land.as_deref(), Some("Australien"));
        assert_eq!(
            (z.asn, z.anbieter.as_deref()),
            (Some(13335), Some("CLOUDFLARENET"))
        );
        let z = d.nachschlagen(ip("2606:4700:4700::1111")).unwrap();
        assert_eq!(z.land.as_deref(), Some("Kanada"));
        assert_eq!(z.asn, Some(13335));
        // 8.8.8.8: nur ein Land, kein Anbieter
        let z = d.nachschlagen(ip("8.8.8.8")).unwrap();
        assert_eq!(
            (z.land.as_deref(), z.asn, z.anbieter),
            (Some("USA"), None, None)
        );
        // nicht in der Datei
        assert_eq!(d.nachschlagen(ip("5.5.5.5")), None);
        // IPv4 als IPv6 abgebildet
        assert!(d.nachschlagen(ip("::ffff:9.9.9.9")).is_some());
        assert_eq!(d.stand().as_deref(), Some("2026-05"));
    }

    #[test]
    fn never_looks_up_non_public_ranges() {
        for a in [
            "10.0.0.1",
            "127.0.0.1",
            "169.254.1.1",
            "224.0.0.251",
            "100.64.0.1",
            "192.0.2.1",
            "198.51.100.1",
            "203.0.113.1",
            "0.1.2.3",
            "255.255.255.255",
            "240.0.0.1",
            "::1",
            "fe80::1",
            "fd00::1",
            "ff02::1",
            "2001:db8::1",
            "::ffff:10.0.0.1",
        ] {
            assert!(!oeffentlich(ip(a)), "{a}");
        }
        for a in ["1.1.1.1", "9.9.9.9", "100.128.0.1", "2606:4700:4700::1111"] {
            assert!(oeffentlich(ip(a)), "{a}");
        }
        // auch mit Treffer in der Datei bleibt ein privates Ziel leer
        assert_eq!(db().nachschlagen(ip("192.0.2.1")), None);
    }

    #[test]
    fn missing_files_give_empty_fields_not_errors() {
        let d = Datenbank::laden(Path::new("/gibt/es/nicht"));
        assert!(!d.da());
        assert_eq!((d.nachschlagen(ip("1.1.1.1")), d.stand()), (None, None));
    }

    #[test]
    fn unknown_country_code_stays_visible() {
        assert_eq!(laendername("de"), "Deutschland");
        assert_eq!(laendername("ZZ"), "ZZ");
    }
}
