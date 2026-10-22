use crate::{
    dock::{self, Dock, Lighting},
    icon,
    protocol::{Config, Info, Mouse},
    sys::{self, NamedMutex, autostart},
};
use egui::{
    Align, Align2, Button, Color32, CornerRadius, CursorIcon, FontData, FontDefinitions, FontFamily, FontId, Frame,
    Layout, Margin, Rect, Response, RichText, Sense, Stroke, StrokeKind, TextStyle, TextureHandle, Ui, vec2,
};
use egui_software_backend::{SoftwareBackend, SoftwareBackendAppConfiguration};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

/// Titre de la fenêtre, aussi utilisé par l'icône de notification pour la retrouver.
pub const WINDOW_TITLE: &str = "MCHOSE Lite";
const PERF_MODES: [(u8, &str); 3] = [(1, "Standard"), (2, "Rapide"), (3, "Ultra")];
const BATTERY_REFRESH: Duration = Duration::from_secs(60);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page { Mouse, Dock }

#[derive(Clone, Copy)]
enum TabIcon { Mouse, Dock }

// Palette sombre inspirée de MCHOSE HUB (bleu de marque #0053E2).
const BG: Color32 = Color32::from_rgb(0x12, 0x13, 0x16);
const CARD: Color32 = Color32::from_rgb(0x1B, 0x1C, 0x20);
const BORDER: Color32 = Color32::from_rgb(0x2A, 0x2B, 0x31);
const FIELD: Color32 = Color32::from_rgb(0x26, 0x27, 0x2D);
const FIELD_HOVER: Color32 = Color32::from_rgb(0x31, 0x32, 0x39);
const TEXT: Color32 = Color32::from_rgb(0xEC, 0xEE, 0xF2);
const DIM: Color32 = Color32::from_rgb(0x9A, 0x9E, 0xA8);
const FAINT: Color32 = Color32::from_rgb(0x5E, 0x62, 0x6C);
const ACCENT: Color32 = Color32::from_rgb(0x00, 0x53, 0xE2);
const ACCENT_HI: Color32 = Color32::from_rgb(0x4C, 0x8D, 0xFF);
const ACCENT_TINT: Color32 = Color32::from_rgb(0x15, 0x26, 0x4A);
const GREEN: Color32 = Color32::from_rgb(0x3D, 0xD6, 0x8C);
const AMBER: Color32 = Color32::from_rgb(0xF5, 0xB5, 0x44);
const RED: Color32 = Color32::from_rgb(0xEA, 0x5E, 0x56);

pub fn run() -> Result<(), String> {
    if bring_to_front() {
        return Ok(());
    }
    let icon =
        icon::image(32).map(|img| Arc::new(egui::IconData { rgba: img.to_rgba(), width: img.size, height: img.size }));
    let settings = SoftwareBackendAppConfiguration::new()
        .inner_size(Some(vec2(820.0, 860.0)))
        .min_inner_size(Some(vec2(820.0, 540.0)))
        .resizable(Some(true))
        .title(Some(WINDOW_TITLE.to_string()))
        .icon(icon)
        // Sans cache de rastérisation : ~7 Mo privés de moins, ~3 ms par image (UI statique).
        .caching(false);
    egui_software_backend::run_app_with_software_backend(settings, |ctx| {
        setup_style(&ctx);
        // `request_repaint_after` redessine immédiatement avec ce backend (boucle infinie) :
        // un thread endormi réveille la fenêtre pour relire la batterie.
        let waker = ctx.clone();
        std::thread::spawn(move || {
            loop {
                std::thread::sleep(BATTERY_REFRESH);
                waker.request_repaint();
            }
        });
        App::connect(&ctx)
    })
    .map_err(|e| format!("{e:?}"))
}

/// Une seule fenêtre de réglages : si elle existe déjà, on la ramène au premier plan.
fn bring_to_front() -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        FindWindowW, IsIconic, SW_RESTORE, SetForegroundWindow, ShowWindow,
    };
    unsafe {
        let hwnd = FindWindowW(std::ptr::null(), sys::wide(WINDOW_TITLE).as_ptr());
        if hwnd.is_null() {
            return false;
        }
        if IsIconic(hwnd) != 0 {
            ShowWindow(hwnd, SW_RESTORE);
        }
        SetForegroundWindow(hwnd);
    }
    true
}

fn semibold() -> FontFamily {
    FontFamily::Name("semibold".into())
}

fn strong(text: impl Into<String>, size: f32) -> RichText {
    RichText::new(text).size(size).family(semibold()).color(TEXT)
}

fn dim(text: impl Into<String>) -> RichText {
    RichText::new(text).size(12.5).color(DIM)
}

