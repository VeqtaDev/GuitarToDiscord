//! Black liquid-glass theme for egui: dark glass cards, glass nav pill,
//! heavy italic headings, red gradient buttons with a white "key".

use eframe::egui::{
    self, epaint::Mesh, Align2, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId,
    Pos2, Rect, Response, Sense, Shape, Stroke, StrokeKind, Ui, Vec2,
};
use eframe::epaint::Shadow;
use std::f32::consts::TAU;

// ---------------------------------------------------------------------------
// Palette (black + red)
// ---------------------------------------------------------------------------

pub const BG: Color32 = Color32::from_rgb(8, 4, 6);
pub const INK: Color32 = Color32::from_rgb(255, 240, 242);
#[allow(dead_code)]
pub const INK2: Color32 = Color32::from_rgb(230, 190, 197);
pub const MUTE: Color32 = Color32::from_rgb(178, 138, 146);
#[allow(dead_code)]
pub const MUTE2: Color32 = Color32::from_rgb(130, 96, 104);
pub const LINE: Color32 = Color32::from_rgb(52, 26, 32);
pub const TINT: Color32 = Color32::from_rgb(40, 14, 20);
pub const R1: Color32 = Color32::from_rgb(255, 77, 94);
pub const R2: Color32 = Color32::from_rgb(200, 16, 46);
pub const R3: Color32 = Color32::from_rgb(255, 154, 164);
pub const OK: Color32 = Color32::from_rgb(31, 184, 114);

// Names used by app.rs
pub const TEXT: Color32 = INK;
pub const TEXT_DIM: Color32 = MUTE;
pub const RED_HOT: Color32 = R1;
#[allow(dead_code)]
pub const RED_DEEP: Color32 = R2;

pub fn white(a: f32) -> Color32 {
    Color32::from_white_alpha((a.clamp(0.0, 1.0) * 255.0) as u8)
}

pub fn rgba(c: Color32, a: f32) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), (a.clamp(0.0, 1.0) * 255.0) as u8)
}

fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgba_premultiplied(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()), l(a.a(), b.a()))
}

fn stops(st: &[(f32, Color32)], t: f32) -> Color32 {
    if t <= st[0].0 {
        return st[0].1;
    }
    for w in st.windows(2) {
        let (p0, c0) = w[0];
        let (p1, c1) = w[1];
        if t <= p1 {
            return lerp_color(c0, c1, (t - p0) / (p1 - p0).max(1e-6));
        }
    }
    st[st.len() - 1].1
}

// ---------------------------------------------------------------------------
// Setup
// ---------------------------------------------------------------------------

