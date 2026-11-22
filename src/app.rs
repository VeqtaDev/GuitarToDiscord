use crate::audio::{self, DeviceInfo, Engine, EngineSettings, InputSel, Shared};
use crate::config::{ChannelSel, Config};
use crate::theme::{self, TEXT, TEXT_DIM};
use crate::vst::{self, PluginInfo};
use eframe::egui::{self, Align, Layout, RichText, ScrollArea, Vec2};
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::Arc;

const VB_CABLE_URL: &str = "https://vb-audio.com/Cable/";

pub struct App {
    cfg: Config,
    shared: Arc<Shared>,
    engine: Option<Engine>,
    error: Option<String>,

    hosts: Vec<String>,
    guitar_inputs: Vec<DeviceInfo>,
    inputs: Vec<DeviceInfo>,
    outputs: Vec<DeviceInfo>,

    plugins: Vec<PluginInfo>,
    plugin_filter: String,
    tab: usize,
    dirty: bool,
    live_anim: f32,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        theme::install(&cc.egui_ctx);
        let cfg = Config::load();
        let mut app = Self {
            cfg,
            shared: Arc::new(Shared::default()),
            engine: None,
            error: None,
            hosts: audio::host_names(),
            guitar_inputs: vec![],
            inputs: vec![],
            outputs: vec![],
            plugins: vec![],
            plugin_filter: String::new(),
            tab: 0,
            dirty: false,
            live_anim: 0.0,
        };
        app.refresh_devices();
        app.autoselect();
        app.rescan_plugins();
        app.push_params();
        if app.cfg.auto_start {
            app.start();
        }
        app
    }

    fn refresh_devices(&mut self) {
        self.hosts = audio::host_names();
        self.guitar_inputs = audio::list_inputs(self.cfg.host.as_deref());
        self.inputs = audio::list_inputs(None);
        self.outputs = audio::list_outputs(None);
    }

    /// First-run guesses: Focusrite/Scarlett for guitar, headset for mic,
    /// VB-Cable for the output.
    fn autoselect(&mut self) {
        if self.cfg.output_device.is_none() {
            self.cfg.output_device = audio::guess_output(&self.outputs);
        }
        if self.cfg.guitar_device.is_none() {
            self.cfg.guitar_device = audio::guess_guitar(&self.guitar_inputs);
        }
        if self.cfg.mic_device.is_none() {
            self.cfg.mic_device = audio::guess_mic(&self.inputs, self.cfg.guitar_device.as_deref());
        }
    }

    fn rescan_plugins(&mut self) {
        let mut dirs = vst::default_dirs();
        dirs.extend(self.cfg.vst_custom_dirs.iter().cloned());
        self.plugins = vst::scan(&dirs);
    }

    fn push_params(&self) {
        let s = &self.shared;
        s.guitar_gain_db.set(self.cfg.guitar_gain_db);
        s.mic_gain_db.set(self.cfg.mic_gain_db);
        s.master_gain_db.set(self.cfg.master_gain_db);
        s.monitor_gain_db.set(self.cfg.monitor_gain_db);
        s.guitar_gate_db.set(self.cfg.guitar_gate_db);
        s.mic_gate_db.set(self.cfg.mic_gate_db);
        s.guitar_muted.store(self.cfg.guitar_muted, Ordering::Relaxed);
        s.mic_muted.store(self.cfg.mic_muted, Ordering::Relaxed);
    }

    fn start(&mut self) {
        self.engine = None;
        let Some(output) = self.cfg.output_device.clone() else {
            self.error = Some("Choisis le périphérique virtuel de sortie (ex : CABLE Input).".into());
            return;
        };
        let settings = EngineSettings {
            guitar: self.cfg.guitar_device.clone().map(|device| InputSel {
                host: self.cfg.host.clone(),
                device,
                channel: self.cfg.guitar_channel,
            }),
            mic: self.cfg.mic_device.clone().map(|device| InputSel { host: None, device, channel: self.cfg.mic_channel }),
            output,
            monitor: if self.cfg.monitor_enabled { self.cfg.monitor_device.clone() } else { None },
        };
        match Engine::start(settings, self.shared.clone()) {
            Ok(e) => {
                self.engine = Some(e);
                self.error = None;
            }
            Err(e) => self.error = Some(format!("{e:#}")),
        }
    }

    fn stop(&mut self) {
        self.engine = None;
        self.shared.guitar_level.set(0.0);
        self.shared.mic_level.set(0.0);
        self.shared.out_level.set(0.0);
    }

    fn restart_if_running(&mut self) {
        if self.engine.is_some() {
            self.start();
        }
    }
}

