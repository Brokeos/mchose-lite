//! MCHOSE MagDock (3837:1012), protocole indépendant de celui de la souris.
//! Output/input reports sans numéro : AA commande type séquence total longueur données.
//! 0x07 lit l'état ; 0x27 écrit l'éclairage. Aucune commande de firmware n'est utilisée.

use hidapi::{HidApi, HidDevice};
use std::{fmt, thread, time::{Duration, Instant}};

const VENDOR_ID: u16 = 0x3837;
const PRODUCT_ID: u16 = 0x1012;
const USAGE_PAGE: u16 = 0xFF00;
const REPORT_LEN: usize = 64;
const STATUS_CMD: u8 = 0x07;
const LIGHTING_CMD: u8 = 0x27;
const TIMEOUT: Duration = Duration::from_millis(800);

pub const EFFECTS: &[(u8, &str)] = &[
    (0, "Fixe"),
    (1, "Respiration"),
    (2, "Scintillement"),
    (3, "Cycle de couleurs"),
    (4, "Vague multicolore"),
];

#[derive(Debug)]
pub enum Error {
    NotFound,
    Hid(hidapi::HidError),
    InvalidReply,
    InvalidLighting,
    NotApplied,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound => write!(f, "dock MCHOSE MagDock introuvable"),
            Self::Hid(e) => write!(f, "erreur du dock : {e}"),
            Self::InvalidReply => write!(f, "le dock ne répond pas correctement ; fermez MCHOSE HUB et réessayez"),
            Self::InvalidLighting => write!(f, "réglage d'éclairage non pris en charge par ce dock"),
            Self::NotApplied => write!(f, "éclairage non enregistré ; fermez MCHOSE HUB et réessayez"),
        }
    }
}

impl From<hidapi::HidError> for Error {
    fn from(e: hidapi::HidError) -> Self { Self::Hid(e) }
}

type Result<T> = std::result::Result<T, Error>;

/// Les dix champs de la commande 0x27. Les réglages non exposés restent inchangés.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Lighting {
    pub enabled: bool,
    pub mode: u8,
    mode_count: u8,
    /// Valeurs matérielles 0..4 (0 = le plus rapide).
    pub speed: u8,
    /// Valeurs matérielles 0..4, soit cinq niveaux de luminosité.
    pub brightness: u8,
    pub music_sync: bool,
    pub color: [u8; 3],
    direction: u8,
}

impl Lighting {
    fn parse(report: &[u8]) -> Result<Self> {
        // hidapi Windows retire l'octet d'ID nul des input reports sans numéro.
        let report = if report.first() == Some(&0) { &report[1..] } else { report };
        if report.len() < 6 || report[..5] != [0xAA, STATUS_CMD, 2, 0, 0] {
            return Err(Error::InvalidReply);
        }
        let len = report[5] as usize;
        if len < 30 || report.len() < 6 + len {
            return Err(Error::InvalidReply);
        }
        let d = &report[6..6 + len];
        if d[0] > 1 || d[5] > 1 {
            return Err(Error::InvalidReply);
        }
        let lighting = Self {
            enabled: d[0] != 0,
            mode: d[1],
            mode_count: d[2],
            speed: d[3],
            brightness: d[4],
            music_sync: d[5] != 0,
            color: [d[6], d[7], d[8]],
            direction: d[29],
        };
        lighting.validate()?;
        Ok(lighting)
    }

    fn validate(&self) -> Result<()> {
        if self.mode > 5 || self.speed > 4 || self.brightness > 4 || self.direction > 1 {
            return Err(Error::InvalidLighting);
        }
        Ok(())
    }

    pub fn effect_name(&self) -> &'static str {
        EFFECTS.iter().find(|(mode, _)| *mode == self.mode).map(|(_, name)| *name).unwrap_or("Musique (MCHOSE HUB)")
    }

    pub fn custom_color(&self) -> bool { matches!(self.mode, 0 | 1) }

    fn payload(&self) -> [u8; 10] {
        [u8::from(self.enabled), self.mode, self.mode_count, self.speed, self.brightness,
            u8::from(self.music_sync), self.color[0], self.color[1], self.color[2], self.direction]
    }
}

fn output_report(cmd: u8, payload: &[u8]) -> [u8; REPORT_LEN] {
    let mut report = [0; REPORT_LEN];
    // Premier octet réservé à l'ID de report 0 attendu par hidapi.
    report[1..7].copy_from_slice(&[0xAA, cmd, 0, 0, 0, payload.len() as u8]);
    report[7..7 + payload.len()].copy_from_slice(payload);
    report
}

pub struct Dock { dev: HidDevice }

impl Dock {
    pub fn open() -> Result<Self> {
        let api = HidApi::new()?;
        let info = api.device_list().find(|d| {
            d.vendor_id() == VENDOR_ID && d.product_id() == PRODUCT_ID && d.usage_page() == USAGE_PAGE && d.usage() == 1
        }).ok_or(Error::NotFound)?;
        Ok(Self { dev: info.open_device(&api)? })
    }

