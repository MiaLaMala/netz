// Fake answers for the engpass card in „Tempo“ (#207) in vorschau.html. Contract with src/engpass.rs:
// engpass_lesen({ macDownMbit: number | null }) -> {
//   leitung: { down_mbit, up_mbit, vertrag_down_mbit | null, vertrag_up_mbit | null, quelle: 'fritzbox' | 'upnp' } | null,
//   leitung_fehlt: { art: 'anmeldung' | 'upnp' | 'kein_router', satz } | null,   // why the router said nothing; the app adds its own way out per art
//   wlan: { rate_mbit, nutz_mbit, band: string | null } | null,   // null on cable
//   kabel: boolean,
//   mac_down_mbit: number | null,   // echoes the argument
//   engpass: 'leitung' | 'wlan' | 'router' | 'keiner' | 'unklar',
//   satz: string, rat: string | null }
// The Mac number comes from last_messen (networkQuality), the card passes it in.
// Robustheit: ,engpass-haengt (Kern antwortet nie, Frist 20 s, mit window.NETZ_FRIST kürzer) ,engpass-kaputt (NaN, negativ, null in den Zahlen).
// Marks: ,engpass-wlan ,engpass-leitung ,engpass-ohne-router ,engpass-kabel ,engpass-ungemessen (before „Tempo messen“) ,engpass-fehler; none = all good
(() => {
  const m = (s) => marken.includes(s);
  const fritz = (down, up, vd, vu) => ({ down_mbit: down, up_mbit: up, vertrag_down_mbit: vd, vertrag_up_mbit: vu, quelle: 'fritzbox' });
  VORSCHAU.engpass_lesen = async ({ macDownMbit = null } = {}) => {
    if (m('engpass-fehler')) throw 'Kein Netz gefunden: Es gibt keine Default-Route. Verbinde den Mac mit WLAN oder Kabel und versuche es noch mal.';
    if (m('engpass-haengt')) return new Promise(() => {});
    if (m('engpass-kaputt')) return { leitung: fritz(NaN, -5, NaN, null), leitung_fehlt: null, wlan: { rate_mbit: null, nutz_mbit: NaN, band: null }, kabel: false, mac_down_mbit: NaN, engpass: 'wlan', satz: 'Kaputte Zahlen.', rat: null };
    const mac = macDownMbit;
    if (m('engpass-wlan')) return { leitung: fritz(116, 42, 100, 40), leitung_fehlt: null, wlan: { rate_mbit: 72, nutz_mbit: 43, band: '2,4 GHz' }, kabel: false, mac_down_mbit: mac ?? 38,
      engpass: 'wlan', satz: 'Gebucht sind 100 Mbit/s, die Leitung schafft 116, das WLAN nur etwa 43.', rat: 'Geh näher an den Router oder nimm das 5-GHz-Netz.' };
    if (m('engpass-leitung')) return { leitung: fritz(16, 2.4, 50, 10), leitung_fehlt: null, wlan: { rate_mbit: 866, nutz_mbit: 520, band: '5 GHz' }, kabel: false, mac_down_mbit: mac ?? 15,
      engpass: 'leitung', satz: 'Die Leitung schafft nur 16 von 50 gebuchten Mbit/s.', rat: 'Das liegt beim Anbieter. Miss mit der Breitbandmessung der Bundesnetzagentur, wenn du eine Minderung willst.' };
    if (m('engpass-ohne-router')) return { leitung: null, leitung_fehlt: { art: 'anmeldung', satz: 'Die Fritz!Box nennt die Leitung erst mit Anmeldung.' }, wlan: { rate_mbit: 866, nutz_mbit: 520, band: '5 GHz' }, kabel: false, mac_down_mbit: mac ?? 94,
      engpass: 'unklar', satz: 'Das WLAN schafft etwa 520 Mbit/s, am Mac kommen 94 an. Ohne die Zahl des Routers bleibt offen, ob die Leitung mehr kann.', rat: null };
    if (m('engpass-kabel')) return { leitung: fritz(250, 40, 250, 40), leitung_fehlt: null, wlan: null, kabel: true, mac_down_mbit: mac ?? 231,
      engpass: 'keiner', satz: 'Gebucht sind 250 Mbit/s, am Mac kommen über Kabel 231 an. Du bekommst, wofür du zahlst.', rat: null };
    const gut = { leitung: fritz(116, 42, 100, 40), leitung_fehlt: null, wlan: { rate_mbit: 866, nutz_mbit: 520, band: '5 GHz' }, kabel: false };
    if (m('engpass-ungemessen') && mac == null) return { ...gut, mac_down_mbit: null, engpass: 'unklar',
      satz: 'Die Leitung schafft 116 Mbit/s, das WLAN etwa 520. Was am Mac ankommt, zeigt „Tempo messen“.', rat: null };
    return { ...gut, mac_down_mbit: mac ?? 104, engpass: 'keiner', satz: 'Gebucht sind 100 Mbit/s, am Mac kommen 104 an. Du bekommst, wofür du zahlst.', rat: null };
  };
})();
