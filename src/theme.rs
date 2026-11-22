//! "Liquid glass" look for egui: animated red gradient backdrop,
//! translucent glass cards with specular rims, iOS-style controls.

use eframe::egui::{
    self, epaint::Mesh, Align2, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId,
    Pos2, Rect, Response, Sense, Shape, Stroke, StrokeKind, Ui, Vec2,
};
use eframe::epaint::Shadow;
use std::f32::consts::TAU;

// ---------------------------------------------------------------------------
// Palette
// ---------------------------------------------------------------------------

pub const RED: Color32 = Color32::from_rgb(255, 36, 66);
pub const RED_HOT: Color32 = Color32::from_rgb(255, 77, 77);
pub const RED_DEEP: Color32 = Color32::from_rgb(196, 0, 42);
pub const CRIMSON: Color32 = Color32::from_rgb(120, 0, 24);
pub const INK: Color32 = Color32::from_rgb(14, 2, 6);
pub const TEXT: Color32 = Color32::from_rgb(255, 244, 246);
pub const TEXT_DIM: Color32 = Color32::from_rgb(255, 196, 204);
pub const OK: Color32 = Color32::from_rgb(64, 230, 140);

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

/// Multi-stop gradient lookup (stops sorted by position).
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
    // Segoe UI Variable / Segoe UI ship with Windows and read close to SF Pro.
    let mut fonts = FontDefinitions::default();
    let try_load = |files: &[&str]| -> Option<Vec<u8>> {
        let windir = std::env::var("WINDIR").unwrap_or_else(|_| r"C:\Windows".into());
        files
            .iter()
            .find_map(|f| std::fs::read(format!(r"{windir}\Fonts\{f}")).ok())
    };
    if let Some(reg) = try_load(&["SegUIVar.ttf", "segoeui.ttf"]) {
        fonts.font_data.insert("ui".into(), FontData::from_owned(reg).into());
        fonts.families.get_mut(&FontFamily::Proportional).unwrap().insert(0, "ui".into());
    }
    if let Some(bold) = try_load(&["seguisb.ttf", "segoeuib.ttf"]) {
        fonts.font_data.insert("bold".into(), FontData::from_owned(bold).into());
        let mut fam = vec!["bold".to_owned()];
        fam.extend(fonts.families[&FontFamily::Proportional].iter().cloned());
        fonts.families.insert(FontFamily::Name("bold".into()), fam);
    } else {
        let fam = fonts.families[&FontFamily::Proportional].clone();
        fonts.families.insert(FontFamily::Name("bold".into()), fam);
    }
    ctx.set_fonts(fonts);

    let mut style = (*ctx.global_style()).clone();
    style.spacing.item_spacing = Vec2::new(10.0, 10.0);
    style.spacing.button_padding = Vec2::new(14.0, 8.0);
    style.spacing.interact_size.y = 32.0;
    style.spacing.combo_width = 260.0;

    let v = &mut style.visuals;
    *v = egui::Visuals::dark();
    v.override_text_color = Some(TEXT);
    v.panel_fill = Color32::TRANSPARENT;
    v.window_fill = Color32::from_rgba_unmultiplied(40, 4, 12, 245);
    v.window_stroke = Stroke::new(1.0, white(0.18));
    v.window_corner_radius = CornerRadius::same(18);
    v.menu_corner_radius = CornerRadius::same(16);
    v.popup_shadow = Shadow { offset: [0, 12], blur: 30, spread: 0, color: Color32::from_black_alpha(140) };
    v.extreme_bg_color = Color32::from_rgba_unmultiplied(0, 0, 0, 70);
    v.selection.bg_fill = rgba(RED, 0.55);
    v.selection.stroke = Stroke::new(1.0, TEXT);
    v.hyperlink_color = RED_HOT;

    let r = CornerRadius::same(12);
    for w in [&mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
        w.corner_radius = r;
        w.fg_stroke = Stroke::new(1.0, TEXT);
    }
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, white(0.10));
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, TEXT_DIM);
    v.widgets.inactive.weak_bg_fill = white(0.07);
    v.widgets.inactive.bg_fill = white(0.07);
    v.widgets.inactive.bg_stroke = Stroke::new(1.0, white(0.16));
    v.widgets.hovered.weak_bg_fill = white(0.13);
    v.widgets.hovered.bg_fill = white(0.13);
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, white(0.32));
    v.widgets.active.weak_bg_fill = rgba(RED, 0.35);
    v.widgets.active.bg_fill = rgba(RED, 0.35);
    v.widgets.active.bg_stroke = Stroke::new(1.0, white(0.5));
    v.widgets.open.weak_bg_fill = white(0.16);
    v.widgets.open.bg_stroke = Stroke::new(1.0, white(0.35));
    ctx.set_global_style(style);
}