// ---------------------------------------------------------------------------
// UI
// ---------------------------------------------------------------------------

fn device_combo(ui: &mut egui::Ui, id: &str, value: &mut Option<String>, list: &[DeviceInfo], allow_none: bool) -> bool {
    let mut changed = false;
    let text = value.clone().unwrap_or_else(|| "— Aucun —".into());
    egui::ComboBox::from_id_salt(id)
        .width(ui.available_width())
        .selected_text(text)
        .show_ui(ui, |ui| {
            if allow_none && ui.selectable_label(value.is_none(), "— Aucun —").clicked() {
                *value = None;
                changed = true;
            }
            for d in list {
                let sel = value.as_deref() == Some(d.name.as_str());
                let label = format!("{}   ·  {} ch · {} Hz", d.name, d.channels, d.sample_rate);
                if ui.selectable_label(sel, label).clicked() {
                    *value = Some(d.name.clone());
                    changed = true;
                }
            }
        });
    changed
}

fn channel_combo(ui: &mut egui::Ui, id: &str, value: &mut ChannelSel, channels: u16) -> bool {
    let mut changed = false;
    let text = match value {
        ChannelSel::Mix => "Toutes (mono)".to_owned(),
        ChannelSel::Index(i) => format!("Entrée {}", *i + 1),
    };
    egui::ComboBox::from_id_salt(id).width(150.0).selected_text(text).show_ui(ui, |ui| {
        if ui.selectable_label(*value == ChannelSel::Mix, "Toutes (mono)").clicked() {
            *value = ChannelSel::Mix;
            changed = true;
        }
        for i in 0..channels.max(1) {
            if ui.selectable_label(*value == ChannelSel::Index(i), format!("Entrée {}", i + 1)).clicked() {
                *value = ChannelSel::Index(i);
                changed = true;
            }
        }
    });
    changed
}

fn open_url(url: &str) {
    let _ = std::process::Command::new("explorer").arg(url).spawn();
}

fn open_recording_devices() {
    let _ = std::process::Command::new("control").arg("mmsys.cpl,,1").spawn();
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let t = ctx.input(|i| i.time) as f32;
        let running = self.engine.is_some();
        self.live_anim += ((if running { 1.0 } else { 0.0 }) - self.live_anim) * 0.06;

        if let Ok(mut e) = self.shared.last_error.lock() {
            if let Some(msg) = e.take() {
                self.error = Some(msg);
            }
        }

        let full = ui.max_rect();
        theme::backdrop(ui, full, t, self.live_anim);

        egui::Frame::new().inner_margin(egui::Margin::symmetric(22, 18)).show(ui, |ui| {
            self.header(ui, t);
            ui.add_space(14.0);
            ui.horizontal(|ui| {
                theme::segmented(ui, "tabs", &mut self.tab, &["Routing", "Plugins VST3", "Aide"]);
            });
            ui.add_space(12.0);
            if let Some(err) = self.error.clone() {
                theme::card(ui, 0.9, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("⚠").size(18.0));
                        ui.label(RichText::new(err).color(TEXT));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if theme::chip(ui, "Fermer").clicked() {
                                self.error = None;
                            }
                        });
                    });
                });
                ui.add_space(10.0);
            }
            ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| match self.tab {
                0 => self.routing_tab(ui),
                1 => self.plugins_tab(ui),
                _ => self.help_tab(ui),
            });
        });

        if self.dirty {
            self.push_params();
            self.cfg.save();
            self.dirty = false;
        }
        // Meters and backdrop are animated.
        ctx.request_repaint_after(std::time::Duration::from_millis(16));
    }

    fn on_exit(&mut self) {
        self.cfg.save();
    }
}

