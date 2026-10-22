#![windows_subsystem = "windows"]

mod gui;
mod dock;
mod icon;
mod protocol;
mod sys;
mod tray;

use protocol::Mouse;
use std::process::ExitCode;

const HELP: &str = "\
mchose-lite — remplaçant léger de MCHOSE HUB

  mchose-lite              ouvre la fenêtre de réglages
  mchose-lite status       affiche batterie, firmware et réglages
  mchose-lite profile N    active le profil embarqué N (1-3)
  mchose-lite tray         icône de batterie dans la zone de notification

Les réglages sont stockés dans la souris : seule l'icône de batterie (facultative) reste en fond.";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let window = match args.first().map(String::as_str) {
        None => Some(gui::run()),
        Some("tray") => Some(tray::run()),
        _ => None,
    };
    if let Some(result) = window {
        return if result.is_ok() { ExitCode::SUCCESS } else { ExitCode::FAILURE };
    }
    attach_console();
    let result = match args[0].as_str() {
        "status" => status(),
        "profile" => match args.get(1).and_then(|v| v.parse::<u8>().ok()).filter(|v| (1..=3).contains(v)) {
            Some(n) => Mouse::open().and_then(|m| m.set_profile(n - 1)).map_err(|e| e.to_string()),
            None => Err("usage : mchose-lite profile <1-3>".into()),
        },
        _ => {
            println!("{HELP}");
            Ok(())
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("erreur : {e}");
            ExitCode::FAILURE
        }
    }
}

fn status() -> Result<(), String> {
    let m = Mouse::open().map_err(|e| e.to_string())?;
    let c = m.config().map_err(|e| e.to_string())?;
    let i = &m.info;
    let w = i.wireless;
    println!("{} ({})  firmware {}", i.name, if w { "2.4G" } else { "filaire" }, i.firmware);
    println!("batterie   : {}%{}", i.battery, if i.charging { " (en charge)" } else { "" });
    println!("profil     : {}", c.profile + 1);
    let stages: Vec<String> = c.dpi[..c.dpi_stages as usize]
        .iter()
        .enumerate()
        .map(|(n, v)| if n as u8 == c.dpi_index(w) { format!("[{v}]") } else { v.to_string() })
        .collect();
    println!("DPI        : {}", stages.join(" "));
    let rate = i.rates().get(c.rate_index(w) as usize).copied().unwrap_or(0);
    println!("polling    : {rate} Hz");
    println!("LOD        : {} mm", c.lod);
    println!("ripple {} | angle snap {} | motion sync {} | mode perf {}", c.ripple, c.angle_snap, c.motion_sync, c.perf_mode);
    println!("debounce   : {} ms", c.debounce_ms);
    match c.sleep_min {
        0 => println!("veille     : jamais"),
        v => println!("veille     : {v} min"),
    }
    Ok(())
}

/// Le binaire est en sous-système GUI : on se rattache à la console parente pour le mode CLI.
fn attach_console() {
    use windows_sys::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole};
    unsafe {
        AttachConsole(ATTACH_PARENT_PROCESS);
    }
}
