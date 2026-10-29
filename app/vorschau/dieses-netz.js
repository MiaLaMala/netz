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
