//! Protocole HID des souris MCHOSE (L7 / A7 / M7 / K7), extrait du driver web officiel.
//!
//! - Feature reports `0x11` (20 octets) et `0x12` (64 octets) sur la collection
//!   vendeur `usage_page 0xFF01`.
//! - Tous les octets après l'ID de report sont inversés (XOR 0xFF).
//! - Chaque commande est acquittée par un input report `0x13` : `[0xE5, cmd, ...]`.
//! - Lecture : on envoie `[cmd, 0...]` puis on lit le feature report du même ID.

use hidapi::{HidApi, HidDevice};
use std::{fmt, thread, time::Duration};

pub const VENDOR_ID: u16 = 0x5253;
const K7_VENDOR_ID: u16 = 0x3837;
const K7_WIRED_PID: u16 = 0x4150;
const K7_RECEIVER_PID: u16 = 0x1001;
const VENDOR_USAGE_PAGE: u16 = 0xFF01;
const ACK_REPORT: u8 = 0x13;
const ACK_TAG: u8 = 0xE5;
const READ_RETRIES: usize = 6;
const ACK_TIMEOUT_MS: i32 = 400;

pub const RATES_8K: &[u16] = &[125, 500, 1000, 2000, 4000, 8000];
pub const RATES_1K: &[u16] = &[125, 500, 1000];

#[derive(Debug)]
pub enum Error {
    NotFound,
    Offline,
    Hid(hidapi::HidError),
    BadReply(u8),
    InvalidRate(u8),
    RateNotApplied { expected: u16, actual: Option<u16> },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::NotFound => write!(f, "souris MCHOSE introuvable"),
            Error::Offline => write!(f, "souris en veille ou éteinte : bougez-la pour la réveiller, puis réessayez"),
            Error::Hid(e) => write!(f, "erreur HID : {e}"),
            Error::BadReply(c) => write!(f, "réponse invalide à la commande 0x{c:02X}"),
            Error::InvalidRate(i) => write!(f, "indice de fréquence non pris en charge : {i}"),
            Error::RateNotApplied { expected, actual } => {
                write!(f, "fréquence {expected} Hz non enregistrée")?;
                if let Some(actual) = actual {
                    write!(f, " (la souris indique {actual} Hz)")?;
                }
                write!(f, ". Réveillez la souris et fermez MCHOSE HUB, puis réessayez.")
            }
        }
    }
}

impl From<hidapi::HidError> for Error {
    fn from(e: hidapi::HidError) -> Self {
        Error::Hid(e)
    }
}

pub type Result<T> = std::result::Result<T, Error>;

fn report_len(report_id: u8) -> usize {
    if report_id == 0x11 { 20 } else { 64 }
}

fn supported_device(vendor_id: u16, product_id: u16, usage_page: u16) -> bool {
    usage_page == VENDOR_USAGE_PAGE
        && (vendor_id == VENDOR_ID
            || (vendor_id == K7_VENDOR_ID && matches!(product_id, K7_WIRED_PID | K7_RECEIVER_PID)))
}

#[derive(Clone, Debug, Default)]
pub struct Info {
    pub name: String,
    /// PID du récepteur ou de la souris en filaire.
    pub product_id: u16,
    /// PID de la connexion active (0x11 0x03), notamment le récepteur en 2.4G.
    pub connection_pid: u16,
    /// PID de la souris elle-même (0x11 0x06).
    pub mouse_pid: u16,
    pub wireless: bool,
    pub firmware: String,
    pub battery: u8,
    pub charging: bool,
}

impl Info {
    /// Fréquences proposées selon le récepteur (logique du driver officiel :
    /// 0x1021 = récepteur 1K, 0x1020 / K7 0x1001 = récepteur 8K).
    pub fn rates(&self) -> &'static [u16] {
        let pid = if self.connection_pid != 0 { self.connection_pid } else { self.product_id };
        if self.wireless && pid == 0x1021 { RATES_1K } else { RATES_8K }
    }

    /// Souris en veille ou éteinte : le récepteur répond alors avec des zéros
    /// (batterie 0 %, firmware vide, config nulle). Constaté sur L7 Pro.
    pub fn offline(&self) -> bool {
        self.battery == 0 && !self.charging
    }
}