/// Segoe UI (mappé depuis C:\Windows\Fonts, donc hors mémoire privée), polices egui en secours.
fn setup_style(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    let dir =
        std::path::PathBuf::from(std::env::var_os("WINDIR").unwrap_or_else(|| "C:\\Windows".into())).join("Fonts");
    let fallback = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    let mut regular = fallback.clone();
    let mut bold = fallback.clone();
    if let Some(data) = sys::map_file(&dir.join("segoeui.ttf")) {
        fonts.font_data.insert("segoe".into(), Arc::new(FontData::from_static(data)));
        regular.insert(0, "segoe".into());
        bold.insert(0, "segoe".into());
    }
    if let Some(data) = sys::map_file(&dir.join("seguisb.ttf")) {
        fonts.font_data.insert("segoe-sb".into(), Arc::new(FontData::from_static(data)));
        bold.insert(0, "segoe-sb".into());
    }
    fonts.families.insert(FontFamily::Proportional, regular);
    fonts.families.insert(semibold(), bold);
    ctx.set_fonts(fonts);

    ctx.set_theme(egui::Theme::Dark);
    ctx.all_styles_mut(|style| {
        style.visuals = egui::Visuals::dark();
        let v = &mut style.visuals;
        v.panel_fill = BG;
        v.window_fill = CARD;
        v.extreme_bg_color = FIELD;
        v.faint_bg_color = CARD;
        v.override_text_color = None;
        v.selection.bg_fill = ACCENT;
        v.selection.stroke = stroke(1.0, Color32::WHITE);
        v.slider_trailing_fill = true;
        v.handle_shape = egui::style::HandleShape::Circle;
        v.interact_cursor = Some(CursorIcon::PointingHand);
        let radius = CornerRadius::same(8);
        let w = &mut v.widgets;
        w.noninteractive.fg_stroke = stroke(1.0, TEXT);
        w.noninteractive.bg_stroke = stroke(1.0, BORDER);
        for (state, fill, border) in [
            (&mut w.inactive, FIELD, Stroke::NONE),
            (&mut w.hovered, FIELD_HOVER, stroke(1.0, Color32::from_rgb(0x3E, 0x40, 0x48))),
            (&mut w.active, FIELD_HOVER, stroke(1.0, ACCENT_HI)),
            (&mut w.open, FIELD_HOVER, Stroke::NONE),
        ] {
            state.bg_fill = fill;
            state.weak_bg_fill = fill;
            state.bg_stroke = border;
            state.corner_radius = radius;
            state.fg_stroke = stroke(1.5, TEXT);
            state.expansion = 0.0;
        }
        // Poignée de slider : blanche, comme dans MCHOSE HUB.
        w.inactive.fg_stroke = stroke(1.5, TEXT);

        let s = &mut style.spacing;
        s.item_spacing = vec2(8.0, 8.0);
        s.button_padding = vec2(14.0, 6.0);
        s.interact_size = vec2(40.0, 24.0);
        s.slider_rail_height = 4.0;
        s.scroll = egui::style::ScrollStyle::solid();
        style.interaction.selectable_labels = false;
        style.text_styles = [
            (TextStyle::Small, FontId::new(12.0, FontFamily::Proportional)),
            (TextStyle::Body, FontId::new(14.0, FontFamily::Proportional)),
            (TextStyle::Button, FontId::new(14.0, FontFamily::Proportional)),
            (TextStyle::Heading, FontId::new(20.0, semibold())),
            (TextStyle::Monospace, FontId::new(13.0, FontFamily::Monospace)),
        ]
        .into();
    });
}

struct App {
    page: Page,
    dock: Option<Dock>,
    dock_device: Option<Lighting>,
    dock_edit: Option<Lighting>,
    dock_message: Option<(String, bool)>,
    mouse: Option<Mouse>,
    /// Config telle que lue dans la souris.
    device: Option<Config>,
    /// Config en cours d'édition.
    edit: Option<Config>,
    separate_y: bool,
    message: Option<(String, bool)>,
    autostart: bool,
    /// Récepteur trouvé mais souris en veille : on n'affiche pas sa config (que des zéros).
    asleep: bool,
    logo: TextureHandle,
    battery_at: Instant,
}

impl App {
    fn connect(ctx: &egui::Context) -> Self {
        let img = icon::image(48);
        let image = match &img {
            Some(i) => egui::ColorImage::from_rgba_unmultiplied([i.size as usize; 2], &i.to_rgba()),
            None => egui::ColorImage::filled([1, 1], Color32::TRANSPARENT),
        };
        let logo = ctx.load_texture("logo", image, egui::TextureOptions::LINEAR);
        let mut app = Self {
            page: Page::Mouse,
            dock: None,
            dock_device: None,
            dock_edit: None,
            dock_message: None,
            mouse: None,
            device: None,
            edit: None,
            separate_y: false,
            message: None,
            autostart: autostart::is_enabled(),
            asleep: false,
            logo,
            battery_at: Instant::now(),
        };
        app.reload();
        app.reload_dock();
        if app.mouse.is_none() && app.dock.is_some() { app.page = Page::Dock; }
        app
    }

    fn reload_dock(&mut self) {
        let result = {
            let _lock = NamedMutex::hid();
            Dock::open().and_then(|dock| dock.lighting().map(|lighting| (dock, lighting)))
        };
        match result {
            Ok((dock, lighting)) => {
                self.dock = Some(dock);
                self.dock_device = Some(lighting.clone());
                self.dock_edit = Some(lighting);
                self.dock_message = None;
            }
            Err(e) => {
                self.dock = None;
                self.dock_device = None;
                self.dock_edit = None;
                self.dock_message = Some((e.to_string(), true));
            }
        }
    }

    fn apply_dock(&mut self) {
        let (Some(dock), Some(lighting)) = (&self.dock, &self.dock_edit) else { return };
        let result = {
            let _lock = NamedMutex::hid();
            dock.set_lighting(lighting)
        };
        match result {
            Ok(actual) => {
                self.dock_device = Some(actual.clone());
                self.dock_edit = Some(actual);
                self.dock_message = Some(("Éclairage enregistré dans le dock".into(), false));
            }
            Err(e) => self.dock_message = Some((e.to_string(), true)),
        }
    }