pub fn install(ctx: &egui::Context) {
    let windir = std::env::var("WINDIR").unwrap_or_else(|_| r"C:\Windows".into());
    let load = |files: &[&str]| -> Option<Vec<u8>> {
        files.iter().find_map(|f| std::fs::read(format!(r"{windir}\Fonts\{f}")).ok())
    };
    let mut fonts = FontDefinitions::default();
    let add_family = |fonts: &mut FontDefinitions, key: &str, data: Option<Vec<u8>>| {
        let mut fam = Vec::new();
        if let Some(d) = data {
            fonts.font_data.insert(key.into(), FontData::from_owned(d).into());
            fam.push(key.to_owned());
        }
        fam.extend(fonts.families[&FontFamily::Proportional].iter().cloned());
        fonts.families.insert(FontFamily::Name(key.into()), fam);
    };
    // Body: Segoe UI Semibold, close to Nunito 700/800.
    if let Some(d) = load(&["seguisb.ttf", "segoeui.ttf"]) {
        fonts.font_data.insert("body".into(), FontData::from_owned(d).into());
        fonts.families.get_mut(&FontFamily::Proportional).unwrap().insert(0, "body".into());
    }
    add_family(&mut fonts, "bold", load(&["segoeuib.ttf", "seguisb.ttf"]));
    // Headings: Segoe UI Black Italic, stands in for Nunito 900 italic.
    add_family(&mut fonts, "black", load(&["seguibli.ttf", "seguibl.ttf", "segoeuib.ttf"]));
    ctx.set_fonts(fonts);

    let mut style = (*ctx.global_style()).clone();
    style.spacing.item_spacing = Vec2::new(10.0, 10.0);
    style.spacing.button_padding = Vec2::new(14.0, 8.0);
    style.spacing.interact_size.y = 34.0;
    style.spacing.combo_width = 260.0;

    let v = &mut style.visuals;
    *v = egui::Visuals::dark();
    v.override_text_color = Some(INK);
    v.panel_fill = BG;
    v.window_fill = Color32::from_rgb(22, 10, 13);
    v.window_stroke = Stroke::new(1.0, LINE);
    v.window_corner_radius = CornerRadius::same(18);
    v.menu_corner_radius = CornerRadius::same(16);
    v.popup_shadow = Shadow { offset: [0, 14], blur: 32, spread: 0, color: Color32::from_black_alpha(160) };
    v.extreme_bg_color = Color32::from_rgb(18, 9, 12);
    v.selection.bg_fill = rgba(R2, 0.45);
    v.selection.stroke = Stroke::new(1.0, R1);
    v.hyperlink_color = R1;
    let r = CornerRadius::same(14);
    for w in [&mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
        w.corner_radius = r;
        w.fg_stroke = Stroke::new(1.5, INK);
        w.expansion = 0.0;
    }
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, LINE);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, MUTE);
    v.widgets.inactive.weak_bg_fill = Color32::from_rgb(24, 12, 15);
    v.widgets.inactive.bg_fill = Color32::from_rgb(24, 12, 15);
    v.widgets.inactive.bg_stroke = Stroke::new(1.5, LINE);
    v.widgets.hovered.weak_bg_fill = Color32::from_rgb(24, 12, 15);
    v.widgets.hovered.bg_fill = Color32::from_rgb(24, 12, 15);
    v.widgets.hovered.bg_stroke = Stroke::new(1.5, R3);
    v.widgets.active.weak_bg_fill = TINT;
    v.widgets.active.bg_fill = TINT;
    v.widgets.active.bg_stroke = Stroke::new(1.5, R1);
    v.widgets.open.weak_bg_fill = Color32::from_rgb(24, 12, 15);
    v.widgets.open.bg_stroke = Stroke::new(1.5, R1);
    ctx.set_global_style(style);
}

pub fn bold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("bold".into()))
}

pub fn black(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("black".into()))
}

/// Small heavy uppercase label.
#[allow(dead_code)]
pub fn lbl(ui: &mut Ui, text: &str) {
    ui.label(egui::RichText::new(text.to_uppercase()).font(bold(11.5)).color(MUTE).extra_letter_spacing(0.9));
}

// ---------------------------------------------------------------------------
// Geometry helpers
// ---------------------------------------------------------------------------

fn rrect_points(rect: Rect, r: f32) -> Vec<Pos2> {
    let r = r.min(rect.width() / 2.0).min(rect.height() / 2.0).max(0.0);
    let seg = 8;
    let mut pts = Vec::with_capacity(4 * (seg + 1));
    let corners = [
        (Pos2::new(rect.right() - r, rect.top() + r), -0.25),
        (Pos2::new(rect.right() - r, rect.bottom() - r), 0.0),
        (Pos2::new(rect.left() + r, rect.bottom() - r), 0.25),
        (Pos2::new(rect.left() + r, rect.top() + r), 0.5),
    ];
    for (c, start) in corners {
        for i in 0..=seg {
            let a = (start + 0.25 * i as f32 / seg as f32) * TAU;
            pts.push(c + Vec2::new(a.cos(), a.sin()) * r);
        }
    }
    pts
}

/// Rounded rect filled with a per-point colour function.
pub fn gradient_rrect(rect: Rect, r: f32, color: impl Fn(Pos2) -> Color32) -> Shape {
    let pts = rrect_points(rect, r);
    let mut mesh = Mesh::default();
    let c = rect.center();
    mesh.colored_vertex(c, color(c));
    for p in &pts {
        mesh.colored_vertex(*p, color(*p));
    }
    let n = pts.len() as u32;
    for i in 0..n {
        mesh.add_triangle(0, 1 + i, 1 + (i + 1) % n);
    }
    Shape::mesh(mesh)
}