    pub fn lighting(&self) -> Result<Lighting> {
        let mut buf = [0; REPORT_LEN + 1];
        // Écarter les notifications et réponses d'une requête précédente avant d'en envoyer une nouvelle.
        for _ in 0..32 {
            if self.dev.read_timeout(&mut buf, 0)? == 0 { break; }
        }
        self.dev.write(&output_report(STATUS_CMD, &[]))?;
        let deadline = Instant::now() + TIMEOUT;
        while let Some(remaining) = deadline.checked_duration_since(Instant::now()) {
            let n = self.dev.read_timeout(&mut buf, remaining.as_millis().max(1) as i32)?;
            if n == 0 { break; }
            // Une réponse d'écriture ou une notification ne remplace pas l'état demandé.
            if let Ok(lighting) = Lighting::parse(&buf[..n]) { return Ok(lighting); }
        }
        Err(Error::InvalidReply)
    }

    pub fn set_lighting(&self, lighting: &Lighting) -> Result<Lighting> {
        lighting.validate()?;
        self.dev.write(&output_report(LIGHTING_CMD, &lighting.payload()))?;
        for _ in 0..4 {
            thread::sleep(Duration::from_millis(60));
            let actual = self.lighting()?;
            if actual == *lighting { return Ok(actual); }
        }
        Err(Error::NotApplied)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Réponse du MagDock connecté le 22/10/2026 : vague, luminosité 3/5, noir.
    const STATUS: [u8; 64] = [
        0xAA, 0x07, 0x02, 0x00, 0x00, 0x1E, 0x01, 0x04, 0x06, 0x01, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x1E, 0x04, 0x00, 0x00, 0x4D, 0x43, 0x48, 0x4F, 0x53, 0x45, 0x20, 0x4D, 0x61, 0x67, 0x44,
        0x6F, 0x63, 0x6B, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    ];

    #[test]
    fn parses_real_magdock_status() {
        let lighting = Lighting::parse(&STATUS).unwrap();
        assert!(lighting.enabled);
        assert_eq!((lighting.mode, lighting.mode_count, lighting.speed, lighting.brightness), (4, 6, 1, 2));
        assert_eq!(lighting.color, [0, 0, 0]);
        assert!(!lighting.music_sync);
        let mut numbered = vec![0];
        numbered.extend(STATUS);
        assert_eq!(Lighting::parse(&numbered).unwrap(), lighting);
    }

    #[test]
    fn rejects_truncated_unrelated_or_invalid_status() {
        for len in 0..36 { assert!(Lighting::parse(&STATUS[..len]).is_err()); }
        for (pos, value) in [(0, 0), (1, 0x27), (2, 0), (3, 1), (4, 1), (5, 64), (6, 2), (10, 5), (11, 2)] {
            let mut status = STATUS;
            status[pos] = value;
            assert!(Lighting::parse(&status).is_err(), "octet {pos}");
        }
    }

    #[test]
    fn writes_rgb_without_changing_hidden_settings() {
        let mut lighting = Lighting::parse(&STATUS).unwrap();
        lighting.mode = 0;
        lighting.color = [12, 128, 255];
        let report = output_report(LIGHTING_CMD, &lighting.payload());
        assert_eq!(&report[..17], &[0, 0xAA, 0x27, 0, 0, 0, 10, 1, 0, 6, 1, 2, 0, 12, 128, 255, 0]);
        assert!(report[17..].iter().all(|&b| b == 0));
        assert_eq!(&output_report(STATUS_CMD, &[])[..7], &[0, 0xAA, 7, 0, 0, 0, 0]);
    }

    #[test]
    #[ignore = "nécessite un MagDock ; teste les couleurs et effets puis restaure l'éclairage initial"]
    fn hardware_magdock_lighting() {
        let _lock = crate::sys::NamedMutex::hid();
        let dock = Dock::open().unwrap();
        let original = dock.lighting().unwrap();
        struct Restore<'a>(&'a Dock, Lighting);
        impl Drop for Restore<'_> {
            fn drop(&mut self) {
                if let Err(e) = self.0.set_lighting(&self.1) { eprintln!("Restauration du dock impossible : {e}"); }
            }
        }
        let restore = Restore(&dock, original.clone());
        let mut expected = original.clone();
        expected.enabled = true;
        expected.music_sync = false;
        for &(mode, name) in EFFECTS {
            expected.mode = mode;
            expected.color = [32, 128, 255];
            expected.brightness = 3;
            assert_eq!(dock.set_lighting(&expected).unwrap(), expected);
            // Réouverture pour vérifier la conservation hors du handle d'écriture.
            assert_eq!(Dock::open().unwrap().lighting().unwrap(), expected);
            println!("{name} : couleur et luminosité relues après réouverture");
        }
        for color in [[255, 0, 0], [0, 255, 0], [0, 0, 255]] {
            expected.mode = 0;
            expected.color = color;
            assert_eq!(dock.set_lighting(&expected).unwrap(), expected);
        }
        for brightness in 0..=4 {
            expected.brightness = brightness;
            assert_eq!(dock.set_lighting(&expected).unwrap(), expected);
        }
        expected.mode = 1;
        for speed in 0..=4 {
            expected.speed = speed;
            assert_eq!(dock.set_lighting(&expected).unwrap(), expected);
        }
        expected.enabled = false;
        assert_eq!(dock.set_lighting(&expected).unwrap(), expected);
        drop(restore);
        assert_eq!(dock.lighting().unwrap(), original);
        println!("Couleurs RGB, luminosité, vitesse et extinction vérifiées ; état initial restauré");
    }
}