    fn reload(&mut self) {
        let _lock = NamedMutex::hid();
        if self.mouse.is_none() {
            match Mouse::open() {
                Ok(m) => self.mouse = Some(m),
                Err(e) => {
                    self.asleep = false;
                    return self.fail(e.to_string());
                }
            }
        }
        let Some(mouse) = self.mouse.as_mut() else { return };
        let result = mouse.refresh_info().and_then(|()| mouse.config());
        self.battery_at = Instant::now();
        self.asleep = mouse.info.offline();
        if self.asleep {
            // Infos lues à l'ouverture fausses aussi (firmware vide…) : on rouvrira au réveil.
            self.mouse = None;
            return;
        }
        match result {
            Ok(cfg) => {
                self.separate_y = cfg.dpi != cfg.dpi_y;
                self.edit = Some(cfg.clone());
                self.device = Some(cfg);
                self.message = None;
            }
            Err(e) => {
                self.mouse = None;
                self.fail(e.to_string());
            }
        }
    }

    fn fail(&mut self, msg: String) {
        self.message = Some((msg, true));
    }

    fn apply(&mut self) {
        let (Some(mouse), Some(old), Some(new)) = (&self.mouse, &self.device, &mut self.edit) else { return };
        if !self.separate_y {
            new.dpi_y = new.dpi;
        }
        let result = {
            let _lock = NamedMutex::hid();
            mouse.apply(old, new)
        };
        match result {
            Ok(()) => {
                self.reload();
                if self.mouse.is_some() && !self.asleep && self.message.is_none() {
                    self.message = Some(("Réglages enregistrés dans la souris".into(), false));
                }
            }
            Err(e) => self.fail(e.to_string()),
        }
    }

    fn switch_profile(&mut self, index: u8) {
        if let Some(m) = &self.mouse {
            let _lock = NamedMutex::hid();
            if let Err(e) = m.set_profile(index) {
                return self.fail(e.to_string());
            }
        }
        self.reload();
    }

    /// Batterie relue toutes les minutes tant que la fenêtre est ouverte (erreurs ignorées).
    fn refresh_battery(&mut self) {
        // Marge de 5 s : le réveil du thread peut précéder légèrement l'échéance.
        if self.battery_at.elapsed() >= BATTERY_REFRESH - Duration::from_secs(5) {
            // Souris en veille : nouvelle tentative, la fenêtre se remplit seule au réveil.
            if self.mouse.is_none() && self.asleep {
                return self.reload();
            }
            if let Some(m) = self.mouse.as_mut() {
                let _lock = NamedMutex::hid();
                if m.refresh_battery().is_ok() && m.info.offline() {
                    self.asleep = true;
                    self.mouse = None;
                }
            }
            self.battery_at = Instant::now();
        }
    }

    fn set_autostart(&mut self, on: bool) {
        match autostart::set(on) {
            Ok(()) => {
                // Activation : on lance aussi l'icône tout de suite.
                if on && !sys::tray_running() {
                    sys::spawn_self(&["tray"]);
                }
            }
            Err(e) => self.fail(e),
        }
        self.autostart = autostart::is_enabled();
    }
}

impl egui_software_backend::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _backend: &mut SoftwareBackend) {
        self.refresh_battery();
        let mut action = Action::None;
        let dirty = match self.page {
            Page::Mouse => self.mouse.is_some() && self.edit != self.device,
            Page::Dock => self.dock.is_some() && self.dock_edit != self.dock_device,
        };

        // Barre d'actions seulement avec une souris connectée.
        if self.mouse.is_some() || self.page == Page::Dock {
            egui::Panel::bottom("actions")
                .frame(Frame::new().fill(CARD).inner_margin(Margin::symmetric(20, 12)))
                .show_inside(ui, |ui| {
                    // Filet supérieur seulement.
                    let r = ui.max_rect().expand2(vec2(20.0, 12.0));
                    ui.painter().hline(r.x_range(), r.top(), stroke(1.0, BORDER));
                    self.footer(ui, dirty, &mut action)
                });
        }

        egui::CentralPanel::default().frame(Frame::new().fill(BG).inner_margin(Margin::same(20))).show_inside(
            ui,
            |ui| {
                ui.add_space(36.0);
                let info = self.mouse.as_ref().map(|m| m.info.clone());
                self.header(ui, info.as_ref());
                ui.add_space(16.0);
                egui::ScrollArea::vertical()
                    .id_salt(match self.page { Page::Mouse => "mouse-settings", Page::Dock => "dock-settings" })
                    .max_height(ui.available_height())
                    .auto_shrink([false, false]).show(ui, |ui| {
                    if self.page == Page::Dock {
                        self.dock_page(ui, &mut action);
                    } else if let Some(info) = info {
                        ui.spacing_mut().item_spacing.x = 16.0;
                        ui.columns_const(|[left, right]| {
                            left.spacing_mut().item_spacing = vec2(8.0, 8.0);
                            right.spacing_mut().item_spacing = vec2(8.0, 8.0);
                            self.left_column(left, &info, &mut action);
                            self.right_column(right, &mut action);
                        });
                    } else {
                        self.disconnected(ui, &mut action);
                    }
                    ui.add_space(12.0);
                });
            },
        );

        match action {
            Action::None => {}
            Action::Profile(p) => self.switch_profile(p),
            Action::Apply => match self.page { Page::Mouse => self.apply(), Page::Dock => self.apply_dock() },
            Action::Reload => match self.page { Page::Mouse => self.reload(), Page::Dock => self.reload_dock() },
            Action::Autostart(on) => self.set_autostart(on),
        }
    }
}

enum Action {
    None,
    Profile(u8),
    Apply,
    Reload,
    Autostart(bool),
}