pub fn radial_blob(center: Pos2, radius: f32, color: Color32) -> Shape {
    let seg = 48;
    let mut mesh = Mesh::default();
    mesh.colored_vertex(center, color);
    let mid = lerp_color(Color32::TRANSPARENT, color, 0.4);
    for i in 0..seg {
        mesh.colored_vertex(center + Vec2::angled(i as f32 / seg as f32 * TAU) * radius * 0.55, mid);
    }
    for i in 0..seg {
        mesh.colored_vertex(center + Vec2::angled(i as f32 / seg as f32 * TAU) * radius, Color32::TRANSPARENT);
    }
    for i in 0..seg {
        let j = (i + 1) % seg;
        mesh.add_triangle(0, 1 + i, 1 + j);
        let (a, b, c, d) = (1 + i, 1 + j, 1 + seg + i, 1 + seg + j);
        mesh.add_triangle(a, b, c);
        mesh.add_triangle(b, d, c);
    }
    Shape::mesh(mesh)
}

// ---------------------------------------------------------------------------
// Backdrop: red "stage" band on top fading into the light page (.sky)
// ---------------------------------------------------------------------------

pub fn backdrop(ui: &Ui, rect: Rect, t: f32, live: f32) {
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::ZERO, BG);
    let band = Rect::from_min_size(rect.min, Vec2::new(rect.width(), 260.0));
    p.add(gradient_rrect(band, 0.0, |pt| {
        let y = (pt.y - band.top()) / band.height();
        stops(&[(0.0, Color32::from_rgb(70, 4, 16)), (0.5, Color32::from_rgb(34, 4, 10)), (1.0, BG)], y)
    }));
    let w = rect.width();
    for (i, (x, sp, a)) in [(0.12, 0.31, 0.55), (0.5, 0.23, 0.35), (0.88, 0.27, 0.55)].iter().enumerate() {
        let cx = rect.left() + w * (x + 0.06 * (t * sp + i as f32).sin());
        p.add(radial_blob(Pos2::new(cx, rect.top() + 20.0), 260.0, rgba(R2, a * (0.6 + 0.4 * live))));
    }
    let cx = rect.left() + w * (0.5 + 0.2 * (t * 0.11).sin());
    p.add(radial_blob(Pos2::new(cx, rect.bottom() + 60.0), 420.0, rgba(Color32::from_rgb(120, 6, 28), 0.35)));
}

// ---------------------------------------------------------------------------
// Glass surfaces
// ---------------------------------------------------------------------------

/// Dark glass card.
pub fn gw_shapes(rect: Rect, radius: f32) -> Vec<Shape> {
    let cr = CornerRadius::same(radius as u8);
    let mut v = Vec::new();
    let s1 = Shadow { offset: [0, 20], blur: 44, spread: 0, color: Color32::from_black_alpha(150) };
    v.push(Shape::Rect(s1.as_shape(rect, cr)));
    v.push(gradient_rrect(rect, radius, |p| {
        let dx = (p.x - (rect.left() + rect.width() * 0.2)) / (rect.width() * 0.7);
        let dy = (p.y - rect.top()) / (rect.height() * 0.6);
        let hl = (1.0 - (dx * dx + dy * dy).sqrt()).clamp(0.0, 1.0);
        let by = ((p.y - rect.top()) / rect.height()).clamp(0.0, 1.0);
        let base = lerp_color(Color32::from_rgba_premultiplied(26, 12, 15, 236), Color32::from_rgba_premultiplied(18, 7, 10, 236), by);
        lerp_color(base, Color32::from_rgba_premultiplied(58, 24, 30, 240), hl * 0.6)
    }));
    v.push(Shape::Rect(egui::epaint::RectShape::stroke(rect, cr, Stroke::new(1.0, white(0.07)), StrokeKind::Inside)));
    v.push(top_rim(rect, radius, 0.35));
    v
}