pub fn bold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("bold".into()))
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

/// Rounded rect filled with an arbitrary per-point colour function
/// (exact for linear gradients thanks to the fan triangulation).
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

/// Soft radial glow (for the animated backdrop blobs).
pub fn radial_blob(center: Pos2, radius: f32, color: Color32) -> Shape {
    let seg = 48;
    let mut mesh = Mesh::default();
    mesh.colored_vertex(center, color);
    let mid = lerp_color(Color32::TRANSPARENT, color, 0.35);
    for i in 0..seg {
        let a = i as f32 / seg as f32 * TAU;
        mesh.colored_vertex(center + Vec2::angled(a) * radius * 0.55, mid);
    }
    for i in 0..seg {
        let a = i as f32 / seg as f32 * TAU;
        mesh.colored_vertex(center + Vec2::angled(a) * radius, Color32::TRANSPARENT);
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
// Backdrop
// ---------------------------------------------------------------------------

pub fn backdrop(ui: &Ui, rect: Rect, t: f32, live: f32) {
    let p = ui.painter();
    p.add(gradient_rrect(rect, 0.0, |pt| {
        let y = (pt.y - rect.top()) / rect.height();
        stops(&[(0.0, Color32::from_rgb(38, 2, 10)), (0.55, Color32::from_rgb(20, 1, 6)), (1.0, INK)], y)
    }));
    let w = rect.width();
    let h = rect.height();
    let blob = |cx: f32, cy: f32, r: f32, c: Color32, a: f32| {
        p.add(radial_blob(rect.min + Vec2::new(cx * w, cy * h), r * w.max(h), rgba(c, a)));
    };
    let s = 0.18 + 0.12 * live;
    blob(0.18 + 0.06 * (t * 0.21).sin(), 0.10 + 0.05 * (t * 0.17).cos(), 0.55, RED, 0.55 * s / 0.3);
    blob(0.86 + 0.05 * (t * 0.13).cos(), 0.30 + 0.08 * (t * 0.19).sin(), 0.45, RED_DEEP, 0.60 * s / 0.3);
    blob(0.50 + 0.10 * (t * 0.11).sin(), 1.02 + 0.04 * (t * 0.23).cos(), 0.50, RED_HOT, 0.40 * s / 0.3);
    blob(0.05 + 0.04 * (t * 0.27).cos(), 0.85, 0.35, CRIMSON, 0.70);
}

// ---------------------------------------------------------------------------
// Glass surfaces
// ---------------------------------------------------------------------------

/// Paint a liquid-glass panel into `rect`. `tint` 0..1 adds red body colour.
pub fn glass_shapes(rect: Rect, radius: f32, tint: f32) -> Vec<Shape> {
    let mut v = Vec::new();
    let shadow = Shadow { offset: [0, 14], blur: 40, spread: 0, color: Color32::from_black_alpha(110) };
    v.push(Shape::Rect(shadow.as_shape(rect, CornerRadius::same(radius as u8))));
    // Body: 135deg sheen like the Roamly navbar (.lq), on a red-tinted base.
    let base = lerp_color(white(0.05), rgba(RED_DEEP, 0.28), tint);
    let diag = rect.width() + rect.height();
    v.push(gradient_rrect(rect, radius, |p| {
        let t = ((p.x - rect.left()) + (p.y - rect.top())) / diag;
        let sheen = stops(
            &[(0.0, white(0.20)), (0.38, white(0.06)), (0.62, white(0.025)), (1.0, white(0.11))],
            t,
        );
        let a = sheen;
        Color32::from_rgba_premultiplied(
            base.r().saturating_add(a.r()),
            base.g().saturating_add(a.g()),
            base.b().saturating_add(a.b()),
            base.a().saturating_add(a.a()),
        )
    }));
    // Specular top-left rim + faint bottom-right rim.
    v.push(Shape::Rect(egui::epaint::RectShape::stroke(
        rect.shrink(0.5),
        CornerRadius::same(radius as u8),
        Stroke::new(1.0, white(0.16)),
        StrokeKind::Inside,
    )));
    let top = Rect::from_min_max(rect.min + Vec2::new(radius * 0.6, 0.5), Pos2::new(rect.right() - radius * 0.6, rect.top() + 1.6));
    v.push(gradient_rrect(top, 1.0, |p| {
        let x = (p.x - top.left()) / top.width();
        stops(&[(0.0, white(0.0)), (0.25, white(0.65)), (0.7, white(0.35)), (1.0, white(0.0))], x)
    }));
    v
}

/// Run `add` inside a glass card that stretches to `width`.
pub fn card<R>(ui: &mut Ui, tint: f32, add: impl FnOnce(&mut Ui) -> R) -> R {
    let bg = ui.painter().add(Shape::Noop);
    let pad = 18.0;
    let avail = ui.available_width();
    let outer = ui.available_rect_before_wrap();
    let inner_rect = Rect::from_min_size(outer.min + Vec2::splat(pad), Vec2::new(avail - 2.0 * pad, 0.0));
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(Rect::from_min_size(inner_rect.min, Vec2::new(inner_rect.width(), f32::INFINITY))));
    child.set_width(inner_rect.width());
    let r = add(&mut child);
    let used = child.min_rect();
    let card_rect = Rect::from_min_max(outer.min, Pos2::new(outer.min.x + avail, used.bottom() + pad));
    ui.painter().set(bg, Shape::Vec(glass_shapes(card_rect, 22.0, tint)));
    ui.allocate_rect(card_rect, Sense::hover());
    r
}

pub fn section_title(ui: &mut Ui, icon: &str, title: &str, subtitle: &str) {
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(Vec2::splat(34.0), Sense::hover());
        ui.painter().add(gradient_rrect(r, 11.0, |p| {
            let t = (p.y - r.top()) / r.height();
            lerp_color(RED_HOT, RED_DEEP, t)
        }));
        ui.painter().text(r.center(), Align2::CENTER_CENTER, icon, FontId::proportional(17.0), TEXT);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.label(egui::RichText::new(title).font(bold(17.0)).color(TEXT));
            ui.label(egui::RichText::new(subtitle).size(12.5).color(TEXT_DIM));
        });
    });
    ui.add_space(4.0);
}