impl App {
    fn dock_page(&mut self, ui: &mut Ui, action: &mut Action) {
        if self.dock_edit.is_none() {
            card(ui, "MagDock non disponible", "", |ui| {
                ui.label(dim("Branchez le dock sur un port USB, puis cliquez sur Réessayer."));
                if let Some((message, _)) = &self.dock_message { ui.label(dim(message)); }
                ui.add_space(8.0);
                if primary_button(ui, "Réessayer", true).clicked() { *action = Action::Reload; }
            });
            return;
        }
        ui.spacing_mut().item_spacing.x = 16.0;
        ui.columns_const(|[left, right]| {
            left.spacing_mut().item_spacing = vec2(8.0, 8.0);
            right.spacing_mut().item_spacing = vec2(8.0, 8.0);
            let Some(lighting) = self.dock_edit.as_mut() else { return };
            card(left, "Aperçu", "", |ui| {
                dock_preview(ui, lighting);
                ui.horizontal(|ui| {
                    ui.label(strong(lighting.effect_name(), 14.0));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(dim(if lighting.enabled { "Allumé" } else { "Éteint" }));
                    });
                });
            });
            card(left, "Couleur", "", |ui| {
                let custom = lighting.custom_color();
                ui.add_enabled_ui(lighting.enabled && custom, |ui| {
                    ui.horizontal(|ui| {
                        ui.color_edit_button_srgb(&mut lighting.color);
                        ui.label(RichText::new(format!("#{:02X}{:02X}{:02X}", lighting.color[0], lighting.color[1], lighting.color[2])).monospace().color(TEXT));
                    });
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        for (name, color) in [
                            ("Bleu", [0, 83, 226]), ("Cyan", [0, 210, 230]), ("Violet", [155, 80, 255]),
                            ("Rose", [255, 70, 160]), ("Rouge", [255, 50, 45]), ("Blanc", [255, 255, 255]),
                        ] {
                            let selected = lighting.color == color;
                            let button = Button::new("").fill(Color32::from_rgb(color[0], color[1], color[2]))
                                .stroke(if selected { stroke(2.0, TEXT) } else { Stroke::NONE })
                                .corner_radius(7).min_size(vec2(32.0, 26.0));
                            if ui.add(button).on_hover_text(name).clicked() { lighting.color = color; }
                        }
                    });
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        for (label, channel) in ["R", "V", "B"].into_iter().zip(&mut lighting.color) {
                            ui.label(dim(label));
                            ui.add(egui::DragValue::new(channel).range(0..=255).speed(1.0));
                        }
                    });
                });
                if !custom {
                    ui.add_space(8.0);
                    ui.label(dim("Choisissez Fixe ou Respiration pour personnaliser la couleur."));
                }
            });
            card(right, "Réglages de l'éclairage", "", |ui| {
                toggle_row(ui, "Éclairage", "Allumer ou éteindre le dock", &mut lighting.enabled);
                separator(ui);
                ui.add_enabled_ui(lighting.enabled, |ui| {
                    field_label(ui, "Effet", "");
                    let previous_mode = lighting.mode;
                    egui::ComboBox::from_id_salt("dock-effect").width(ui.available_width())
                        .selected_text(lighting.effect_name()).show_ui(ui, |ui| {
                            for &(mode, label) in dock::EFFECTS { ui.selectable_value(&mut lighting.mode, mode, label); }
                        });
                    if previous_mode != lighting.mode { lighting.music_sync = false; }
                    ui.add_space(12.0);
                    let mut brightness = lighting.brightness + 1;
                    slider_row(ui, "Luminosité", "Niveau 1 à 5", &mut brightness, 1..=5, "");
                    lighting.brightness = brightness - 1;
                    if matches!(lighting.mode, 1..=3) {
                        ui.add_space(8.0);
                        let mut speed = 5 - lighting.speed;
                        slider_row(ui, "Vitesse", "Lent → rapide", &mut speed, 1..=5, "");
                        lighting.speed = 5 - speed;
                    }
                });
            });
            self.app_card(right, action);
        });
    }

    fn header(&mut self, ui: &mut Ui, info: Option<&Info>) {
        let dock_page = self.page == Page::Dock;
        let title = if dock_page { "MCHOSE MagDock" } else { info.map_or("MCHOSE Lite", |i| i.name.as_str()) };
        let (header, _) = ui.allocate_exact_size(vec2(ui.available_width(), 64.0), Sense::hover());
        // Centrer le groupe sur la fenêtre, indépendamment de la largeur du nom et de la batterie.
        let navigation = Rect::from_center_size(header.center(), vec2(260.0, 32.0));
        let identity = Rect::from_min_max(header.min, egui::pos2(navigation.left() - 12.0, header.bottom()));
        let status_rect = Rect::from_min_max(egui::pos2(navigation.right() + 12.0, header.top()), header.max);

        let mut left = ui.new_child(egui::UiBuilder::new().id_salt("header-identity")
            .max_rect(identity).layout(Layout::left_to_right(Align::Center)));
        left.set_clip_rect(ui.clip_rect().intersect(identity));
        left.add(egui::Image::new(&self.logo).fit_to_exact_size(vec2(44.0, 44.0)).corner_radius(10));
        left.add_space(4.0);
        left.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            ui.add(egui::Label::new(strong(title, 20.0)).truncate());
            if dock_page {
                ui.label(dim("Éclairage du dock"));
            } else if let Some(info) = info {
                ui.horizontal(|ui| {
                    pill(ui, if info.wireless { "2.4G" } else { "Filaire" }, ACCENT_HI, ACCENT_TINT);
                    ui.label(dim(format!("Firmware {}", info.firmware)));
                });
            } else {
                ui.label(dim("Aucune souris connectée"));
            }
        });

        let mut middle = ui.new_child(egui::UiBuilder::new().id_salt("header-pages")
            .max_rect(navigation).layout(Layout::left_to_right(Align::Center)));
        segmented_with_icons(&mut middle, "page", &mut self.page,
            &[(Page::Mouse, "Souris"), (Page::Dock, "Dock")], Some(&[TabIcon::Mouse, TabIcon::Dock]));

        let mut right = ui.new_child(egui::UiBuilder::new().id_salt("header-status")
            .max_rect(status_rect).layout(Layout::right_to_left(Align::Center)));
        if self.page == Page::Dock && self.dock.is_some() {
            pill(&mut right, "MagDock connecté", GREEN, CARD);
        } else if self.page == Page::Mouse && let Some(info) = info {
            battery(&mut right, info);
        }
    }

    fn disconnected(&mut self, ui: &mut Ui, action: &mut Action) {
        ui.add_space(70.0);
        ui.vertical_centered(|ui| {
            ui.add(egui::Image::new(&self.logo).fit_to_exact_size(vec2(72.0, 72.0)).corner_radius(16));
            ui.add_space(14.0);
            let (title, hint) = if self.asleep {
                ("Souris en veille", "Bougez la souris pour la réveiller, puis cliquez sur Réessayer.")
            } else {
                ("Aucune souris MCHOSE détectée", "Branchez le récepteur 2.4G ou le câble USB.")
            };
            ui.label(strong(title, 22.0));
            ui.add_space(2.0);
            let detail = match &self.message {
                Some((m, _)) if !self.asleep => m.as_str(),
                _ => hint,
            };
            ui.label(dim(detail));
            ui.add_space(16.0);
            if primary_button(ui, "Réessayer", true).clicked() {
                *action = Action::Reload;
            }
        });
        ui.add_space(60.0);
        ui.horizontal(|ui| {
            ui.add_space((ui.available_width() - 400.0) / 2.0);
            ui.vertical(|ui| {
                ui.set_width(400.0);
                self.app_card(ui, action);
            });
        });
    }

    fn left_column(&mut self, ui: &mut Ui, info: &Info, action: &mut Action) {
        let w = info.wireless;
        let Some(cfg) = self.edit.as_mut() else { return };

        card_frame(ui, |ui| {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 1.0;
                    ui.label(strong("Profils", 15.0));
                    ui.label(dim("Stockés dans la souris"));
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.set_width(220.0);
                    let mut p = cfg.profile;
                    if segmented(ui, "profile", &mut p, &[(0u8, "1"), (1, "2"), (2, "3")]) {
                        *action = Action::Profile(p);
                    }
                });
            });
        });

        card(ui, "Performance", "", |ui| {
            let rates = info.rates();
            let mut rate = cfg.rate_index(w);
            field_label(ui, "Taux de rapport", "Hz");
            let labels: Vec<(u8, String)> = rates
                .iter()
                .enumerate()
                .map(|(i, &r)| (i as u8, if r >= 1000 { format!("{}K", r / 1000) } else { r.to_string() }))
                .collect();
            let options: Vec<(u8, &str)> = labels.iter().map(|(i, s)| (*i, s.as_str())).collect();
            if segmented(ui, "rate", &mut rate, &options) {
                if w {
                    cfg.wireless_rate_index = rate
                } else {
                    cfg.usb_rate_index = rate
                }
            }
            ui.add_space(4.0);
            field_label(ui, "Mode de performance", "");
            segmented(ui, "perf", &mut cfg.perf_mode, &PERF_MODES);
        });

        let separate_y = &mut self.separate_y;
        card_frame(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(strong("DPI", 15.0));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    toggle(ui, separate_y);
                    ui.label(RichText::new("Axe Y séparé").color(DIM).size(13.0));
                });
            });
            ui.add_space(2.0);
            let max_dpi = if info.name.contains("Ultra") { 42_000 } else { 26_000 };
            ui.horizontal(|ui| {
                ui.label(RichText::new("Niveaux").color(DIM).size(13.0));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.set_width(250.0);
                    let counts: Vec<(u8, String)> = (1..=6u8).map(|n| (n, n.to_string())).collect();
                    let options: Vec<(u8, &str)> = counts.iter().map(|(n, s)| (*n, s.as_str())).collect();
                    segmented(ui, "stages", &mut cfg.dpi_stages, &options);
                });
            });
            ui.add_space(2.0);

            let mut active = cfg.dpi_index(w).min(cfg.dpi_stages - 1);
            dpi_tiles(ui, cfg, &mut active, *separate_y);
            if w {
                cfg.wireless_dpi_index = active
            } else {
                cfg.usb_dpi_index = active
            }
            ui.add_space(2.0);

            let n = active as usize;
            let axes: &[(&str, bool)] = if *separate_y { &[("X", false), ("Y", true)] } else { &[("", false)] };
            for &(axis, is_y) in axes {
                ui.horizontal(|ui| {
                    let label = if axis.is_empty() {
                        format!("Niveau {}", n + 1)
                    } else {
                        format!("Niveau {} · {axis}", n + 1)
                    };
                    let (r, _) = ui.allocate_exact_size(vec2(86.0, 24.0), Sense::hover());
                    ui.painter().text(r.left_center(), Align2::LEFT_CENTER, label, FontId::proportional(12.5), DIM);
                    let value = if is_y { &mut cfg.dpi_y[n] } else { &mut cfg.dpi[n] };
                    ui.spacing_mut().slider_width = ui.available_width() - 92.0;
                    ui.add(egui::Slider::new(value, 50..=max_dpi).logarithmic(true).step_by(50.0).show_value(false));
                    ui.add_sized([84.0, 26.0], egui::DragValue::new(value).range(50..=max_dpi).speed(50.0));
                });
            }
        });
    }

    fn right_column(&mut self, ui: &mut Ui, action: &mut Action) {
        if let Some(cfg) = self.edit.as_mut() {
            card(ui, "Capteur", "", |ui| {
                ui.horizontal(|ui| {
                    label_block(ui, "Distance de décollage", "Lift-off distance (LOD)");
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.set_width(140.0);
                        segmented(ui, "lod", &mut cfg.lod, &[(1, "1 mm"), (2, "2 mm")]);
                    });
                });
                separator(ui);
                toggle_row(ui, "Ripple control", "Lisse le signal aux DPI élevés", &mut cfg.ripple);
                toggle_row(ui, "Angle snapping", "Redresse les tracés rectilignes", &mut cfg.angle_snap);
                toggle_row(ui, "Motion sync", "Synchronise capteur et rapports USB", &mut cfg.motion_sync);
                separator(ui);
                slider_row(ui, "Anti-rebond", "Délai de debounce des clics", &mut cfg.debounce_ms, 0..=20, "ms");
            });

            card(ui, "Économie d'énergie", "", |ui| {
                let mut auto = cfg.sleep_min > 0;
                if toggle_row(ui, "Mise en veille automatique", "Après une période d'inactivité", &mut auto) {
                    cfg.sleep_min = if auto { 3 } else { 0 };
                }
                if auto {
                    slider_row(ui, "Délai avant veille", "", &mut cfg.sleep_min, 1..=60, "min");
                }
            });
        }
        self.app_card(ui, action);
    }

    fn app_card(&mut self, ui: &mut Ui, action: &mut Action) {
        card(ui, "Application", "", |ui| {
            let mut on = self.autostart;
            if toggle_row(ui, "Icône de batterie au démarrage", "Zone de notification, ~2 Mo de RAM", &mut on) {
                *action = Action::Autostart(on);
            }
            if !sys::tray_running() {
                ui.horizontal(|ui| {
                    if ui
                        .add(Button::new(RichText::new("Afficher l'icône maintenant").color(ACCENT_HI)).frame(false))
                        .clicked()
                    {
                        sys::spawn_self(&["tray"]);
                    }
                });
            }
        });
    }

    fn footer(&self, ui: &mut Ui, dirty: bool, action: &mut Action) {
        ui.horizontal(|ui| {
            let (message, connected, synced) = match self.page {
                Page::Mouse => (&self.message, self.mouse.is_some(), "Réglages synchronisés avec la souris"),
                Page::Dock => (&self.dock_message, self.dock.is_some(), "Éclairage synchronisé avec le dock"),
            };
            let (text, color) = match message {
                Some((m, true)) => (m.clone(), RED),
                Some((m, false)) if !dirty => (m.clone(), GREEN),
                _ if dirty => ("Modifications non appliquées".to_string(), AMBER),
                _ if connected => (synced.to_string(), DIM),
                _ => (String::new(), DIM),
            };
            let message_width = (ui.available_width() - 232.0).max(0.0);
            ui.allocate_ui_with_layout(vec2(message_width, 32.0), Layout::left_to_right(Align::Center), |ui| {
                if !text.is_empty() {
                    let (r, _) = ui.allocate_exact_size(vec2(8.0, 8.0), Sense::hover());
                    ui.painter().circle_filled(r.center(), 4.0, color);
                    ui.add(egui::Label::new(RichText::new(&text).color(if color == DIM { DIM } else { TEXT })).truncate()).on_hover_text(text);
                }
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if primary_button(ui, "Appliquer", dirty).clicked() {
                    *action = Action::Apply;
                }
                if ui.add(Button::new("Recharger").min_size(vec2(100.0, 32.0))).clicked() {
                    *action = Action::Reload;
                }
            });
        });
    }
}

