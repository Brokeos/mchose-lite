//! Icône de notification : batterie de la souris, relue toutes les 60 s.
//!
//! Win32 brut (Shell_NotifyIconW + GDI) pour rester à quelques Mo de mémoire privée.
//! Le périphérique HID n'est ouvert que le temps de la lecture, dans un thread dédié,
//! pour laisser la fenêtre de réglages s'en servir librement.

use crate::{
    gui::WINDOW_TITLE,
    icon,
    protocol::{Error, Mouse},
    sys::{self, NamedMutex, autostart, wide},
};
use std::{
    cell::Cell,
    ffi::c_void,
    mem::{size_of, zeroed},
    ptr::{null, null_mut},
    sync::{Mutex, OnceLock, mpsc},
    thread,
    time::Duration,
};
use windows_sys::Win32::{
    Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, SYSTEMTIME, WPARAM},
    Graphics::{Dwm::DwmSetWindowAttribute, Gdi::*},
    System::{LibraryLoader::*, SystemInformation::*},
    UI::{
        HiDpi::*,
        Input::KeyboardAndMouse::{TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent, VK_ESCAPE},
        Shell::*,
        WindowsAndMessaging::*,
    },
};

const WM_TRAY: u32 = WM_APP + 1;
const WM_STATUS: u32 = WM_APP + 2;
/// Envoyé par une seconde instance : ouvre le widget de l'instance en cours.
const WM_SHOW: u32 = WM_APP + 3;
// Absents de windows-sys.
const NIN_KEYSELECT: u32 = WM_USER + 1;
const WM_MOUSELEAVE: u32 = 0x02A3;
const TRAY_CLASS: &str = "MchoseLiteTray";
const POPUP_CLASS: &str = "MchoseLitePopup";
const POLL_EVERY: Duration = Duration::from_secs(60);
const LOW_BATTERY: u8 = 15;

const CMD_OPEN: usize = 1;
const CMD_REFRESH: usize = 2;
const CMD_AUTOSTART: usize = 3;
const CMD_QUIT: usize = 4;

// Palette (0xRRGGBB), alignée sur la fenêtre de réglages.
const BG: u32 = 0x1B1C20;
const FIELD: u32 = 0x2A2B31;
const HOVER: u32 = 0x26272D;
const LINE: u32 = 0x2C2D33;
const TEXT: u32 = 0xECEEF2;
const DIM: u32 = 0x9A9EA8;
const ACCENT: u32 = 0x4C8DFF;
const GREEN: u32 = 0x3DD68C;
const AMBER: u32 = 0xF5B544;
const RED: u32 = 0xEA5E56;

/// Dimensions du widget en pixels logiques (96 dpi).
const POPUP_W: i32 = 300;
const POPUP_H: i32 = 178;
const BUTTON_TOP: i32 = 138;

#[derive(Clone, Copy, PartialEq)]
enum Link {
    /// Première lecture pas encore terminée.
    Pending,
    Online,
    /// Récepteur présent mais souris muette (veille ou éteinte).
    Asleep,
    /// Aucun récepteur ni câble MCHOSE.
    Absent,
}

#[derive(Clone)]
struct Status {
    link: Link,
    name: String,
    battery: u8,
    charging: bool,
    wireless: bool,
    /// Heure de la dernière lecture (h, min).
    at: (u16, u16),
}

static STATUS: Mutex<Status> = Mutex::new(Status {
    link: Link::Pending,
    name: String::new(),
    battery: 0,
    charging: false,
    wireless: true,
    at: (0, 0),
});
static WAKE: OnceLock<mpsc::Sender<()>> = OnceLock::new();