/// Configuration du profil actif (commande 0x12 0x67).
#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    pub profile: u8,
    pub wireless_dpi_index: u8,
    pub wireless_rate_index: u8,
    pub usb_dpi_index: u8,
    pub usb_rate_index: u8,
    reserved: u8,
    pub dpi: [u16; 6],
    pub dpi_y: [u16; 6],
    pub dpi_stages: u8,
    pub lod: u8,
    pub ripple: bool,
    pub angle_snap: bool,
    pub motion_sync: bool,
    /// 1 = standard, 2 = rapide, 3 = ultra.
    pub perf_mode: u8,
    pub debounce_ms: u8,
    /// Minutes avant mise en veille, 0 = jamais.
    pub sleep_min: u8,
}

impl Config {
    fn parse(d: &[u8]) -> Result<Self> {
        if d.len() < 63 {
            return Err(Error::BadReply(0x67));
        }
        if d[..63].iter().all(|&b| b == 0) {
            return Err(Error::Offline);
        }
        let u16_at = |i: usize| u16::from_le_bytes([d[i], d[i + 1]]);
        let dpi = std::array::from_fn(|i| u16_at(4 + i * 2));
        let dpi_y = std::array::from_fn(|i| u16_at(51 + i * 2));
        let sensor = d[17];
        let perf_mode = match (sensor >> 7 & 1, sensor >> 6 & 1) {
            (0, _) => 1,
            (1, 0) => 2,
            _ => 3,
        };
        Ok(Self {
            profile: d[0],
            // USB puis 2.4G ; chaque octet contient (fréquence << 4) | niveau DPI.
            // Le parseur bit4 little-endian du pilote web regroupe ces deux octets.
            // Une config symétrique telle que 0x22 0x22 masque les deux inversions.
            wireless_dpi_index: d[2] & 0x0F,
            wireless_rate_index: d[2] >> 4,
            usb_dpi_index: d[1] & 0x0F,
            usb_rate_index: d[1] >> 4,
            reserved: d[3],
            dpi,
            dpi_y,
            dpi_stages: d[16].clamp(1, 6),
            // bit0 = 1mm, bit1 = 2mm (valeurs envoyées : 1 / 2)
            lod: if sensor & 0b10 != 0 { 2 } else { 1 },
            ripple: sensor & 0b100 != 0,
            angle_snap: sensor & 0b1000 != 0,
            motion_sync: sensor & 0b1_0000 != 0,
            perf_mode,
            debounce_ms: d[18],
            sleep_min: d[19],
        })
    }

    pub fn dpi_index(&self, wireless: bool) -> u8 {
        if wireless { self.wireless_dpi_index } else { self.usb_dpi_index }
    }

    pub fn rate_index(&self, wireless: bool) -> u8 {
        if wireless { self.wireless_rate_index } else { self.usb_rate_index }
    }
}

pub struct Mouse {
    dev: HidDevice,
    pub info: Info,
}

impl Mouse {
    pub fn open() -> Result<Self> {
        let api = HidApi::new()?;
        let entry = api
            .device_list()
            .filter(|d| supported_device(d.vendor_id(), d.product_id(), d.usage_page()))
            // Le câble prend la main sur la radio : privilégier la K7 filaire si les deux sont branchés.
            .min_by_key(|d| !(d.vendor_id() == K7_VENDOR_ID && d.product_id() == K7_WIRED_PID))
            .ok_or(Error::NotFound)?;
        let product_id = entry.product_id();
        let name = entry.product_string().unwrap_or("MCHOSE").to_owned();
        let dev = entry.open_device(&api)?;
        let mut mouse = Self { dev, info: Info { name, product_id, ..Default::default() } };
        mouse.refresh_info()?;
        Ok(mouse)
    }

    pub fn refresh_info(&mut self) -> Result<()> {
        let conn = self.read(0x11, 0x03)?;
        if conn.len() < 7 {
            return Err(Error::BadReply(0x03));
        }
        self.info.connection_pid = u16::from_le_bytes([conn[3], conn[4]]);
        self.info.wireless = conn.get(5).copied().unwrap_or(0) == 1;
        let ver = self.read(0x11, 0x04)?;
        let len = (ver[0] as usize).min(ver.len() - 1);
        self.info.firmware = String::from_utf8_lossy(&ver[1..=len]).trim_end_matches('\0').to_owned();
        self.refresh_battery()
    }