// ---- Widgets ----------------------------------------------------------------------------

fn stroke(width: f32, color: Color32) -> Stroke {
    Stroke::new(width, color)
}

fn card_frame<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    Frame::new()
        .fill(CARD)
        .stroke(stroke(1.0, BORDER))
        .corner_radius(12)
        .inner_margin(Margin::same(16))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        })
        .inner
}

fn card<R>(ui: &mut Ui, title: &str, subtitle: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
    card_frame(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(strong(title, 15.0));
            if !subtitle.is_empty() {
                ui.label(dim(subtitle));
            }
        });
        ui.add_space(4.0);
        add(ui)
    })
}

fn pill(ui: &mut Ui, text: &str, fg: Color32, bg: Color32) {
    let galley = ui.painter().layout_no_wrap(text.into(), FontId::new(12.0, semibold()), fg);
    let (rect, _) = ui.allocate_exact_size(galley.size() + vec2(16.0, 6.0), Sense::hover());
    ui.painter().rect_filled(rect, CornerRadius::same(10), bg);
    ui.painter().galley(rect.center() - galley.size() / 2.0, galley, fg);
}

fn dock_preview(ui: &mut Ui, lighting: &Lighting) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 180.0), Sense::hover());
    let center = rect.center();
    let painter = ui.painter();
    let base = center + vec2(0.0, 51.0);
    painter.add(egui::Shape::ellipse_filled(base + vec2(0.0, 7.0), vec2(100.0, 23.0), BG));
    painter.add(egui::Shape::ellipse_filled(base, vec2(94.0, 22.0), FIELD));
    painter.rect_filled(Rect::from_center_size(center + vec2(0.0, 6.0), vec2(18.0, 82.0)), 8, BORDER);
    painter.circle_filled(center - vec2(0.0, 36.0), 43.0, BG);
    painter.circle_stroke(center - vec2(0.0, 36.0), 43.0, stroke(2.0, FIELD_HOVER));
    painter.circle_stroke(center - vec2(0.0, 36.0), 25.0, stroke(1.0, BORDER));
    let rgb = Color32::from_rgb(lighting.color[0], lighting.color[1], lighting.color[2]);
    let palette = [ACCENT_HI, Color32::from_rgb(0, 215, 220), GREEN, AMBER, RED, Color32::from_rgb(180, 90, 255)];
    for n in 0..60 {
        let start = n as f32 * std::f32::consts::TAU / 60.0;
        let end = (n + 1) as f32 * std::f32::consts::TAU / 60.0;
        let color = if !lighting.enabled { FAINT } else if lighting.custom_color() { rgb } else { palette[n / 10] };
        let point = |angle: f32| base + vec2(angle.cos() * 92.0, angle.sin() * 21.0 + 2.0);
        painter.line_segment([point(start), point(end)], stroke(4.0, color));
    }
    painter.text(center - vec2(0.0, 36.0), Align2::CENTER_CENTER, "M", FontId::new(24.0, semibold()), DIM);
}