// ---------------------------------------------------------------------------
// Controls
// ---------------------------------------------------------------------------

/// Big glossy red pill button.
pub fn pill_button(ui: &mut Ui, text: &str, primary: bool, width: f32) -> Response {
    let h = 44.0;
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, h), Sense::click());
    let hover = ui.ctx().animate_bool_with_time(resp.id, resp.hovered(), 0.18);
    let press = if resp.is_pointer_button_down_on() { 1.0 } else { 0.0 };
    let rect = rect.shrink(press * 1.5);
    let p = ui.painter();
    let shadow = Shadow {
        offset: [0, 10],
        blur: 28,
        spread: 0,
        color: if primary { rgba(RED, 0.35 + 0.25 * hover) } else { Color32::from_black_alpha(90) },
    };
    p.add(Shape::Rect(shadow.as_shape(rect, CornerRadius::same(22))));
    if primary {
        p.add(gradient_rrect(rect, h / 2.0, |pt| {
            let t = (pt.x - rect.left()) / rect.width() * 0.6 + (pt.y - rect.top()) / rect.height() * 0.4;
            stops(&[(0.0, lerp_color(RED_HOT, white(1.0), 0.1 * hover)), (0.55, RED), (1.0, RED_DEEP)], t)
        }));
    } else {
        for s in glass_shapes(rect, h / 2.0, 0.2 + 0.3 * hover) {
            p.add(s);
        }
    }
    // Glossy top half.
    let gloss = Rect::from_min_max(rect.min + Vec2::new(6.0, 2.0), Pos2::new(rect.right() - 6.0, rect.center().y));
    p.add(gradient_rrect(gloss, h / 2.0 - 3.0, |pt| {
        let t = (pt.y - gloss.top()) / gloss.height();
        lerp_color(white(0.32), white(0.0), t)
    }));
    p.text(rect.center(), Align2::CENTER_CENTER, text, bold(15.0), TEXT);
    resp
}