fn status() -> Status {
    STATUS.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

impl Status {
    fn name(&self) -> &str {
        if self.name.is_empty() { "MCHOSE" } else { &self.name }
    }

    fn battery_color(&self) -> u32 {
        match self.battery {
            _ if self.charging => GREEN,
            0..=LOW_BATTERY => RED,
            16..=35 => AMBER,
            _ => GREEN,
        }
    }

    fn tooltip(&self) -> String {
        match self.link {
            Link::Pending => format!("{} — lecture…", self.name()),
            Link::Online if self.charging => format!("{} — {} % (en charge)", self.name(), self.battery),
            Link::Online => format!("{} — {} %", self.name(), self.battery),
            Link::Asleep | Link::Absent => format!("{} — déconnectée", self.name()),
        }
    }
}

/// Lecture de la batterie, périphérique ouvert puis refermé aussitôt.
fn poll(prev: &Status) -> Status {
    let mut st = Status { at: local_time(), ..prev.clone() };
    let _lock = NamedMutex::hid();
    match Mouse::open() {
        // Récepteur présent mais souris muette : seul le nom (chaîne USB) est fiable.
        Ok(m) if m.info.offline() => {
            st.link = Link::Asleep;
            st.name = m.info.name;
        }
        Ok(m) => {
            st.link = Link::Online;
            st.name = m.info.name;
            st.battery = m.info.battery;
            st.charging = m.info.charging;
            st.wireless = m.info.wireless;
        }
        Err(Error::NotFound) => st.link = Link::Absent,
        Err(_) => st.link = Link::Asleep,
    }
    st
}

fn local_time() -> (u16, u16) {
    let mut t: SYSTEMTIME = unsafe { zeroed() };
    unsafe { GetLocalTime(&mut t) };
    (t.wHour, t.wMinute)
}

fn tick() -> u64 {
    unsafe { GetTickCount64() }
}

/// État de l'interface, propre au thread principal (procédures de fenêtre réentrantes :
/// uniquement des `Cell`, jamais d'emprunt maintenu pendant un appel Win32).
struct Ui {
    main: Cell<HWND>,
    popup: Cell<HWND>,
    tray_icon: Cell<HICON>,
    logo: Cell<(HICON, i32)>,
    hover: Cell<bool>,
    hidden_at: Cell<u64>,
    polled_at: Cell<u64>,
    requested_at: Cell<u64>,
    low_warned: Cell<bool>,
    taskbar_created: Cell<u32>,
}

thread_local! {
    static UI: Ui = Ui {
        main: Cell::new(null_mut()),
        popup: Cell::new(null_mut()),
        tray_icon: Cell::new(null_mut()),
        logo: Cell::new((null_mut(), 0)),
        hover: Cell::new(false),
        hidden_at: Cell::new(0),
        polled_at: Cell::new(0),
        requested_at: Cell::new(0),
        low_warned: Cell::new(false),
        taskbar_created: Cell::new(0),
    };
}

pub fn run() -> Result<(), String> {
    let Some(_instance) = NamedMutex::single_instance() else {
        // Déjà lancée : on montre son widget plutôt que d'ajouter une deuxième icône.
        let hwnd = unsafe { FindWindowW(wide(TRAY_CLASS).as_ptr(), null()) };
        if !hwnd.is_null() {
            unsafe {
                AllowSetForegroundWindow(ASFW_ANY);
                PostMessageW(hwnd, WM_SHOW, 0, 0);
            }
        }
        return Ok(());
    };
    unsafe {
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        dark_menus();
        let hinst = GetModuleHandleW(null());
        let arrow = LoadCursorW(null_mut(), IDC_ARROW);
        let tray_class = wide(TRAY_CLASS);
        let popup_class = wide(POPUP_CLASS);
        let mut wc: WNDCLASSEXW = zeroed();
        wc.cbSize = size_of::<WNDCLASSEXW>() as u32;
        wc.hInstance = hinst;
        wc.hCursor = arrow;
        wc.lpfnWndProc = Some(main_proc);
        wc.lpszClassName = tray_class.as_ptr();
        RegisterClassExW(&wc);
        wc.lpfnWndProc = Some(popup_proc);
        wc.lpszClassName = popup_class.as_ptr();
        wc.style = CS_DROPSHADOW;
        RegisterClassExW(&wc);

        // Fenêtre principale jamais affichée (top-level pour recevoir « TaskbarCreated »).
        let main = CreateWindowExW(
            0,
            tray_class.as_ptr(),
            wide("MCHOSE Lite tray").as_ptr(),
            WS_POPUP,
            0,
            0,
            0,
            0,
            null_mut(),
            null_mut(),
            hinst,
            null(),
        );
        let popup = CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_TOPMOST,
            popup_class.as_ptr(),
            wide("MCHOSE Lite batterie").as_ptr(),
            WS_POPUP,
            0,
            0,
            POPUP_W,
            POPUP_H,
            null_mut(),
            null_mut(),
            hinst,
            null(),
        );
        if main.is_null() || popup.is_null() {
            return Err("création de fenêtre impossible".into());
        }
        // Coins arrondis et bordure discrète (Windows 11 ; ignoré ailleurs).
        let round: i32 = 2; // DWMWCP_ROUND
        DwmSetWindowAttribute(popup, 33, (&raw const round).cast(), 4);
        let border: COLORREF = colorref(0x34353C);
        DwmSetWindowAttribute(popup, 34, (&raw const border).cast(), 4);
        UI.with(|ui| {
            ui.main.set(main);
            ui.popup.set(popup);
            ui.taskbar_created.set(RegisterWindowMessageW(wide("TaskbarCreated").as_ptr()));
        });
        add_icon(main);
        start_poller(main);

        let mut msg: MSG = zeroed();
        while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    Ok(())
}

fn start_poller(main: HWND) {
    let (tx, rx) = mpsc::channel();
    let _ = WAKE.set(tx);
    // HWND n'est pas Send : on transporte sa valeur.
    let target = main as isize;
    thread::spawn(move || {
        loop {
            let st = poll(&status());
            *STATUS.lock().unwrap_or_else(|e| e.into_inner()) = st;
            unsafe { PostMessageW(target as HWND, WM_STATUS, 0, 0) };
            if let Err(mpsc::RecvTimeoutError::Disconnected) = rx.recv_timeout(POLL_EVERY) {
                return;
            }
            // Regroupe les demandes arrivées pendant la lecture.
            while rx.try_recv().is_ok() {}
        }
    });
}

fn request_poll() {
    if let Some(tx) = WAKE.get() {
        let _ = tx.send(());
    }
}

/// Dernière lecture plus vieille que `ms`.
fn stale(ms: u64) -> bool {
    tick().saturating_sub(UI.with(|ui| ui.polled_at.get())) > ms
}

/// Lecture à la demande (survol, ouverture du widget), au plus une toutes les 10 s.
fn request_poll_throttled() {
    let now = tick();
    if stale(10_000) && now.saturating_sub(UI.with(|ui| ui.requested_at.get())) > 10_000 {
        UI.with(|ui| ui.requested_at.set(now));
        request_poll();
    }
}

/// Menus contextuels sombres (API non documentée d'uxtheme, sans effet si absente).
unsafe fn dark_menus() {
    unsafe {
        let lib = LoadLibraryExW(wide("uxtheme.dll").as_ptr(), null_mut(), LOAD_LIBRARY_SEARCH_SYSTEM32);
        if lib.is_null() {
            return;
        }
        // Ordinal 135 : SetPreferredAppMode(ForceDark = 2), 136 : FlushMenuThemes.
        if let Some(f) = GetProcAddress(lib, 135 as *const u8) {
            let set_mode: unsafe extern "system" fn(i32) -> i32 = std::mem::transmute(f);
            set_mode(2);
        }
        if let Some(f) = GetProcAddress(lib, 136 as *const u8) {
            let flush: unsafe extern "system" fn() = std::mem::transmute(f);
            flush();
        }
    }
}

fn notify_data(main: HWND) -> NOTIFYICONDATAW {
    let mut nid: NOTIFYICONDATAW = unsafe { zeroed() };
    nid.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
    nid.hWnd = main;
    nid.uID = 1;
    nid
}

fn copy_wide(dst: &mut [u16], s: &str) {
    let src: Vec<u16> = s.encode_utf16().take(dst.len() - 1).collect();
    dst[..src.len()].copy_from_slice(&src);
    dst[src.len()] = 0;
}

fn add_icon(main: HWND) {
    let st = status();
    let icon = tray_icon(&st);
    let mut nid = notify_data(main);
    nid.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP | NIF_SHOWTIP;
    nid.uCallbackMessage = WM_TRAY;
    nid.hIcon = icon;
    copy_wide(&mut nid.szTip, &st.tooltip());
    unsafe {
        Shell_NotifyIconW(NIM_ADD, &nid);
        nid.Anonymous.uVersion = NOTIFYICON_VERSION_4;
        Shell_NotifyIconW(NIM_SETVERSION, &nid);
    }
    replace_tray_icon(icon);
}

fn replace_tray_icon(icon: HICON) {
    let old = UI.with(|ui| ui.tray_icon.replace(icon));
    if !old.is_null() {
        unsafe { DestroyIcon(old) };
    }
}

/// Met à jour icône, info-bulle, widget et alerte de batterie faible.
fn on_status(main: HWND) {
    let st = status();
    let icon = tray_icon(&st);
    let mut nid = notify_data(main);
    nid.uFlags = NIF_ICON | NIF_TIP | NIF_SHOWTIP;
    nid.hIcon = icon;
    copy_wide(&mut nid.szTip, &st.tooltip());

    let low = st.link == Link::Online && !st.charging && st.battery <= LOW_BATTERY;
    let warned = UI.with(|ui| ui.low_warned.get());
    if low && !warned {
        nid.uFlags |= NIF_INFO;
        nid.dwInfoFlags = NIIF_WARNING;
        copy_wide(&mut nid.szInfoTitle, "Batterie faible");
        copy_wide(&mut nid.szInfo, &format!("{} : {} % restants. Pensez à la recharger.", st.name(), st.battery));
    }
    UI.with(|ui| {
        // Réarmée après recharge pour ne prévenir qu'une fois par décharge.
        if low {
            ui.low_warned.set(true);
        } else if st.link == Link::Online && (st.charging || st.battery >= LOW_BATTERY + 5) {
            ui.low_warned.set(false);
        }
        ui.polled_at.set(tick());
    });
    unsafe { Shell_NotifyIconW(NIM_MODIFY, &nid) };
    replace_tray_icon(icon);
    let popup = UI.with(|ui| ui.popup.get());
    unsafe { InvalidateRect(popup, null(), 0) };
}

/// Logo MCHOSE avec une jauge colorée en bas ; estompé quand la souris est injoignable.
fn tray_icon(st: &Status) -> HICON {
    let size = unsafe { GetSystemMetrics(SM_CXSMICON) }.max(16) as u32;
    let Some(mut img) = icon::image(size) else { return null_mut() };
    let s = size as i32;
    match st.link {
        Link::Online => {
            let h = (s / 8).max(2);
            let (x0, x1, y0) = (s / 8, s - s / 8, s - s / 8 - h);
            let fill = x0 + ((x1 - x0) * st.battery as i32 + 50) / 100;
            let color = st.battery_color();
            for y in y0..y0 + h {
                for x in x0..x1 {
                    let c = if x < fill.max(x0 + 1) { color } else { 0x4A4B52 };
                    let p = &mut img.bgra[((y * s + x) * 4) as usize..][..4];
                    p.copy_from_slice(&[c as u8, (c >> 8) as u8, (c >> 16) as u8, 255]);
                }
            }
        }
        Link::Pending => {}
        Link::Asleep | Link::Absent => img.bgra.chunks_exact_mut(4).for_each(|p| p[3] = (p[3] as u32 * 2 / 5) as u8),
    }
    make_icon(&img)
}

fn make_icon(img: &icon::Image) -> HICON {
    let s = img.size as i32;
    unsafe {
        let mut bits: *mut c_void = null_mut();
        let bmi = bitmap_info(s, s);
        let color = CreateDIBSection(null_mut(), &bmi, DIB_RGB_COLORS, &mut bits, null_mut(), 0);
        if color.is_null() || bits.is_null() {
            return null_mut();
        }
        std::ptr::copy_nonoverlapping(img.bgra.as_ptr(), bits.cast(), img.bgra.len());
        let mask = CreateBitmap(s, s, 1, 1, null());
        let info = ICONINFO { fIcon: 1, xHotspot: 0, yHotspot: 0, hbmMask: mask, hbmColor: color };
        let icon = CreateIconIndirect(&info);
        DeleteObject(color);
        DeleteObject(mask);
        icon
    }
}

fn bitmap_info(w: i32, h: i32) -> BITMAPINFO {
    let mut bmi: BITMAPINFO = unsafe { zeroed() };
    bmi.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
    bmi.bmiHeader.biWidth = w;
    bmi.bmiHeader.biHeight = -h; // lignes de haut en bas
    bmi.bmiHeader.biPlanes = 1;
    bmi.bmiHeader.biBitCount = 32;
    bmi
}

fn colorref(rgb: u32) -> COLORREF {
    (rgb >> 16 & 0xFF) | (rgb & 0xFF00) | (rgb & 0xFF) << 16
}

fn open_settings() {
    unsafe {
        let hwnd = FindWindowW(null(), wide(WINDOW_TITLE).as_ptr());
        if !hwnd.is_null() {
            if IsIconic(hwnd) != 0 {
                ShowWindow(hwnd, SW_RESTORE);
            }
            SetForegroundWindow(hwnd);
            return;
        }
    }
    sys::spawn_self(&[]);
}

fn show_menu(main: HWND, x: i32, y: i32) {
    hide_popup();
    unsafe {
        let menu = CreatePopupMenu();
        let item = |id: usize, text: &str, flags: u32| AppendMenuW(menu, MF_STRING | flags, id, wide(text).as_ptr());
        item(CMD_OPEN, "Ouvrir les réglages", 0);
        item(CMD_REFRESH, "Actualiser la batterie", 0);
        AppendMenuW(menu, MF_SEPARATOR, 0, null());
        item(CMD_AUTOSTART, "Lancer au démarrage", if autostart::is_enabled() { MF_CHECKED } else { 0 });
        AppendMenuW(menu, MF_SEPARATOR, 0, null());
        item(CMD_QUIT, "Quitter", 0);
        SetMenuDefaultItem(menu, CMD_OPEN as u32, 0);
        // Sans premier plan, le menu ne se ferme pas quand on clique ailleurs.
        SetForegroundWindow(main);
        let cmd =
            TrackPopupMenu(menu, TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_BOTTOMALIGN, x, y, 0, main, null()) as usize;
        PostMessageW(main, WM_NULL, 0, 0);
        DestroyMenu(menu);
        match cmd {
            CMD_OPEN => open_settings(),
            CMD_REFRESH => request_poll(),
            CMD_AUTOSTART => {
                let _ = autostart::set(!autostart::is_enabled());
            }
            CMD_QUIT => {
                DestroyWindow(main);
            }
            _ => {}
        }
    }
}

unsafe extern "system" fn main_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_TRAY => {
            // NOTIFYICON_VERSION_4 : événement dans LOWORD(lparam), ancre écran dans wparam.
            let (x, y) = ((wparam & 0xFFFF) as i16 as i32, (wparam >> 16 & 0xFFFF) as i16 as i32);
            match (lparam & 0xFFFF) as u32 {
                NIN_SELECT | NIN_KEYSELECT => toggle_popup(),
                WM_CONTEXTMENU => show_menu(hwnd, x, y),
                // Survol : la souris bouge donc est réveillée, on rafraîchit une info-bulle périmée.
                WM_MOUSEMOVE if status().link != Link::Online || stale(30_000) => request_poll_throttled(),
                _ => {}
            }
            0
        }
        WM_SHOW => {
            show_popup();
            0
        }
        WM_STATUS => {
            on_status(hwnd);
            0
        }
        WM_DESTROY => {
            let nid = notify_data(hwnd);
            unsafe {
                Shell_NotifyIconW(NIM_DELETE, &nid);
                PostQuitMessage(0);
            }
            0
        }
        // Explorer redémarré : l'icône a disparu, on la remet.
        _ if msg == UI.with(|ui| ui.taskbar_created.get()) && msg != 0 => {
            add_icon(hwnd);
            0
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

fn toggle_popup() {
    let (popup, hidden_at) = UI.with(|ui| (ui.popup.get(), ui.hidden_at.get()));
    if unsafe { IsWindowVisible(popup) } != 0 {
        hide_popup();
    } else if tick().saturating_sub(hidden_at) > 300 {
        // Sinon c'est le clic sur l'icône qui vient de fermer le widget : on ne le rouvre pas.
        show_popup();
    }
}

fn hide_popup() {
    let popup = UI.with(|ui| ui.popup.get());
    if unsafe { IsWindowVisible(popup) } != 0 {
        unsafe { ShowWindow(popup, SW_HIDE) };
    }
}

/// Place le widget au-dessus de l'icône, dans la zone de travail de son écran.
fn show_popup() {
    let (main, popup) = UI.with(|ui| (ui.main.get(), ui.popup.get()));
    unsafe {
        let mut anchor = POINT { x: 0, y: 0 };
        let id = NOTIFYICONIDENTIFIER {
            cbSize: size_of::<NOTIFYICONIDENTIFIER>() as u32,
            hWnd: main,
            uID: 1,
            guidItem: zeroed(),
        };
        let mut icon_rect: RECT = zeroed();
        if Shell_NotifyIconGetRect(&id, &mut icon_rect) == 0 {
            anchor = POINT { x: (icon_rect.left + icon_rect.right) / 2, y: icon_rect.top };
        } else {
            GetCursorPos(&mut anchor);
        }
        let monitor = MonitorFromPoint(anchor, MONITOR_DEFAULTTONEAREST);
        let mut mi: MONITORINFO = zeroed();
        mi.cbSize = size_of::<MONITORINFO>() as u32;
        GetMonitorInfoW(monitor, &mut mi);
        let (mut dpi, mut _dpi_y) = (96u32, 96u32);
        GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi, &mut _dpi_y);
        let (w, h) = (scale(POPUP_W, dpi), scale(POPUP_H, dpi));
        let (work, gap) = (mi.rcWork, scale(12, dpi));
        // Barre des tâches en bas (cas courant) : au-dessus de l'icône ; sinon contre le bord concerné.
        let mut x = anchor.x - w / 2;
        let mut y = if anchor.y >= work.bottom { work.bottom - h - gap } else { anchor.y + gap };
        if mi.rcMonitor.top < work.top && anchor.y <= work.top {
            y = work.top + gap;
        }
        if anchor.x <= work.left {
            x = work.left + gap;
        } else if anchor.x >= work.right {
            x = work.right - w - gap;
        }
        x = x.clamp(work.left + gap, work.right - w - gap);
        y = y.clamp(work.top + gap, work.bottom - h - gap);
        UI.with(|ui| ui.hover.set(false));
        SetWindowPos(popup, HWND_TOPMOST, x, y, w, h, SWP_SHOWWINDOW);
        SetForegroundWindow(popup);
    }
    // Lecture fraîche à l'ouverture si la dernière date de plus de 10 s.
    request_poll_throttled();
}

fn scale(v: i32, dpi: u32) -> i32 {
    (v * dpi as i32 + 48) / 96
}

fn button_rect(dpi: u32) -> RECT {
    RECT {
        left: scale(8, dpi),
        top: scale(BUTTON_TOP, dpi),
        right: scale(POPUP_W - 8, dpi),
        bottom: scale(POPUP_H - 8, dpi),
    }
}

unsafe extern "system" fn popup_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96);
    let point = || POINT { x: (lparam & 0xFFFF) as i16 as i32, y: (lparam >> 16 & 0xFFFF) as i16 as i32 };
    match msg {
        WM_PAINT => {
            paint(hwnd, dpi);
            0
        }
        WM_ERASEBKGND => 1,
        WM_ACTIVATE if wparam & 0xFFFF == WA_INACTIVE as usize => {
            UI.with(|ui| ui.hidden_at.set(tick()));
            unsafe { ShowWindow(hwnd, SW_HIDE) };
            0
        }
        WM_KEYDOWN if wparam == VK_ESCAPE as usize => {
            unsafe { ShowWindow(hwnd, SW_HIDE) };
            0
        }
        WM_MOUSEMOVE => {
            let inside = unsafe { PtInRect(&button_rect(dpi), point()) } != 0;
            if UI.with(|ui| ui.hover.replace(inside)) != inside {
                unsafe { InvalidateRect(hwnd, null(), 0) };
            }
            let mut tme = TRACKMOUSEEVENT {
                cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                dwFlags: TME_LEAVE,
                hwndTrack: hwnd,
                dwHoverTime: 0,
            };
            unsafe { TrackMouseEvent(&mut tme) };
            0
        }
        WM_MOUSELEAVE => {
            if UI.with(|ui| ui.hover.replace(false)) {
                unsafe { InvalidateRect(hwnd, null(), 0) };
            }
            0
        }
        WM_SETCURSOR if UI.with(|ui| ui.hover.get()) => {
            unsafe { SetCursor(LoadCursorW(null_mut(), IDC_HAND)) };
            1
        }
        WM_LBUTTONUP => {
            if unsafe { PtInRect(&button_rect(dpi), point()) } != 0 {
                unsafe { ShowWindow(hwnd, SW_HIDE) };
                open_settings();
            }
            0
        }
        WM_DPICHANGED => {
            unsafe { InvalidateRect(hwnd, null(), 0) };
            0
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

/// Pixels du DIB (0x00RRGGBB) : formes anti-crénelées dessinées à la main, texte par GDI ensuite.
struct Canvas<'a> {
    px: &'a mut [u32],
    w: i32,
    h: i32,
}

impl Canvas<'_> {
    /// Rectangle arrondi par distance signée, couverture d'un demi-pixel.
    fn round_rect(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32, color: u32) {
        let (cx, cy, hw, hh) = (x + w / 2.0, y + h / 2.0, w / 2.0, h / 2.0);
        let r = r.min(hw).min(hh);
        for py in (y.floor() as i32).max(0)..((y + h).ceil() as i32).min(self.h) {
            for px in (x.floor() as i32).max(0)..((x + w).ceil() as i32).min(self.w) {
                let qx = (px as f32 + 0.5 - cx).abs() - (hw - r);
                let qy = (py as f32 + 0.5 - cy).abs() - (hh - r);
                let d = qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - r;
                let a = (0.5 - d).clamp(0.0, 1.0);
                if a > 0.0 {
                    let p = &mut self.px[(py * self.w + px) as usize];
                    *p = blend(*p, color, a);
                }
            }
        }
    }
}