fn battery_color(info: &Info) -> Color32 {
    match info.battery {
        _ if info.charging => GREEN,
        0..=15 => RED,
        16..=35 => AMBER,
        _ => GREEN,
    }
}

/// Indicateur de batterie : pictogramme rempli + pourcentage + état.
fn battery(ui: &mut Ui, info: &Info) {
    let color = battery_color(info);
    let (rect, _) = ui.allocate_exact_size(vec2(40.0, 20.0), Sense::hover());
    let body = Rect::from_min_size(rect.min, vec2(36.0, 20.0));
    ui.painter().rect_stroke(body, CornerRadius::same(5), stroke(1.5, DIM), StrokeKind::Inside);
    let nub = Rect::from_min_size(rect.min + vec2(37.0, 6.0), vec2(3.0, 8.0));
    ui.painter().rect_filled(nub, CornerRadius { nw: 0, sw: 0, ne: 2, se: 2 }, DIM);
    let inner = body.shrink(3.5);
    let fill =
        Rect::from_min_size(inner.min, vec2((inner.width() * info.battery as f32 / 100.0).max(2.0), inner.height()));
    ui.painter().rect_filled(fill, CornerRadius::same(2), color);
    ui.add_space(4.0);
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        ui.with_layout(Layout::top_down(Align::Max), |ui| {
            ui.label(strong(format!("{} %", info.battery), 22.0));
            let state = if info.charging { "En charge" } else { "Batterie" };
            ui.label(RichText::new(state).size(12.0).color(if info.charging { GREEN } else { DIM }));
        });
    });
}