/// Dark wine glass (`.gl`), used for the nav pill.
pub fn gl_shapes(rect: Rect, radius: f32) -> Vec<Shape> {
    let cr = CornerRadius::same(radius as u8);
    let mut v = Vec::new();
    let s = Shadow { offset: [0, 18], blur: 40, spread: 0, color: rgba(Color32::from_rgb(80, 8, 20), 0.28) };
    v.push(Shape::Rect(s.as_shape(rect, cr)));
    v.push(gradient_rrect(rect, radius, |p| {
        let y = (p.y - rect.top()) / rect.height();
        let x = (p.x - rect.left()) / rect.width();
        let hl = (1.0 - ((x - 0.25).powi(2) * 4.0 + (y - 0.3).powi(2) * 4.0).sqrt()).clamp(0.0, 1.0) * 0.12;
        let base = stops(&[(0.0, Color32::from_rgb(70, 8, 18)), (1.0, Color32::from_rgb(40, 4, 10))], y);
        lerp_color(base, Color32::from_rgb(255, 160, 170), hl + (y - 0.7).max(0.0) * 0.25)
    }));
    v.push(Shape::Rect(egui::epaint::RectShape::stroke(rect, cr, Stroke::new(1.0, white(0.14)), StrokeKind::Inside)));
    v.push(top_rim(rect, radius, 0.55));
    v
}

fn top_rim(rect: Rect, radius: f32, a: f32) -> Shape {
    let top = Rect::from_min_max(
        Pos2::new(rect.left() + rect.width() * 0.1, rect.top() + 0.5),
        Pos2::new(rect.right() - rect.width() * 0.1, rect.top() + 1.5),
    );
    let _ = radius;
    gradient_rrect(top, 0.5, |p| {
        let x = (p.x - top.left()) / top.width();
        stops(&[(0.0, white(0.0)), (0.5, white(a)), (1.0, white(0.0))], x)
    })
}

/// Back-compat for the header (dark glass).
#[allow(dead_code)]
pub fn glass_shapes(rect: Rect, radius: f32, _tint: f32) -> Vec<Shape> {
    gl_shapes(rect, radius)
}

/// Run `add` inside a white glass card.
pub fn card<R>(ui: &mut Ui, _tint: f32, add: impl FnOnce(&mut Ui) -> R) -> R {
    let bg = ui.painter().add(Shape::Noop);
    let pad = 22.0;
    let avail = ui.available_width();
    let outer = ui.available_rect_before_wrap();
    let inner_min = outer.min + Vec2::splat(pad);
    let inner_w = avail - 2.0 * pad;
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(Rect::from_min_size(inner_min, Vec2::new(inner_w, f32::INFINITY))));
    child.set_width(inner_w);
    let r = add(&mut child);
    let used = child.min_rect();
    let card_rect = Rect::from_min_max(outer.min, Pos2::new(outer.min.x + avail, used.bottom() + pad));
    ui.painter().set(bg, Shape::Vec(gw_shapes(card_rect, 28.0)));
    ui.allocate_rect(card_rect, Sense::hover());
    r
}

/// Heading with a red gradient icon tile.
pub fn section_title(ui: &mut Ui, icon: &str, title: &str, subtitle: &str) {
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(Vec2::splat(40.0), Sense::hover());
        let sh = Shadow { offset: [0, 6], blur: 14, spread: 0, color: rgba(R2, 0.3) };
        ui.painter().add(Shape::Rect(sh.as_shape(r, CornerRadius::same(13))));
        ui.painter().add(gradient_rrect(r, 13.0, |p| lerp_color(R1, R2, (p.y - r.top()) / r.height())));
        ui.painter().text(r.center(), Align2::CENTER_CENTER, icon, FontId::proportional(18.0), Color32::WHITE);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.label(egui::RichText::new(title).font(black(20.0)).color(INK));
            ui.label(egui::RichText::new(subtitle).font(bold(13.0)).color(MUTE));
        });
    });
    ui.add_space(6.0);
}

// ---------------------------------------------------------------------------
// Controls
// ---------------------------------------------------------------------------