/// Small glass chip button.
pub fn chip(ui: &mut Ui, text: &str) -> Response {
    let galley = ui.painter().layout_no_wrap(text.to_owned(), FontId::proportional(13.0), TEXT);
    let size = Vec2::new(galley.size().x + 24.0, 28.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let hover = ui.ctx().animate_bool_with_time(resp.id, resp.hovered(), 0.15);
    for s in glass_shapes(rect, 14.0, 0.15 + 0.45 * hover) {
        ui.painter().add(s);
    }
    ui.painter().galley(rect.center() - galley.size() / 2.0, galley, TEXT);
    resp
}

/// iOS-style animated switch.
pub fn toggle(ui: &mut Ui, on: &mut bool) -> Response {
    let size = Vec2::new(50.0, 30.0);
    let (rect, mut resp) = ui.allocate_exact_size(size, Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    let t = ui.ctx().animate_bool_with_time(resp.id, *on, 0.22);
    let p = ui.painter();
    p.add(gradient_rrect(rect, 15.0, |pt| {
        let off = white(0.12);
        let on_c = lerp_color(RED_HOT, RED_DEEP, (pt.y - rect.top()) / rect.height());
        lerp_color(off, on_c, t)
    }));
    p.rect_stroke(rect, CornerRadius::same(15), Stroke::new(1.0, white(0.2)), StrokeKind::Inside);
    // Knob squishes while moving, like iOS.
    let stretch = 1.0 + 0.25 * (1.0 - (2.0 * t - 1.0).abs());
    let kh = 26.0;
    let kw = kh * stretch;
    let x = rect.left() + 2.0 + t * (rect.width() - 4.0 - kw);
    let knob = Rect::from_min_size(Pos2::new(x, rect.top() + 2.0), Vec2::new(kw, kh));
    let sh = Shadow { offset: [0, 3], blur: 8, spread: 0, color: Color32::from_black_alpha(90) };
    p.add(Shape::Rect(sh.as_shape(knob, CornerRadius::same(13))));
    p.add(gradient_rrect(knob, 13.0, |pt| {
        lerp_color(Color32::WHITE, Color32::from_rgb(236, 226, 230), (pt.y - knob.top()) / knob.height())
    }));
    resp
}

/// Glass slider with red liquid fill. Returns the response (changed() on drag).
pub fn slider(ui: &mut Ui, value: &mut f32, min: f32, max: f32, label: &str, unit: &str) -> Response {
    let width = ui.available_width();
    let h = 46.0;
    let (rect, mut resp) = ui.allocate_exact_size(Vec2::new(width, h), Sense::click_and_drag());
    let track = Rect::from_min_max(Pos2::new(rect.left(), rect.bottom() - 16.0), Pos2::new(rect.right(), rect.bottom() - 6.0));
    if let Some(pos) = resp.interact_pointer_pos() {
        if resp.dragged() || resp.clicked() {
            let t = ((pos.x - track.left()) / track.width()).clamp(0.0, 1.0);
            let nv = min + t * (max - min);
            let nv = (nv * 2.0).round() / 2.0;
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
    p.text(rect.left_top(), Align2::LEFT_TOP, label, FontId::proportional(13.0), TEXT_DIM);
    let val = if *value <= min + 0.01 && unit == "dB" && min <= -90.0 {
        "Off".to_owned()
    } else {
        format!("{:+.1} {unit}", value)
    };
    p.text(rect.right_top(), Align2::RIGHT_TOP, val, bold(13.0), TEXT);

    p.add(gradient_rrect(track, 5.0, |_| Color32::from_black_alpha(80)));
    p.rect_stroke(track, CornerRadius::same(5), Stroke::new(1.0, white(0.12)), StrokeKind::Inside);
    let fill = Rect::from_min_max(track.min, Pos2::new(track.left() + t * track.width(), track.bottom()));
    if fill.width() > 1.0 {
        p.add(gradient_rrect(fill, 5.0, |pt| {
            let x = (pt.x - track.left()) / track.width();
            lerp_color(RED_DEEP, RED_HOT, x)
        }));
    }
    let kc = Pos2::new(track.left() + t * track.width(), track.center().y);
    let kr = 10.0 + 2.0 * hover;
    let sh = Shadow { offset: [0, 3], blur: 10, spread: 0, color: rgba(RED, 0.5) };
    p.add(Shape::Rect(sh.as_shape(Rect::from_center_size(kc, Vec2::splat(kr * 2.0)), CornerRadius::same(kr as u8))));
    p.circle_filled(kc, kr, Color32::WHITE);
    p.circle_stroke(kc, kr, Stroke::new(1.0, white(0.6)));
    resp
}

/// Level meter (dBFS, -60..0) with smoothed peak.
pub fn meter(ui: &mut Ui, level: f32) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 8.0), Sense::hover());
    let db = crate::dsp::lin_to_db(level).clamp(-60.0, 0.0);
    let t = (db + 60.0) / 60.0;
    let p = ui.painter();
    p.add(gradient_rrect(rect, 4.0, |_| Color32::from_black_alpha(90)));
    let segs = 40;
    let gap = 2.0;
    let sw = (rect.width() - gap * (segs as f32 - 1.0)) / segs as f32;
    for i in 0..segs {
        let x0 = rect.left() + i as f32 * (sw + gap);
        let st = i as f32 / segs as f32;
        let lit = st < t;
        let c = if st > 0.9 {
            Color32::WHITE
        } else {
            lerp_color(RED_DEEP, RED_HOT, st / 0.9)
        };
        let col = if lit { c } else { white(0.06) };
        p.rect_filled(
            Rect::from_min_size(Pos2::new(x0, rect.top()), Vec2::new(sw, rect.height())),
            CornerRadius::same(2),
            col,
        );
    }
}

/// Pulsing status dot.
pub fn status_dot(ui: &mut Ui, on: bool, t: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(14.0), Sense::hover());
    let c = rect.center();
    let col = if on { OK } else { white(0.35) };
    if on {
        let ph = (t * 1.6).fract();
        ui.painter().circle_filled(c, 4.0 + 7.0 * ph, rgba(OK, 0.45 * (1.0 - ph)));
    }
    ui.painter().circle_filled(c, 4.5, col);
}