fn field_label(ui: &mut Ui, text: &str, unit: &str) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(text).color(DIM).size(13.0));
        if !unit.is_empty() {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(RichText::new(unit).color(FAINT).size(12.0))
            });
        }
    });
}

fn label_block(ui: &mut Ui, title: &str, hint: &str) {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 1.0;
        ui.label(RichText::new(title).color(TEXT));
        if !hint.is_empty() {
            ui.label(RichText::new(hint).size(12.0).color(DIM));
        }
    });
}

fn separator(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter().rect_filled(rect, CornerRadius::ZERO, BORDER);
}

/// Contrôle segmenté pleine largeur ; renvoie `true` si la valeur a changé.
fn segmented<T: PartialEq + Copy>(ui: &mut Ui, id: &str, value: &mut T, options: &[(T, &str)]) -> bool {
    segmented_with_icons(ui, id, value, options, None)
}

fn segmented_with_icons<T: PartialEq + Copy>(
    ui: &mut Ui, id: &str, value: &mut T, options: &[(T, &str)], icons: Option<&[TabIcon]>,
) -> bool {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 32.0), Sense::hover());
    ui.painter().rect_filled(rect, CornerRadius::same(8), FIELD);
    let seg_w = (rect.width() - 4.0) / options.len() as f32;
    let mut changed = false;
    for (i, (v, label)) in options.iter().enumerate() {
        let r = Rect::from_min_size(rect.min + vec2(2.0 + i as f32 * seg_w, 2.0), vec2(seg_w, rect.height() - 4.0));
        let resp = ui.interact(r, ui.id().with((id, i)), Sense::click()).on_hover_cursor(CursorIcon::PointingHand);
        let selected = *value == *v;
        let (fill, fg) = match (selected, resp.hovered()) {
            (true, _) => (ACCENT, Color32::WHITE),
            (false, true) => (FIELD_HOVER, TEXT),
            _ => (Color32::TRANSPARENT, DIM),
        };
        ui.painter().rect_filled(r, CornerRadius::same(6), fill);
        let font = FontId::new(13.5, if selected { semibold() } else { FontFamily::Proportional });
        if let Some(icon) = icons.and_then(|icons| icons.get(i)).copied() {
            let galley = ui.painter().layout_no_wrap((*label).to_owned(), font, fg);
            let start = r.center().x - (16.0 + 7.0 + galley.size().x) * 0.5;
            draw_tab_icon(ui.painter(), egui::pos2(start + 8.0, r.center().y), icon, fg);
            ui.painter().galley(egui::pos2(start + 23.0, r.center().y - galley.size().y * 0.5), galley, fg);
        } else {
            ui.painter().text(r.center(), Align2::CENTER_CENTER, *label, font, fg);
        }
        if resp.clicked() && !selected {
            *value = *v;
            changed = true;
        }
    }
    changed
}