fn blend(dst: u32, src: u32, a: f32) -> u32 {
    let mix = |shift: u32| {
        let (d, s) = ((dst >> shift & 0xFF) as f32, (src >> shift & 0xFF) as f32);
        ((d + (s - d) * a).round() as u32) << shift
    };
    mix(16) | mix(8) | mix(0)
}

fn font(px: i32, weight: i32, face: &str) -> HFONT {
    let face = wide(face);
    unsafe {
        CreateFontW(
            -px,
            0,
            0,
            0,
            weight,
            0,
            0,
            0,
            DEFAULT_CHARSET as u32,
            0,
            0,
            CLEARTYPE_QUALITY as u32,
            0,
            face.as_ptr(),
        )
    }
}

fn text_width(hdc: HDC, f: HFONT, s: &str) -> i32 {
    let w: Vec<u16> = s.encode_utf16().collect();
    let mut size = SIZE { cx: 0, cy: 0 };
    unsafe {
        SelectObject(hdc, f);
        GetTextExtentPoint32W(hdc, w.as_ptr(), w.len() as i32, &mut size);
    }
    size.cx
}

fn draw_text(hdc: HDC, f: HFONT, s: &str, color: u32, mut r: RECT, flags: u32) {
    let mut w: Vec<u16> = s.encode_utf16().collect();
    unsafe {
        SelectObject(hdc, f);
        SetTextColor(hdc, colorref(color));
        DrawTextW(hdc, w.as_mut_ptr(), w.len() as i32, &mut r, flags | DT_SINGLELINE | DT_NOPREFIX);
    }
}