    pub fn refresh_battery(&mut self) -> Result<()> {
        let st = self.read(0x11, 0x06)?;
        self.info.mouse_pid = u16::from_le_bytes([st[2], st[3]]);
        self.info.battery = st[9].min(100);
        self.info.charging = st[10] != 0;
        Ok(())
    }

    pub fn config(&self) -> Result<Config> {
        let d = self.read(0x12, 0x67)?;
        Config::parse(&d)
    }

    fn send(&self, report_id: u8, payload: &[u8]) -> Result<()> {
        let mut buf = vec![0xFFu8; report_len(report_id) + 1];
        buf[0] = report_id;
        for (dst, src) in buf[1..].iter_mut().zip(payload) {
            *dst = !src;
        }
        self.dev.send_feature_report(&buf)?;
        self.wait_ack(payload[0]);
        Ok(())
    }

    /// Attend l'acquittement `0x13 E5 cmd`. Les notifications (E2...) sont ignorées.
    fn wait_ack(&self, cmd: u8) {
        let mut buf = [0u8; 65];
        let deadline = std::time::Instant::now() + Duration::from_millis(ACK_TIMEOUT_MS as u64);
        while std::time::Instant::now() < deadline {
            match self.dev.read_timeout(&mut buf, ACK_TIMEOUT_MS) {
                Ok(n) if n >= 3 && buf[0] == ACK_REPORT && !buf[1] == ACK_TAG && !buf[2] == cmd => {
                    return;
                }
                Ok(0) | Err(_) => return,
                Ok(_) => {}
            }
        }
    }

    /// Envoie une requête de lecture et renvoie les données décodées après l'octet de commande.
    fn read(&self, report_id: u8, cmd: u8) -> Result<Vec<u8>> {
        let len = report_len(report_id);
        for _ in 0..READ_RETRIES {
            self.send(report_id, &[cmd])?;
            let mut buf = vec![0u8; len + 1];
            buf[0] = report_id;
            let n = self.dev.get_feature_report(&mut buf)?;
            if n >= 2 && !buf[1] == cmd {
                return Ok(buf[2..n].iter().map(|b| !b).collect());
            }
            thread::sleep(Duration::from_millis(20));
        }
        Err(Error::BadReply(cmd))
    }

    pub fn set_profile(&self, index: u8) -> Result<()> {
        self.send(0x11, &[0x58, index.min(2)])?;
        thread::sleep(Duration::from_millis(100));
        Ok(())
    }

    pub fn set_dpi(&self, cfg: &Config, active: u8) -> Result<()> {
        let mut p = Vec::with_capacity(41);
        p.extend([0x40, active, active, cfg.reserved]);
        cfg.dpi.iter().for_each(|v| p.extend(v.to_le_bytes()));
        p.extend([cfg.dpi_stages, 0xFF]);
        cfg.dpi_y.iter().for_each(|v| p.extend(v.to_le_bytes()));
        self.send(0x12, &p)
    }

    pub fn set_rate(&self, wireless: bool, index: u8) -> Result<()> {
        let rates = self.info.rates();
        let expected = *rates.get(index as usize).ok_or(Error::InvalidRate(index))?;
        let current = self.config()?;
        // La commande contient les deux connexions : préserver celle qui n'est pas modifiée.
        let (usb, rf) = if wireless { (current.usb_rate_index, index) } else { (index, current.wireless_rate_index) };
        self.send(0x11, &[0x41, usb, rf])?;
        let mut actual = None;
        for _ in 0..READ_RETRIES {
            thread::sleep(Duration::from_millis(50));
            let cfg = self.config()?;
            if cfg.rate_index(wireless) == index {
                return Ok(());
            }
            actual = rates.get(cfg.rate_index(wireless) as usize).copied();
        }
        Err(Error::RateNotApplied { expected, actual })
    }

    /// LOD, ripple control, angle snapping, motion sync et mode performance.
    pub fn set_sensor(&self, cfg: &Config) -> Result<()> {
        let flag = |on: bool| if on { 1 } else { 2 };
        self.send(
            0x11,
            &[0x42, cfg.lod, flag(cfg.ripple), flag(cfg.angle_snap), flag(cfg.motion_sync), 0, 0, cfg.perf_mode],
        )
    }