fn draw_tab_icon(painter: &egui::Painter, center: egui::Pos2, icon: TabIcon, color: Color32) {
    let outline = stroke(1.4, color);
    match icon {
        TabIcon::Mouse => {
            let (sin, cos) = (-0.65_f32).sin_cos();
            let point = |x: f32, y: f32| center + vec2(x * cos - y * sin, x * sin + y * cos);
            let mut points = Vec::with_capacity(22);
            for (start, offset) in [(std::f32::consts::PI, -2.2), (0.0, 2.2)] {
                for step in 0..=10 {
                    let angle = start + step as f32 * std::f32::consts::PI / 10.0;
                    points.push(point(angle.cos() * 4.3, offset + angle.sin() * 4.3));
                }
            }
            painter.add(egui::Shape::closed_line(points, outline));
            painter.line_segment([point(-4.3, -1.4), point(4.3, -1.4)], outline);
            painter.line_segment([point(0.0, -6.0), point(0.0, -1.4)], outline);
        }
        TabIcon::Dock => {
            let points = (0..6).map(|n| {
                let angle = n as f32 * std::f32::consts::TAU / 6.0 - std::f32::consts::FRAC_PI_2;
                center + vec2(angle.cos(), angle.sin()) * 7.0
            }).collect();
            painter.add(egui::Shape::closed_line(points, outline));
            painter.circle_stroke(center, 2.3, outline);
        }
    }
}

fn toggle(ui: &mut Ui, on: &mut bool) -> Response {
    let (rect, mut resp) = ui.allocate_exact_size(vec2(38.0, 22.0), Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    let resp = resp.on_hover_cursor(CursorIcon::PointingHand);
    let track = match (*on, resp.hovered()) {
        (true, false) => ACCENT,
        (true, true) => Color32::from_rgb(0x1A, 0x66, 0xF0),
        (false, false) => Color32::from_rgb(0x3A, 0x3B, 0x43),
        (false, true) => Color32::from_rgb(0x46, 0x48, 0x50),
    };
    ui.painter().rect_filled(rect, CornerRadius::same(11), track);
    let x = if *on { rect.right() - 11.0 } else { rect.left() + 11.0 };
    ui.painter().circle_filled(egui::pos2(x, rect.center().y), 8.0, Color32::WHITE);
    resp
}

/// Ligne libellé + description à gauche, interrupteur à droite ; renvoie `true` si basculé.
fn toggle_row(ui: &mut Ui, title: &str, hint: &str, on: &mut bool) -> bool {
    ui.horizontal(|ui| {
        label_block(ui, title, hint);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| toggle(ui, on).changed()).inner
    })
    .inner
}

fn slider_row(ui: &mut Ui, title: &str, hint: &str, value: &mut u8, range: std::ops::RangeInclusive<u8>, unit: &str) {
    ui.horizontal(|ui| {
        label_block(ui, title, hint);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.add_sized([70.0, 26.0], egui::DragValue::new(value).range(range.clone()).suffix(format!(" {unit}")));
            ui.spacing_mut().slider_width = (ui.available_width() - 12.0).clamp(60.0, 150.0);
            ui.add(egui::Slider::new(value, range).show_value(false));
        });
    });
}

/// Six emplacements de niveaux DPI ; clic = niveau actif (et édité). Emplacements inutilisés grisés.
fn dpi_tiles(ui: &mut Ui, cfg: &Config, active: &mut u8, separate_y: bool) {
    let gap = 6.0;
    let w = (ui.available_width() - gap * 5.0) / 6.0;
    let h = if separate_y { 58.0 } else { 50.0 };
    let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), h), Sense::hover());
    for i in 0..6usize {
        let r = Rect::from_min_size(row.min + vec2(i as f32 * (w + gap), 0.0), vec2(w, h));
        let p = ui.painter();
        if i >= cfg.dpi_stages as usize {
            p.rect_stroke(r, CornerRadius::same(8), stroke(1.0, BORDER), StrokeKind::Inside);
            p.text(r.center(), Align2::CENTER_CENTER, "—", FontId::proportional(13.0), FAINT);
            continue;
        }
        let resp =
            ui.interact(r, ui.id().with(("dpi-tile", i)), Sense::click()).on_hover_cursor(CursorIcon::PointingHand);
        if resp.clicked() {
            *active = i as u8;
        }
        let selected = *active as usize == i;
        let (fill, stroke) = match (selected, resp.hovered()) {
            (true, _) => (ACCENT_TINT, stroke(1.5, ACCENT_HI)),
            (false, true) => (FIELD_HOVER, Stroke::NONE),
            _ => (FIELD, Stroke::NONE),
        };
        let p = ui.painter();
        p.rect(r, CornerRadius::same(8), fill, stroke, StrokeKind::Inside);
        p.text(
            r.left_top() + vec2(8.0, 6.0),
            Align2::LEFT_TOP,
            format!("{}", i + 1),
            FontId::proportional(11.0),
            if selected { ACCENT_HI } else { FAINT },
        );
        let value_y = if separate_y { r.center().y + 1.0 } else { r.center().y + 5.0 };
        p.text(
            egui::pos2(r.center().x, value_y),
            Align2::CENTER_CENTER,
            cfg.dpi[i].to_string(),
            FontId::new(15.0, semibold()),
            if selected { Color32::WHITE } else { TEXT },
        );
        if separate_y {
            p.text(
                egui::pos2(r.center().x, r.bottom() - 10.0),
                Align2::CENTER_CENTER,
                format!("Y {}", cfg.dpi_y[i]),
                FontId::proportional(11.0),
                DIM,
            );
        }
    }
}

fn primary_button(ui: &mut Ui, text: &str, enabled: bool) -> Response {
    let button = Button::new(RichText::new(text).color(Color32::WHITE).family(semibold()))
        .fill(if enabled { ACCENT } else { FIELD })
        .min_size(vec2(120.0, 32.0));
    ui.add_enabled(enabled, button)
}