fn logo(dpi: u32) -> HICON {
    let size = scale(26, dpi);
    let (icon, cached) = UI.with(|ui| ui.logo.get());
    if cached == size && !icon.is_null() {
        return icon;
    }
    let new = icon::image(size as u32).map_or(null_mut(), |img| make_icon(&img));
    if !icon.is_null() {
        unsafe { DestroyIcon(icon) };
    }
    UI.with(|ui| ui.logo.set((new, size)));
    new
}

fn paint(hwnd: HWND, dpi: u32) {
    let st = status();
    let hover = UI.with(|ui| ui.hover.get());
    let s = |v: i32| scale(v, dpi);
    let sf = |v: f32| v * dpi as f32 / 96.0;
    unsafe {
        let mut ps: PAINTSTRUCT = zeroed();
        let hdc = BeginPaint(hwnd, &mut ps);
        let mut rc: RECT = zeroed();
        GetClientRect(hwnd, &mut rc);
        let (w, h) = (rc.right, rc.bottom);
        let mem = CreateCompatibleDC(hdc);
        let mut bits: *mut c_void = null_mut();
        let dib = CreateDIBSection(hdc, &bitmap_info(w, h), DIB_RGB_COLORS, &mut bits, null_mut(), 0);
        if dib.is_null() || bits.is_null() {
            DeleteDC(mem);
            EndPaint(hwnd, &ps);
            return;
        }
        let old_bmp = SelectObject(mem, dib);
        SetBkMode(mem, TRANSPARENT as i32);

        let f_title = font(s(15), 600, "Segoe UI");
        let f_big = font(s(36), 600, "Segoe UI");
        let f_body = font(s(13), 400, "Segoe UI");
        let f_small = font(s(12), 400, "Segoe UI");
        let f_icons = font(s(14), 400, "Segoe Fluent Icons");

        let online = st.link == Link::Online;
        let (big, state, state_color) = match st.link {
            Link::Pending => ("…".to_string(), "Lecture en cours", DIM),
            Link::Online if st.charging => (format!("{} %", st.battery), "En charge", GREEN),
            Link::Online if st.battery <= LOW_BATTERY => (format!("{} %", st.battery), "Batterie faible", RED),
            Link::Online => (format!("{} %", st.battery), "Sur batterie", TEXT),
            Link::Asleep => ("—".to_string(), "Déconnectée ou en veille", DIM),
            Link::Absent => ("—".to_string(), "Récepteur introuvable", DIM),
        };
        let pill = match st.link {
            Link::Online if st.wireless => "2.4G",
            Link::Online => "Filaire",
            Link::Pending => "…",
            _ => "Hors ligne",
        };
        let pill_w = text_width(mem, f_small, pill) + s(20);
        let big_w = text_width(mem, f_big, &big);

        // Formes : fond, pastille de connexion, jauge, séparateur, bouton survolé.
        let mut canvas = Canvas { px: std::slice::from_raw_parts_mut(bits.cast(), (w * h) as usize), w, h };
        canvas.px.fill(BG);
        let pill_x = (w - s(16) - pill_w) as f32;
        canvas.round_rect(pill_x, sf(18.0), pill_w as f32, sf(22.0), sf(11.0), FIELD);
        let (bar_x, bar_y, bar_w, bar_h) = (sf(16.0), sf(112.0), w as f32 - sf(32.0), sf(8.0));
        canvas.round_rect(bar_x, bar_y, bar_w, bar_h, bar_h / 2.0, FIELD);
        if online && st.battery > 0 {
            let fill = (bar_w * st.battery as f32 / 100.0).max(bar_h);
            canvas.round_rect(bar_x, bar_y, fill, bar_h, bar_h / 2.0, st.battery_color());
        }
        canvas.round_rect(sf(16.0), sf(BUTTON_TOP as f32 - 7.0), w as f32 - sf(32.0), 1.0, 0.0, LINE);
        let br = button_rect(dpi);
        if hover {
            canvas.round_rect(
                br.left as f32,
                br.top as f32,
                (br.right - br.left) as f32,
                (br.bottom - br.top) as f32,
                sf(8.0),
                HOVER,
            );
        }
        // Pastille d'état à gauche du libellé.
        let dot_x = (s(16) + big_w + s(14)) as f32;
        canvas.round_rect(
            dot_x,
            sf(85.0),
            sf(8.0),
            sf(8.0),
            sf(4.0),
            if online { st.battery_color() } else { 0x5A5D66 },
        );
        GdiFlush();

        // Logo, textes.
        let logo = logo(dpi);
        if !logo.is_null() {
            DrawIconEx(mem, s(16), s(16), logo, s(26), s(26), 0, null_mut(), DI_NORMAL);
        }
        let row = |l: i32, t: i32, r: i32, b: i32| RECT { left: l, top: t, right: r, bottom: b };
        draw_text(
            mem,
            f_title,
            st.name(),
            TEXT,
            row(s(52), s(13), pill_x as i32 - s(8), s(33)),
            DT_LEFT | DT_END_ELLIPSIS,
        );
        let sub = match st.link {
            Link::Pending => "Recherche de la souris…".to_string(),
            _ => format!("Actualisé à {:02}:{:02}", st.at.0, st.at.1),
        };
        draw_text(mem, f_small, &sub, DIM, row(s(52), s(32), w - s(16), s(48)), DT_LEFT);
        draw_text(
            mem,
            f_small,
            pill,
            if online { TEXT } else { DIM },
            row(pill_x as i32, s(18), pill_x as i32 + pill_w, s(40)),
            DT_CENTER | DT_VCENTER,
        );
        draw_text(mem, f_big, &big, TEXT, row(s(14), s(52), s(16) + big_w + s(4), s(104)), DT_LEFT | DT_VCENTER);
        let label_x = dot_x as i32 + s(14);
        draw_text(mem, f_small, "Batterie", DIM, row(label_x, s(62), w - s(16), s(80)), DT_LEFT);
        draw_text(
            mem,
            f_body,
            state,
            if online { state_color } else { DIM },
            row(label_x, s(78), w - s(16), s(100)),
            DT_LEFT | DT_END_ELLIPSIS,
        );
        // Bouton : engrenage, libellé, chevron (Segoe Fluent Icons, Windows 11).
        let (bt, bb) = (br.top, br.bottom);
        draw_text(mem, f_icons, "\u{E713}", ACCENT, row(s(20), bt, s(44), bb), DT_LEFT | DT_VCENTER);
        draw_text(
            mem,
            f_body,
            "Ouvrir les réglages",
            if hover { 0x7AAAFF } else { ACCENT },
            row(s(44), bt, w - s(40), bb),
            DT_LEFT | DT_VCENTER,
        );
        draw_text(mem, f_icons, "\u{E76C}", DIM, row(w - s(44), bt, w - s(20), bb), DT_RIGHT | DT_VCENTER);

        BitBlt(hdc, 0, 0, w, h, mem, 0, 0, SRCCOPY);
        SelectObject(mem, old_bmp);
        for f in [f_title, f_big, f_body, f_small, f_icons] {
            DeleteObject(f);
        }
        DeleteObject(dib);
        DeleteDC(mem);
        EndPaint(hwnd, &ps);
    }
}