impl App {
    fn header(&mut self, ui: &mut egui::Ui, t: f32) {
        let running = self.engine.is_some();
        let h = 64.0;
        let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), h), egui::Sense::hover());
        for s in theme::glass_shapes(rect, h / 2.0, 0.35) {
            ui.painter().add(s);
        }
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink2(Vec2::new(18.0, 10.0))).layout(Layout::left_to_right(Align::Center)));
        let ui = &mut child;
        // Logo mark
        let (lr, _) = ui.allocate_exact_size(Vec2::splat(40.0), egui::Sense::hover());
        ui.painter().add(theme::gradient_rrect(lr, 13.0, |p| {
            let k = ((p.x - lr.left()) + (p.y - lr.top())) / (lr.width() * 2.0);
            if k < 0.5 { theme::RED_HOT } else { theme::RED_DEEP }
        }));
        ui.painter().text(lr.center(), egui::Align2::CENTER_CENTER, "🎸", egui::FontId::proportional(20.0), TEXT);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.add_space(4.0);
            ui.label(RichText::new("Guitar To Discord").font(theme::bold(20.0)).color(TEXT));
            ui.label(RichText::new("Guitare + voix → un seul micro").size(12.5).color(TEXT_DIM));
        });
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let label = if running { "Arrêter" } else { "Démarrer" };
            if theme::pill_button(ui, label, !running, 140.0).clicked() {
                if running {
                    self.stop();
                } else {
                    self.start();
                }
            }
            ui.add_space(6.0);
            ui.label(RichText::new(if running { "En direct" } else { "Arrêté" }).font(theme::bold(13.0)).color(if running { theme::OK } else { TEXT_DIM }));
            theme::status_dot(ui, running, t);
        });
    }

    fn routing_tab(&mut self, ui: &mut egui::Ui) {
        let mut restart = false;
        let col_w = (ui.available_width() - 14.0) / 2.0;
        ui.horizontal_top(|ui| {
            // ---------------- left column: inputs ----------------
            ui.allocate_ui_with_layout(Vec2::new(col_w, 0.0), Layout::top_down(Align::Min), |ui| {
                theme::card(ui, 0.25, |ui| {
                    theme::section_title(ui, "🎸", "Guitare", "Ta carte son (Focusrite, Scarlett…)");
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Pilote").color(TEXT_DIM));
                        let mut host = self.cfg.host.clone().unwrap_or_else(|| self.hosts.first().cloned().unwrap_or_default());
                        let before = host.clone();
                        egui::ComboBox::from_id_salt("host").width(120.0).selected_text(&host).show_ui(ui, |ui| {
                            for h in &self.hosts {
                                ui.selectable_value(&mut host, h.clone(), h);
                            }
                        });
                        if host != before {
                            self.cfg.host = Some(host);
                            self.cfg.guitar_device = None;
                            self.guitar_inputs = audio::list_inputs(self.cfg.host.as_deref());
                            restart = true;
                            self.dirty = true;
                        }
                        if !self.hosts.iter().any(|h| h.eq_ignore_ascii_case("asio")) {
                            ui.label(RichText::new("ASIO : build --features asio").size(11.5).color(TEXT_DIM));
                        }
                    });
                    if device_combo(ui, "guitar_dev", &mut self.cfg.guitar_device, &self.guitar_inputs, true) {
                        restart = true;
                        self.dirty = true;
                    }
                    let ch = self
                        .guitar_inputs
                        .iter()
                        .find(|d| Some(&d.name) == self.cfg.guitar_device.as_ref())
                        .map(|d| d.channels)
                        .unwrap_or(2);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Canal").color(TEXT_DIM));
                        if channel_combo(ui, "guitar_ch", &mut self.cfg.guitar_channel, ch) {
                            restart = true;
                            self.dirty = true;
                        }
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            let mut on = !self.cfg.guitar_muted;
                            if theme::toggle(ui, &mut on).changed() {
                                self.cfg.guitar_muted = !on;
                                self.dirty = true;
                            }
                        });
                    });
                    theme::meter(ui, self.shared.guitar_level.get());
                    if theme::slider(ui, &mut self.cfg.guitar_gain_db, -24.0, 24.0, "Gain", "dB").changed() {
                        self.dirty = true;
                    }
                    if theme::slider(ui, &mut self.cfg.guitar_gate_db, -96.0, -20.0, "Noise gate", "dB").changed() {
                        self.dirty = true;
                    }
                });
                ui.add_space(14.0);
                theme::card(ui, 0.15, |ui| {
                    theme::section_title(ui, "🎙", "Micro", "Ton casque ou ton micro habituel");
                    if device_combo(ui, "mic_dev", &mut self.cfg.mic_device, &self.inputs, true) {
                        restart = true;
                        self.dirty = true;
                    }
                    let ch = self.inputs.iter().find(|d| Some(&d.name) == self.cfg.mic_device.as_ref()).map(|d| d.channels).unwrap_or(1);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Canal").color(TEXT_DIM));
                        if channel_combo(ui, "mic_ch", &mut self.cfg.mic_channel, ch) {
                            restart = true;
                            self.dirty = true;
                        }
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            let mut on = !self.cfg.mic_muted;
                            if theme::toggle(ui, &mut on).changed() {
                                self.cfg.mic_muted = !on;
                                self.dirty = true;
                            }
                        });
                    });
                    theme::meter(ui, self.shared.mic_level.get());
                    if theme::slider(ui, &mut self.cfg.mic_gain_db, -24.0, 24.0, "Gain", "dB").changed() {
                        self.dirty = true;
                    }
                    if theme::slider(ui, &mut self.cfg.mic_gate_db, -96.0, -20.0, "Noise gate", "dB").changed() {
                        self.dirty = true;
                    }
                });
            });
            ui.add_space(14.0);
            // ---------------- right column: output ----------------
            ui.allocate_ui_with_layout(Vec2::new(col_w, 0.0), Layout::top_down(Align::Min), |ui| {
                theme::card(ui, 0.45, |ui| {
                    theme::section_title(ui, "🔊", "Sortie → Discord", "Le micro virtuel que Discord écoute");
                    if device_combo(ui, "out_dev", &mut self.cfg.output_device, &self.outputs, false) {
                        restart = true;
                        self.dirty = true;
                    }
                    let has_cable = self.outputs.iter().any(|d| audio::is_virtual_cable(&d.name));
                    let ok = self.cfg.output_device.as_deref().map(audio::is_virtual_cable).unwrap_or(false);
                    if ok {
                        let side = audio::discord_side_name(self.cfg.output_device.as_deref().unwrap_or_default());
                        ui.label(RichText::new(format!("✔ Câble virtuel détecté. Dans Discord : Paramètres › Voix › Périphérique d'entrée › « {side} ».")).size(12.5).color(TEXT_DIM));
                    } else if has_cable {
                        ui.label(RichText::new("Un câble virtuel existe : sélectionne-le ci-dessus (ex : « CABLE Input »).").size(12.5).color(theme::RED_HOT));
                    } else {
                        ui.label(RichText::new("Aucun câble virtuel trouvé. Installe VB-Cable (gratuit) puis clique sur Actualiser.").size(12.5).color(theme::RED_HOT));
                    }
                    ui.horizontal_wrapped(|ui| {
                        if !has_cable && theme::chip(ui, "Télécharger VB-Cable").clicked() {
                            open_url(VB_CABLE_URL);
                        }
                        if theme::chip(ui, "Renommer en « Guitar To Discord »").clicked() {
                            open_recording_devices();
                        }
                        if theme::chip(ui, "⟳ Actualiser").clicked() {
                            self.refresh_devices();
                            self.autoselect();
                        }
                    });
                    theme::meter(ui, self.shared.out_level.get());
                    if theme::slider(ui, &mut self.cfg.master_gain_db, -24.0, 12.0, "Volume envoyé", "dB").changed() {
                        self.dirty = true;
                    }
                });
                ui.add_space(14.0);
                theme::card(ui, 0.15, |ui| {
                    ui.horizontal(|ui| {
                        theme::section_title(ui, "🎧", "Retour casque", "Entendre ta guitare en direct");
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if theme::toggle(ui, &mut self.cfg.monitor_enabled).changed() {
                                restart = true;
                                self.dirty = true;
                            }
                        });
                    });
                    if self.cfg.monitor_enabled {
                        if device_combo(ui, "mon_dev", &mut self.cfg.monitor_device, &self.outputs, false) {
                            restart = true;
                            self.dirty = true;
                        }
                        if theme::slider(ui, &mut self.cfg.monitor_gain_db, -40.0, 6.0, "Volume retour", "dB").changed() {
                            self.dirty = true;
                        }
                    }
                });
                ui.add_space(14.0);
                theme::card(ui, 0.1, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Démarrer automatiquement au lancement").color(TEXT));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if theme::toggle(ui, &mut self.cfg.auto_start).changed() {
                                self.dirty = true;
                            }
                        });
                    });
                    if let Some(e) = &self.engine {
                        ui.label(RichText::new(&e.summary).size(11.5).color(TEXT_DIM));
                        let u = self.shared.underruns.load(Ordering::Relaxed);
                        if u > 0 {
                            ui.label(RichText::new(format!("Micro-coupures : {u}")).size(11.5).color(TEXT_DIM));
                        }
                    }
                });
            });
        });
        if restart {
            self.cfg.save();
            self.restart_if_running();
        }
    }

    fn plugins_tab(&mut self, ui: &mut egui::Ui) {
        theme::card(ui, 0.3, |ui| {
            theme::section_title(ui, "🧩", "Dossiers VST3", "Dossiers Windows standards + tes dossiers");
            for d in vst::default_dirs() {
                ui.label(RichText::new(format!("•  {}", d.display())).size(12.5).color(TEXT_DIM));
            }
            let mut remove: Option<usize> = None;
            for (i, d) in self.cfg.vst_custom_dirs.iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("•  {}", d.display())).size(12.5).color(TEXT));
                    if theme::chip(ui, "Retirer").clicked() {
                        remove = Some(i);
                    }
                });
            }
            if let Some(i) = remove {
                self.cfg.vst_custom_dirs.remove(i);
                self.dirty = true;
                self.rescan_plugins();
            }
            ui.horizontal(|ui| {
                if theme::chip(ui, "+ Ajouter un dossier").clicked() {
                    if let Some(dir) = rfd::FileDialog::new().set_title("Dossier de plugins VST3").pick_folder() {
                        if !self.cfg.vst_custom_dirs.contains(&dir) {
                            self.cfg.vst_custom_dirs.push(dir);
                            self.dirty = true;
                            self.rescan_plugins();
                        }
                    }
                }
                if theme::chip(ui, "⟳ Rescanner").clicked() {
                    self.rescan_plugins();
                }
            });
        });
        ui.add_space(14.0);

        let col_w = (ui.available_width() - 14.0) / 2.0;
        ui.horizontal_top(|ui| {
            ui.allocate_ui_with_layout(Vec2::new(col_w, 0.0), Layout::top_down(Align::Min), |ui| {
                theme::card(ui, 0.15, |ui| {
                    theme::section_title(ui, "🔎", &format!("{} plugins trouvés", self.plugins.len()), "Clique pour ajouter à la chaîne guitare");
                    ui.add(egui::TextEdit::singleline(&mut self.plugin_filter).hint_text("Rechercher…").desired_width(f32::INFINITY));
                    let f = self.plugin_filter.to_lowercase();
                    let mut add: Option<PathBuf> = None;
                    ScrollArea::vertical().id_salt("plist").max_height(320.0).show(ui, |ui| {
                        for p in self.plugins.iter().filter(|p| f.is_empty() || p.name.to_lowercase().contains(&f) || p.vendor.as_deref().unwrap_or("").to_lowercase().contains(&f)) {
                            ui.horizontal(|ui| {
                                ui.vertical(|ui| {
                                    ui.spacing_mut().item_spacing.y = 0.0;
                                    ui.label(RichText::new(&p.name).color(TEXT));
                                    let sub = [p.vendor.clone(), p.category.clone()].into_iter().flatten().collect::<Vec<_>>().join(" · ");
                                    if !sub.is_empty() {
                                        ui.label(RichText::new(sub).size(11.5).color(TEXT_DIM));
                                    }
                                });
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    if theme::chip(ui, "+").clicked() {
                                        add = Some(p.path.clone());
                                    }
                                });
                            });
                        }
                        if self.plugins.is_empty() {
                            ui.label(RichText::new("Aucun plugin VST3 trouvé. Ajoute ton dossier ci-dessus.").color(TEXT_DIM));
                        }
                    });
                    if let Some(p) = add {
                        self.cfg.vst_chain.push(p);
                        self.dirty = true;
                    }
                });
            });
            ui.add_space(14.0);
            ui.allocate_ui_with_layout(Vec2::new(col_w, 0.0), Layout::top_down(Align::Min), |ui| {
                theme::card(ui, 0.4, |ui| {
                    theme::section_title(ui, "⛓", "Chaîne guitare", "Ordre de traitement, de haut en bas");
                    let mut action: Option<(usize, i32)> = None;
                    for (i, p) in self.cfg.vst_chain.iter().enumerate() {
                        let name = self.plugins.iter().find(|x| &x.path == p).map(|x| x.name.clone()).unwrap_or_else(|| p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default());
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(format!("{}.", i + 1)).font(theme::bold(14.0)).color(theme::RED_HOT));
                            ui.label(RichText::new(name).color(TEXT));
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if theme::chip(ui, "✕").clicked() {
                                    action = Some((i, 0));
                                }
                                if theme::chip(ui, "↓").clicked() {
                                    action = Some((i, 1));
                                }
                                if theme::chip(ui, "↑").clicked() {
                                    action = Some((i, -1));
                                }
                            });
                        });
                    }
                    if let Some((i, d)) = action {
                        match d {
                            0 => {
                                self.cfg.vst_chain.remove(i);
                            }
                            1 if i + 1 < self.cfg.vst_chain.len() => self.cfg.vst_chain.swap(i, i + 1),
                            -1 if i > 0 => self.cfg.vst_chain.swap(i, i - 1),
                            _ => {}
                        }
                        self.dirty = true;
                    }
                    if self.cfg.vst_chain.is_empty() {
                        ui.label(RichText::new("Chaîne vide : le son de ta guitare part brut, en natif.").color(TEXT_DIM));
                    }
                    ui.add_space(6.0);
                    ui.label(RichText::new("Aperçu : le scan et la chaîne sont prêts, le traitement audio VST3 (et l'ouverture des interfaces des plugins) arrive dans la prochaine version.").size(11.5).color(TEXT_DIM));
                });
            });
        });
    }

    fn help_tab(&mut self, ui: &mut egui::Ui) {
        theme::card(ui, 0.3, |ui| {
            theme::section_title(ui, "✨", "Mise en route", "3 minutes, une seule fois");
            let steps = [
                ("1", "Installe VB-Cable (gratuit). Il crée « CABLE Input » (sortie) et « CABLE Output » (entrée)."),
                ("2", "Optionnel : dans Panneau de configuration › Son › Enregistrement, renomme « CABLE Output » en « Guitar To Discord »."),
                ("3", "Ici : choisis ta carte son (guitare), ton micro, et « CABLE Input » en sortie. Clique sur Démarrer."),
                ("4", "Dans Discord : Paramètres › Voix et vidéo › Périphérique d'entrée › « CABLE Output » (ou « Guitar To Discord »)."),
                ("5", "Dans Discord, désactive Suppression du bruit / Krisp et l'atténuation d'écho : sinon ta guitare sera coupée."),
            ];
            for (n, s) in steps {
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(n).font(theme::bold(16.0)).color(theme::RED_HOT));
                    ui.label(RichText::new(s).color(TEXT));
                });
            }
        });
        ui.add_space(14.0);
        theme::card(ui, 0.15, |ui| {
            theme::section_title(ui, "💡", "Astuces", "Pour un son propre");
            for s in [
                "ASIO (Focusrite) : la build par défaut utilise WASAPI, qui marche avec toutes les Focusrite. Pour ASIO, compile avec --features asio.",
                "Latence : règle ta Focusrite et le câble sur la même fréquence (48 kHz) pour éviter tout rééchantillonnage.",
                "Larsen ou écho : n'active pas le Retour casque si Windows « écoute » déjà ta carte son.",
                "Mode Discord : passe en « Activation vocale » avec un seuil bas, ou en Push-to-talk pendant que tu joues.",
            ] {
                ui.label(RichText::new(format!("•  {s}")).color(TEXT_DIM));
            }
            ui.horizontal(|ui| {
                if theme::chip(ui, "GitHub").clicked() {
                    open_url("https://github.com/VeqtaDev/GuitarToDiscord");
                }
                if theme::chip(ui, "VB-Cable").clicked() {
                    open_url(VB_CABLE_URL);
                }
            });
        });
    }
}