    pub fn set_debounce(&self, ms: u8) -> Result<()> {
        self.send(0x11, &[0x43, ms.min(20)])
    }

    pub fn set_sleep(&self, minutes: u8) -> Result<()> {
        self.send(0x11, &[0x0A, u8::from(minutes > 0), minutes])
    }

    /// Écrit uniquement les champs qui diffèrent de `old`.
    pub fn apply(&self, old: &Config, new: &Config) -> Result<()> {
        let w = self.info.wireless;
        if new.dpi != old.dpi
            || new.dpi_y != old.dpi_y
            || new.dpi_stages != old.dpi_stages
            || new.dpi_index(w) != old.dpi_index(w)
        {
            self.set_dpi(new, new.dpi_index(w).min(new.dpi_stages - 1))?;
        }
        if new.rate_index(w) != old.rate_index(w) {
            self.set_rate(w, new.rate_index(w))?;
        }
        if (new.lod, new.ripple, new.angle_snap, new.motion_sync, new.perf_mode)
            != (old.lod, old.ripple, old.angle_snap, old.motion_sync, old.perf_mode)
        {
            self.set_sensor(new)?;
        }
        if new.debounce_ms != old.debounce_ms {
            self.set_debounce(new.debounce_ms)?;
        }
        if new.sleep_min != old.sleep_min {
            self.set_sleep(new.sleep_min)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Réponse réelle d'une L7 Pro (0x12 0x67, décodée, sans l'octet de commande).
    const L7_PRO_CONFIG: [u8; 63] = [
        0x00, 0x22, 0x22, 0x02, 0x90, 0x01, 0x20, 0x03, 0xe8, 0x03, 0x80, 0x0c, 0x00, 0x19, 0x90, 0x65, 0x06, 0x02,
        0x08, 0x03, 0x00, 0x00, 0x00, 0x00, 0x10, 0x00, 0x00, 0x00, 0x20, 0x00, 0x00, 0x00, 0x30, 0x00, 0x00, 0x00,
        0x40, 0x00, 0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x80, 0x90, 0x01, 0x20,
        0x03, 0xe8, 0x03, 0x80, 0x0c, 0x00, 0x19, 0x90, 0x65,
    ];

    /// K7 Ultra 2.4G, récepteur 5253:1020, firmware 5.28.5.0.
    /// Lecture matérielle du 22/10/2026 : USB 1000 Hz, radio 8000 Hz, DPI actif 0.
    const K7_ULTRA_CONFIG: [u8; 63] = [
        0x00, 0x20, 0x50, 0x00, 0xb0, 0x04, 0xe2, 0x04, 0xe2, 0x04, 0xe2, 0x04, 0xe2, 0x04, 0xe2, 0x04, 0x01, 0x02,
        0x08, 0x03, 0x00, 0x00, 0x00, 0x00, 0x10, 0x00, 0x00, 0x00, 0x20, 0x00, 0x00, 0x00, 0x30, 0x00, 0x00, 0x00,
        0x40, 0x00, 0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x80, 0xb0, 0x04, 0xe2,
        0x04, 0xe2, 0x04, 0xe2, 0x04, 0xe2, 0x04, 0xe2, 0x04,
    ];

    #[test]
    fn parses_real_l7_pro_config() {
        let c = Config::parse(&L7_PRO_CONFIG).unwrap();
        assert_eq!(c.dpi, [400, 800, 1000, 3200, 6400, 26000]);
        assert_eq!(c.dpi_y, c.dpi);
        assert_eq!((c.dpi_stages, c.wireless_dpi_index, c.wireless_rate_index), (6, 2, 2));
        assert_eq!((c.lod, c.ripple, c.angle_snap, c.motion_sync, c.perf_mode), (2, false, false, false, 1));
        assert_eq!((c.debounce_ms, c.sleep_min), (8, 3));
    }

    #[test]
    fn parses_real_k7_ultra_config() {
        let c = Config::parse(&K7_ULTRA_CONFIG).unwrap();
        assert_eq!((c.dpi_stages, c.dpi[0], c.dpi_y[0]), (1, 1200, 1200));
        assert_eq!((c.dpi_index(false), c.dpi_index(true)), (0, 0));
        assert_eq!((c.rate_index(false), c.rate_index(true)), (2, 5));
        assert_eq!(RATES_8K[c.rate_index(true) as usize], 8000);
    }

    #[test]
    fn decodes_different_usb_and_wireless_dpi_and_rates() {
        let mut data = K7_ULTRA_CONFIG;
        data[1] = 0x31;
        data[2] = 0x52;
        data[16] = 6;
        let c = Config::parse(&data).unwrap();
        assert_eq!((c.dpi_index(false), c.rate_index(false)), (1, 3));
        assert_eq!((c.dpi_index(true), c.rate_index(true)), (2, 5));
    }

    #[test]
    fn rejects_sleeping_and_truncated_configs() {
        assert!(matches!(Config::parse(&[0; 63]), Err(Error::Offline)));
        for len in 0..63 {
            assert!(matches!(Config::parse(&K7_ULTRA_CONFIG[..len]), Err(Error::BadReply(0x67))));
        }
    }

    #[test]
    fn detects_k7_interfaces_without_opening_the_charging_dock() {
        for (vid, pid) in
            [(VENDOR_ID, 0x1020), (VENDOR_ID, 0x1021), (K7_VENDOR_ID, K7_WIRED_PID), (K7_VENDOR_ID, K7_RECEIVER_PID)]
        {
            assert!(supported_device(vid, pid, VENDOR_USAGE_PAGE));
            assert!(!supported_device(vid, pid, 0xFF0B));
            assert!(!supported_device(vid, pid, 0x0001));
        }
        assert!(!supported_device(K7_VENDOR_ID, 0x1012, VENDOR_USAGE_PAGE));
        assert!(!supported_device(0x1234, K7_WIRED_PID, VENDOR_USAGE_PAGE));
    }

    #[test]
    fn limits_rates_using_the_active_receiver() {
        let mut info = Info { product_id: K7_WIRED_PID, connection_pid: 0x1021, wireless: true, ..Default::default() };
        assert_eq!(info.rates(), RATES_1K);
        info.connection_pid = K7_RECEIVER_PID;
        assert_eq!(info.rates(), RATES_8K);
        info.connection_pid = 0x1020;
        assert_eq!(info.rates(), RATES_8K);
        info.connection_pid = 0;
        info.product_id = 0x1021;
        assert_eq!(info.rates(), RATES_1K);
        info.wireless = false;
        assert_eq!(info.rates(), RATES_8K);
    }

    #[test]
    #[ignore = "nécessite une K7 Ultra éveillée ; change les Hz puis restaure la fréquence initiale"]
    fn hardware_k7_ultra_polling_rates() {
        let _lock = crate::sys::NamedMutex::hid();
        let mouse = Mouse::open().expect("K7 Ultra connectée");
        assert!(mouse.info.name.contains("K7 Ultra"));
        assert!(!mouse.info.offline(), "Réveillez la souris");
        let original = mouse.config().unwrap();
        let wireless = mouse.info.wireless;

        struct RestoreRate<'a>(&'a Mouse, bool, u8);
        impl Drop for RestoreRate<'_> {
            fn drop(&mut self) {
                if let Err(e) = self.0.set_rate(self.1, self.2) {
                    eprintln!("Restauration de la fréquence initiale impossible : {e}");
                }
            }
        }
        let restore = RestoreRate(&mouse, wireless, original.rate_index(wireless));
        for (index, hz) in mouse.info.rates().iter().enumerate() {
            let old = mouse.config().unwrap();
            let mut expected = original.clone();
            if wireless {
                expected.wireless_rate_index = index as u8;
            } else {
                expected.usb_rate_index = index as u8;
            }
            mouse.apply(&old, &expected).unwrap();
            let actual = mouse.config().unwrap();
            assert_eq!(actual, expected, "{hz} Hz ; les autres réglages doivent être préservés");
            println!("{hz} Hz : fréquence relue et autres réglages préservés");
        }
        drop(restore);
        assert_eq!(mouse.config().unwrap(), original, "Configuration initiale restaurée");
    }
}
