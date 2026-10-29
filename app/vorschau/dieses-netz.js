// Fake answers for the WLAN card of the view „dieses-netz“ in vorschau.html.
// Marks: wlan-gut, wlan-schwach; without a mark (cable) the card stays hidden.
(() => {
  const gut = { standard: 'Wi-Fi 6 (802.11ax)', band: '5 GHz', kanal: 36, breite_mhz: 80, rssi_dbm: -52, rauschen_dbm: -92, snr_db: 40, rate_mbit: 1201, sicherheit: 'Wpa3', land: 'DE' };
  const schwach = { standard: 'Wi-Fi 4 (802.11n)', band: '2,4 GHz', kanal: 6, breite_mhz: 20, rssi_dbm: -83, rauschen_dbm: null, snr_db: null, rate_mbit: 24, sicherheit: 'Wpa', land: 'DE' };
  // Standort: ort-erlaubt (mit SSID), ort-abgelehnt, ort-eingeschraenkt; ohne Marke „nicht gefragt“.
  const mitName = { ...gut, ssid: 'Beispiel-WLAN', bssid: '00:00:5e:00:53:01' };
  VORSCHAU.wlan_lesen = async () => (marken.includes('ort-erlaubt') ? mitName : marken.includes('wlan-gut') ? gut : marken.includes('wlan-schwach') ? schwach : null);
  VORSCHAU.ort_status = async () => (marken.includes('ort-erlaubt') ? 'Erlaubt' : marken.includes('ort-abgelehnt') ? 'Abgelehnt' : marken.includes('ort-eingeschraenkt') ? 'Eingeschraenkt' : 'NichtGefragt');
  VORSCHAU.ort_anfragen = async () => 'NichtGefragt';
  VORSCHAU.ort_einstellungen_oeffnen = async () => null;
})();
// Verschlüsseltes DNS (#88). Marken: dns-frisch (gespeicherte Messung von heute), dns-alt (drei Tage alt,
// wird neu gemessen); ohne Marke wie bisher. Der Zähler der Messungen steht in body[data-messungen].
(() => {
  addEventListener('DOMContentLoaded', () => { document.body.dataset.messungen ??= '0'; });
  let messungen = 0;
  const messen = VORSCHAU.dns_verschluesselt;
  VORSCHAU.dns_verschluesselt = async (a) => {
    document.body.setAttribute('data-messungen', String(++messungen));
    // Die Neumessung braucht in der Vorschau etwas Zeit, damit der Zwischenzustand sichtbar bleibt.
    if (marken.includes('dns-alt-laeuft')) await new Promise(() => {});
    return messen(a);
  };
  const gespeichert = { dot: 'Erreichbar', doh: 'Erreichbar', ddr: null };
  VORSCHAU.dns_gespeichert = async () => (marken.includes('dns-frisch')
    ? { messung: { ...gespeichert, zeit: t - 3600 * 5 }, frisch: true }
    : marken.includes('dns-alt') || marken.includes('dns-alt-laeuft')
      ? { messung: { ...gespeichert, zeit: t - 86400 * 3 }, frisch: false }
      : null);
})();
// Proxy und WPAD (#53). Marken: proxy-pac (PAC-Datei und WPAD), proxy-pac-offen (Hostnamen schon geladen);
// ohne Marke „Kein Proxy“.
(() => {
  const pac = marken.includes('proxy-pac') || marken.includes('proxy-pac-offen');
  VORSCHAU.proxy_lesen = async () => (pac
    ? { manuell: null, pac_url: 'http://wpad.example.net/proxy.pac', wpad_dhcp: 'http://wpad.example.net/wpad.dat', wpad_dns: true, auto_suche: true }
    : { manuell: null, pac_url: null, wpad_dhcp: null, wpad_dns: false, auto_suche: false });
  VORSCHAU.proxy_pac_ansehen = async () => ['proxy1.example.net', 'proxy2.example.net'];
  if (marken.includes('proxy-pac-offen')) addEventListener('load', () => setTimeout(() => document.querySelector('[data-ansicht=\'dieses-netz\'] button[data-pac]')?.click(), 300));
})();