/// Red gradient button with a white key that expands on hover.
pub fn pill_button(ui: &mut Ui, text: &str, primary: bool, width: f32) -> Response {
    let h = 50.0;
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, h), Sense::click());
    let hover = ui.ctx().animate_bool_with_time(resp.id, resp.hovered(), 0.35);
    let press = if resp.is_pointer_button_down_on() { 0.95 } else { 1.0 };
    let rect = Rect::from_center_size(rect.center(), rect.size() * press);
    let p = ui.painter();
    let (c1, c2, sh) = if primary { (R1, R2, rgba(R2, 0.5)) } else { (Color32::from_rgb(70, 20, 30), Color32::from_rgb(40, 10, 16), Color32::from_black_alpha(120)) };
    let shadow = Shadow { offset: [0, 8], blur: 20, spread: 0, color: sh };
    p.add(Shape::Rect(shadow.as_shape(rect, CornerRadius::same(17))));
    p.add(gradient_rrect(rect, 17.0, |pt| lerp_color(c1, c2, (pt.y - rect.top()) / rect.height())));
    p.add(top_rim(rect, 17.0, 0.6));
    // key
    let kw = 44.0 + (rect.width() - 8.0 - 44.0) * ease_back(hover);
    let key = Rect::from_min_size(rect.min + Vec2::new(4.0, 4.0), Vec2::new(kw, h - 8.0));
    let ksh = Shadow { offset: [0, 2], blur: 6, spread: 0, color: Color32::from_black_alpha(70) };
    p.add(Shape::Rect(ksh.as_shape(key, CornerRadius::same(13))));
    p.add(gradient_rrect(key, 13.0, |pt| lerp_color(Color32::WHITE, Color32::from_rgb(252, 228, 231), (pt.y - key.top()) / key.height())));
    let ic = Pos2::new(key.right() - 22.0, key.center().y);
    if primary {
        p.add(Shape::convex_polygon(
            vec![ic + Vec2::new(-5.0, -7.0), ic + Vec2::new(7.0, 0.0), ic + Vec2::new(-5.0, 7.0)],
            R2,
            Stroke::new(2.0, R2),
        ));
    } else {
        p.rect_filled(Rect::from_center_size(ic, Vec2::splat(12.0)), CornerRadius::same(3), R2);
    }
    let tx = rect.center().x + 20.0 * (1.0 - hover) - 20.0 * hover;
    let tc = lerp_color(Color32::WHITE, R2, hover);
    p.text(Pos2::new(tx, rect.center().y), Align2::CENTER_CENTER, text, bold(16.0), tc);
    resp
}

fn ease_back(t: f32) -> f32 {
    // slight overshoot for a springy feel
    let c1 = 1.4;
    let c3 = c1 + 1.0;
    let x = t - 1.0;
    1.0 + c3 * x * x * x + c1 * x * x
}