/// Segmented control with a sliding liquid pill.
pub fn segmented(ui: &mut Ui, id: &str, selected: &mut usize, items: &[&str]) {
    let item_w = 120.0;
    let h = 36.0;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(item_w * items.len() as f32 + 8.0, h + 8.0), Sense::hover());
    for s in glass_shapes(rect, (h + 8.0) / 2.0, 0.1) {
        ui.painter().add(s);
    }
    let anim = ui.ctx().animate_value_with_time(ui.scope_id().with(id), *selected as f32, 0.28);
    let pill = Rect::from_min_size(rect.min + Vec2::new(4.0 + anim * item_w, 4.0), Vec2::new(item_w, h));
    let sh = Shadow { offset: [0, 6], blur: 18, spread: 0, color: rgba(RED, 0.45) };
    ui.painter().add(Shape::Rect(sh.as_shape(pill, CornerRadius::same(18))));
    ui.painter().add(gradient_rrect(pill, h / 2.0, |pt| {
        lerp_color(RED_HOT, RED_DEEP, (pt.y - pill.top()) / pill.height())
    }));
    let gloss = Rect::from_min_max(pill.min + Vec2::new(8.0, 2.0), Pos2::new(pill.right() - 8.0, pill.center().y));
    ui.painter().add(gradient_rrect(gloss, h / 2.0 - 4.0, |pt| {
        lerp_color(white(0.35), white(0.0), (pt.y - gloss.top()) / gloss.height())
    }));
    for (i, label) in items.iter().enumerate() {
        let r = Rect::from_min_size(rect.min + Vec2::new(4.0 + i as f32 * item_w, 4.0), Vec2::new(item_w, h));
        let resp = ui.interact(r, ui.scope_id().with((id, i)), Sense::click());
        if resp.clicked() {
            *selected = i;
        }
        let col = if i == *selected { TEXT } else { TEXT_DIM };
        ui.painter().text(r.center(), Align2::CENTER_CENTER, *label, bold(14.0), col);
    }
}
