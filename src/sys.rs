//! Petits utilitaires Win32 partagés par la fenêtre de réglages et l'icône de notification.

use std::{ffi::OsStr, os::windows::ffi::OsStrExt, path::Path, ptr::null};
use windows_sys::Win32::{
    Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE, INVALID_HANDLE_VALUE},
    System::Threading::{CreateMutexW, OpenMutexW, ReleaseMutex, SYNCHRONIZATION_SYNCHRONIZE, WaitForSingleObject},
};

/// Mutex nommé de l'instance unique de l'icône de notification.
const TRAY_MUTEX: &str = "Local\\mchose-lite-tray";
/// Sérialise les échanges HID entre la fenêtre et l'icône (deux processus distincts).
const HID_MUTEX: &str = "Local\\mchose-lite-hid";

pub fn wide(s: impl AsRef<OsStr>) -> Vec<u16> {
    s.as_ref().encode_wide().chain([0]).collect()
}

/// Possession d'un mutex nommé, relâché à la destruction.
pub struct NamedMutex {
    handle: HANDLE,
    owned: bool,
}

impl NamedMutex {
    /// Crée le mutex et le prend ; `None` s'il existait déjà (autre instance).
    pub fn single_instance() -> Option<Self> {
        let handle = unsafe { CreateMutexW(null(), 1, wide(TRAY_MUTEX).as_ptr()) };
        if handle.is_null() {
            return None;
        }
        if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
            unsafe { CloseHandle(handle) };
            return None;
        }
        Some(Self { handle, owned: true })
    }

    /// Verrou HID inter-processus. Si l'attente échoue, on continue sans verrou :
    /// le protocole relit déjà ses réponses jusqu'à obtenir la bonne commande.
    pub fn hid() -> Self {
        let handle = unsafe { CreateMutexW(null(), 0, wide(HID_MUTEX).as_ptr()) };
        // WAIT_OBJECT_0 (0) ou WAIT_ABANDONED (0x80) : le verrou est à nous.
        let owned = !handle.is_null() && matches!(unsafe { WaitForSingleObject(handle, 5000) }, 0 | 0x80);
        Self { handle, owned }
    }
}

impl Drop for NamedMutex {
    fn drop(&mut self) {
        if self.handle.is_null() {
            return;
        }
        unsafe {
            if self.owned {
                ReleaseMutex(self.handle);
            }
            CloseHandle(self.handle);
        }
    }
}

pub fn tray_running() -> bool {
    let handle = unsafe { OpenMutexW(SYNCHRONIZATION_SYNCHRONIZE, 0, wide(TRAY_MUTEX).as_ptr()) };
    if handle.is_null() {
        return false;
    }
    unsafe { CloseHandle(handle) };
    true
}

/// Relance l'exécutable courant avec `args`, détaché.
pub fn spawn_self(args: &[&str]) -> bool {
    std::env::current_exe().and_then(|exe| std::process::Command::new(exe).args(args).spawn()).is_ok()
}

/// Mappe un fichier en lecture seule pour toute la durée du processus : les pages restent
/// partagées avec le cache système au lieu de compter dans la mémoire privée.
pub fn map_file(path: &Path) -> Option<&'static [u8]> {
    use windows_sys::Win32::{
        Storage::FileSystem::{CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ, GetFileSizeEx, OPEN_EXISTING},
        System::Memory::{CreateFileMappingW, FILE_MAP_READ, MapViewOfFile, PAGE_READONLY},
    };
    const GENERIC_READ: u32 = 0x8000_0000;
    unsafe {
        let file = CreateFileW(
            wide(path).as_ptr(),
            GENERIC_READ,
            FILE_SHARE_READ,
            null(),
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            std::ptr::null_mut(),
        );
        if file == INVALID_HANDLE_VALUE {
            return None;
        }
        let mut size = 0i64;
        let mapping = if GetFileSizeEx(file, &mut size) != 0 && size > 0 {
            CreateFileMappingW(file, null(), PAGE_READONLY, 0, 0, null())
        } else {
            std::ptr::null_mut()
        };
        CloseHandle(file);
        if mapping.is_null() {
            return None;
        }
        let view = MapViewOfFile(mapping, FILE_MAP_READ, 0, 0, 0);
        // La vue garde le mapping vivant après fermeture du handle.
        CloseHandle(mapping);
        (!view.Value.is_null()).then(|| std::slice::from_raw_parts(view.Value as *const u8, size as usize))
    }
}

/// Lancement de l'icône de notification à l'ouverture de session (HKCU\…\Run).
pub mod autostart {
    use super::wide;
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_SZ, RegCloseKey, RegDeleteValueW, RegOpenKeyExW,
        RegQueryValueExW, RegSetValueExW,
    };

    const RUN_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
    const VALUE: &str = "MCHOSE Lite";

    fn command() -> Option<String> {
        Some(format!("\"{}\" tray", std::env::current_exe().ok()?.display()))
    }

    fn with_key<T>(access: u32, f: impl FnOnce(HKEY) -> T) -> Option<T> {
        let mut key: HKEY = null_mut();
        if unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, wide(RUN_KEY).as_ptr(), 0, access, &mut key) } != 0 {
            return None;
        }
        let out = f(key);
        unsafe { RegCloseKey(key) };
        Some(out)
    }

    /// Actif seulement si la valeur pointe vers cet exécutable (un ancien chemin compte comme inactif).
    pub fn is_enabled() -> bool {
        let Some(expected) = command() else { return false };
        with_key(KEY_QUERY_VALUE, |key| {
            let mut buf = [0u16; 1024];
            let mut len = (buf.len() * 2) as u32;
            let mut kind = 0;
            let name = wide(VALUE);
            let ok =
                unsafe { RegQueryValueExW(key, name.as_ptr(), null(), &mut kind, buf.as_mut_ptr().cast(), &mut len) };
            if ok != 0 || kind != REG_SZ {
                return false;
            }
            let value = String::from_utf16_lossy(&buf[..len as usize / 2]);
            value.trim_end_matches('\0').eq_ignore_ascii_case(&expected)
        })
        .unwrap_or(false)
    }

    pub fn set(enabled: bool) -> Result<(), String> {
        let err = || "impossible de modifier le démarrage automatique".to_string();
        let code = with_key(KEY_SET_VALUE, |key| {
            let name = wide(VALUE);
            if enabled {
                let data = wide(command().unwrap_or_default());
                unsafe { RegSetValueExW(key, name.as_ptr(), 0, REG_SZ, data.as_ptr().cast(), (data.len() * 2) as u32) }
            } else {
                // Valeur déjà absente : rien à faire.
                match unsafe { RegDeleteValueW(key, name.as_ptr()) } {
                    2 => 0,
                    c => c,
                }
            }
        })
        .ok_or_else(err)?;
        if code == 0 { Ok(()) } else { Err(err()) }
    }
}