/// White glass chip button (`.gw` small).
pub fn chip(ui: &mut Ui, text: &str) -> Response {
    let galley = ui.painter().layout_no_wrap(text.to_owned(), bold(13.5), INK);
    let size = Vec2::new(galley.size().x + 26.0, 34.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let hover = ui.ctx().animate_bool_with_time(resp.id, resp.hovered(), 0.15);
    let p = ui.painter();
    let sh = Shadow { offset: [0, 4], blur: 12, spread: 0, color: Color32::from_black_alpha(90) };
    p.add(Shape::Rect(sh.as_shape(rect, CornerRadius::same(12))));
    p.rect_filled(rect, CornerRadius::same(12), lerp_color(Color32::from_rgb(30, 14, 18), Color32::from_rgb(48, 18, 25), hover));
    p.rect_stroke(rect, CornerRadius::same(12), Stroke::new(1.0, lerp_color(white(0.10), rgba(R1, 0.8), hover)), StrokeKind::Inside);
    p.galley(rect.center() - galley.size() / 2.0, galley, INK);
    resp
}

/// iOS-style switch.
pub fn toggle(ui: &mut Ui, on: &mut bool) -> Response {
    let size = Vec2::new(52.0, 31.0);
    let (rect, mut resp) = ui.allocate_exact_size(size, Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    let t = ui.ctx().animate_bool_with_time(resp.id, *on, 0.25);
    let p = ui.painter();
    p.add(gradient_rrect(rect, 15.5, |pt| {
        let on_c = lerp_color(R1, R2, (pt.y - rect.top()) / rect.height());
        lerp_color(LINE, on_c, t)
    }));
    let stretch = 1.0 + 0.25 * (1.0 - (2.0 * t - 1.0).abs());
    let kh = 27.0;
    let kw = kh * stretch;
    let x = rect.left() + 2.0 + t * (rect.width() - 4.0 - kw);
    let knob = Rect::from_min_size(Pos2::new(x, rect.top() + 2.0), Vec2::new(kw, kh));
    let sh = Shadow { offset: [0, 3], blur: 8, spread: 0, color: Color32::from_black_alpha(110) };
    p.add(Shape::Rect(sh.as_shape(knob, CornerRadius::same(13))));
    p.rect_filled(knob, CornerRadius::same(13), Color32::WHITE);
    resp
}

/// Range: thin track, red fill, white thumb with a red ring.
pub fn slider(ui: &mut Ui, value: &mut f32, min: f32, max: f32, label: &str, unit: &str) -> Response {
    let width = ui.available_width();
    let h = 50.0;
    let (rect, mut resp) = ui.allocate_exact_size(Vec2::new(width, h), Sense::click_and_drag());
    let track = Rect::from_min_max(Pos2::new(rect.left() + 12.0, rect.bottom() - 18.0), Pos2::new(rect.right() - 12.0, rect.bottom() - 8.0));
    if let Some(pos) = resp.interact_pointer_pos() {
        if resp.dragged() || resp.clicked() {
            let t = ((pos.x - track.left()) / track.width()).clamp(0.0, 1.0);
            let nv = ((min + t * (max - min)) * 2.0).round() / 2.0;
            if (nv - *value).abs() > f32::EPSILON {
                *value = nv;
                resp.mark_changed();
            }
        }
    }
    if resp.double_clicked() {
        *value = 0.0f32.clamp(min, max);
        resp.mark_changed();
    }
    let t = ((*value - min) / (max - min)).clamp(0.0, 1.0);
    let hover = ui.ctx().animate_bool_with_time(resp.id, resp.hovered() || resp.dragged(), 0.15);
    let p = ui.painter();
    p.text(rect.left_top(), Align2::LEFT_TOP, label.to_uppercase(), bold(11.5), MUTE);
    let val = if *value <= min + 0.01 && unit == "dB" && min <= -90.0 { "Off".to_owned() } else { format!("{:+.1} {unit}", value) };
    p.text(rect.right_top(), Align2::RIGHT_TOP, val, black(15.0), R1);
    p.rect_filled(track, CornerRadius::same(5), TINT);
    let fill = Rect::from_min_max(track.min, Pos2::new(track.left() + t * track.width(), track.bottom()));
    if fill.width() > 1.0 {
        p.add(gradient_rrect(fill, 5.0, |pt| lerp_color(R2, R1, (pt.x - track.left()) / track.width())));
    }
    let kc = Pos2::new(track.left() + t * track.width(), track.center().y);
    let kr = 13.0 + 1.5 * hover;
    let sh = Shadow { offset: [0, 4], blur: 12, spread: 0, color: rgba(R2, 0.45) };
    p.add(Shape::Rect(sh.as_shape(Rect::from_center_size(kc, Vec2::splat(kr * 2.0)), CornerRadius::same(kr as u8))));
    p.circle_filled(kc, kr, Color32::WHITE);
    p.circle_stroke(kc, kr - 1.5, Stroke::new(3.0, R1));
    resp
}

/// Segmented level meter.
pub fn meter(ui: &mut Ui, level: f32) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 10.0), Sense::hover());
    let db = crate::dsp::lin_to_db(level).clamp(-60.0, 0.0);
    let t = (db + 60.0) / 60.0;
    let p = ui.painter();
    let segs = 40;
    let gap = 3.0;
    let sw = (rect.width() - gap * (segs as f32 - 1.0)) / segs as f32;
    for i in 0..segs {
        let x0 = rect.left() + i as f32 * (sw + gap);
        let st = i as f32 / segs as f32;
        let col = if st < t { lerp_color(R3, R2, st) } else { TINT };
        p.rect_filled(Rect::from_min_size(Pos2::new(x0, rect.top()), Vec2::new(sw, rect.height())), CornerRadius::same(3), col);
    }
}

/// Pulsing status dot.
pub fn status_dot(ui: &mut Ui, on: bool, t: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(14.0), Sense::hover());
    let c = rect.center();
    if on {
        let ph = (t * 1.6).fract();
        ui.painter().circle_filled(c, 4.0 + 7.0 * ph, rgba(OK, 0.45 * (1.0 - ph)));
    }
    ui.painter().circle_filled(c, 4.5, if on { OK } else { white(0.45) });
}

/// Dark glass nav pill with a sliding translucent pill.
pub fn segmented(ui: &mut Ui, id: &str, selected: &mut usize, items: &[&str]) {
    let font = bold(14.0);
    let widths: Vec<f32> = items
        .iter()
        .map(|s| ui.painter().layout_no_wrap((*s).to_owned(), font.clone(), Color32::WHITE).size().x + 34.0)
        .collect();
    let h = 52.0;
    let total: f32 = widths.iter().sum::<f32>() + 12.0;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(total, h), Sense::hover());
    for s in gl_shapes(rect, h / 2.0) {
        ui.painter().add(s);
    }
    let mut xs = Vec::with_capacity(items.len());
    let mut x = rect.left() + 6.0;
    for w in &widths {
        xs.push(x);
        x += w;
    }
    let sid = ui.scope_id().with(id);
    let ax = ui.ctx().animate_value_with_time(sid.with("x"), xs[*selected], 0.35);
    let aw = ui.ctx().animate_value_with_time(sid.with("w"), widths[*selected], 0.35);
    let pill = Rect::from_min_size(Pos2::new(ax, rect.top() + 6.0), Vec2::new(aw, 40.0));
    ui.painter().rect_filled(pill, CornerRadius::same(20), white(0.16));
    ui.painter().add(top_rim(pill, 20.0, 0.35));
    for (i, label) in items.iter().enumerate() {
        let r = Rect::from_min_size(Pos2::new(xs[i], rect.top() + 6.0), Vec2::new(widths[i], 40.0));
        let resp = ui.interact(r, sid.with(i), Sense::click());
        if resp.clicked() {
            *selected = i;
        }
        let a = if i == *selected { 1.0 } else if resp.hovered() { 0.85 } else { 0.62 };
        ui.painter().text(r.center(), Align2::CENTER_CENTER, *label, font.clone(), white(a));
    }
}

/// Small segmented choice for in-card options (dark glass, red pill).
pub fn choice(ui: &mut Ui, id: &str, selected: &mut usize, items: &[&str]) -> bool {
    let font = bold(13.5);
    let w = ui.available_width();
    let h = 40.0;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(w, h), Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::same(14), Color32::from_rgb(16, 7, 10));
    p.rect_stroke(rect, CornerRadius::same(14), Stroke::new(1.0, white(0.07)), StrokeKind::Inside);
    let n = items.len() as f32;
    let iw = (w - 8.0) / n;
    let sid = ui.scope_id().with(id);
    let ax = ui.ctx().animate_value_with_time(sid.with("x"), *selected as f32, 0.3);
    let pill = Rect::from_min_size(rect.min + Vec2::new(4.0 + ax * iw, 4.0), Vec2::new(iw, h - 8.0));
    let sh = Shadow { offset: [0, 4], blur: 12, spread: 0, color: rgba(R2, 0.5) };
    p.add(Shape::Rect(sh.as_shape(pill, CornerRadius::same(11))));
    p.add(gradient_rrect(pill, 11.0, |pt| lerp_color(R1, R2, (pt.y - pill.top()) / pill.height())));
    let mut changed = false;
    for (i, label) in items.iter().enumerate() {
        let r = Rect::from_min_size(rect.min + Vec2::new(4.0 + i as f32 * iw, 4.0), Vec2::new(iw, h - 8.0));
        let resp = ui.interact(r, sid.with(i), Sense::click());
        if resp.clicked() && *selected != i {
            *selected = i;
            changed = true;
        }
        let a = if i == *selected { 1.0 } else if resp.hovered() { 0.85 } else { 0.55 };
        ui.painter().text(r.center(), Align2::CENTER_CENTER, *label, font.clone(), white(a));
    }
    changed
}
