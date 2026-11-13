//! Страницы Physics 2D / Physics 3D: гравитация, решатель, сон тел и матрица столкновений
//! слоёв — треугольник флажков, как в Unity: строки — слои сверху вниз, столбцы — те же
//! слои справа налево, клетка (строка, столбец) — сталкиваются ли эти два слоя.

use egui::{pos2, vec2, Align2, Rect, Sense, Stroke};
use engine_core::settings::{LayerMatrix, Physics2dSettings, Physics3dSettings, TagsAndLayers};

use super::SettingsUi;
use crate::icons::{self, Icon};
use crate::state::Editor;
use crate::theme;

/// Сторона клетки матрицы.
const CELL: f32 = 18.0;

impl Editor {
    /// Клетка матрицы столкновений слоёв `a` и `b` на странице Physics 2D (`dimension = 2`)
    /// или Physics 3D (`3`).
    pub fn layer_matrix_cell_id(dimension: u8, a: u32, b: u32) -> egui::Id {
        let (low, high) = (a.min(b), a.max(b));
        egui::Id::new(("layer_matrix_cell", dimension, low, high))
    }
}

pub(super) fn physics2d(s: &mut SettingsUi<'_>, editor: &mut Editor) {
    let d = Physics2dSettings::default();
    let draft = editor.project_draft_mut();
    let names = draft.tags_and_layers.clone();
    let p = &mut draft.physics2d;
    s.group("World");
    s.vector("Gravity", "Acceleration of every 2D body, metres per second squared.", &mut p.gravity, d.gravity, 0.05);
    solver(s, &mut p.solver_iterations, &mut p.sleep_linear_threshold, &mut p.sleep_angular_threshold, (d.solver_iterations, d.sleep_linear_threshold, d.sleep_angular_threshold));
    s.group("Collisions");
    matrix(s, 2, &mut p.layer_matrix, &names);
}

pub(super) fn physics3d(s: &mut SettingsUi<'_>, editor: &mut Editor) {
    let d = Physics3dSettings::default();
    let draft = editor.project_draft_mut();
    let names = draft.tags_and_layers.clone();
    let p = &mut draft.physics3d;
    s.group("World");
    s.vector("Gravity", "Acceleration of every 3D body, metres per second squared.", &mut p.gravity, d.gravity, 0.05);
    solver(s, &mut p.solver_iterations, &mut p.sleep_linear_threshold, &mut p.sleep_angular_threshold, (d.solver_iterations, d.sleep_linear_threshold, d.sleep_angular_threshold));
    s.group("Collisions");
    matrix(s, 3, &mut p.layer_matrix, &names);
}

fn solver(s: &mut SettingsUi<'_>, iterations: &mut u32, linear: &mut f32, angular: &mut f32, d: (u32, f32, f32)) {
    s.group("Solver");
    s.number(
        "Solver iterations",
        "Contact solver passes per step: more keeps stacks of bodies stiffer and costs more time.",
        iterations,
        d.0,
        1..=64,
        0.1,
        "",
    );
    s.number(
        "Sleep threshold (linear)",
        "Speed below which a body may fall asleep and stop costing time (Rapier's normalized threshold). 0: never.",
        linear,
        d.1,
        0.0..=10.0,
        0.01,
        "",
    );
    s.number(
        "Sleep threshold (angular)",
        "Turning speed below which a body may fall asleep, in radians per second. 0: never.",
        angular,
        d.2,
        0.0..=10.0,
        0.01,
        " rad/s",
    );
}

/// Матрица столкновений по именованным слоям.
fn matrix(s: &mut SettingsUi<'_>, dimension: u8, matrix: &mut LayerMatrix, names: &TagsAndLayers) {
    let layers: Vec<(u32, String)> = (0..32u32)
        .filter(|i| names.layers.get(*i as usize).is_some_and(|n| !n.is_empty()))
        .map(|i| (i, names.layer_name(i)))
        .collect();
    let row = s.block(
        "Layer collision matrix",
        "Ticked: bodies on these two layers collide. Name more layers on the Tags and Layers page to see them here.",
        !matrix.is_default(),
        |ui| {
            ui.horizontal(|ui| {
                if theme::ghost_button(ui, "Enable All").clicked() {
                    *matrix = LayerMatrix::default();
                }
                if theme::ghost_button(ui, "Disable All").clicked() {
                    for (a, _) in &layers {
                        for (b, _) in &layers {
                            matrix.set(*a, *b, false);
                        }
                    }
                }
            });
            ui.add_space(theme::SP_S);
            paint_matrix(ui, dimension, matrix, &layers);
        },
    );
    if row.reset {
        *matrix = LayerMatrix::default();
    }
}

fn paint_matrix(ui: &mut egui::Ui, dimension: u8, matrix: &mut LayerMatrix, layers: &[(u32, String)]) {
    if layers.is_empty() {
        return;
    }
    let font = theme::body();
    let label_w = layers
        .iter()
        .map(|(_, name)| ui.painter().layout_no_wrap(name.clone(), font.clone(), theme::TEXT).size().x)
        .fold(0.0_f32, f32::max)
        + theme::SP_M;
    let header_h = label_w;
    let n = layers.len();
    let size = vec2(label_w + CELL * n as f32 + theme::SP_S, header_h + CELL * n as f32 + theme::SP_S);
    let (area, response) = ui.allocate_exact_size(size, Sense::hover());
    let painter = ui.painter().clone();
    let origin = pos2(area.left() + label_w, area.top() + header_h);
    // Под курсором: строка и столбец (номера в списке).
    let hovered = response.hover_pos().and_then(|p| {
        let col = ((p.x - origin.x) / CELL).floor();
        let row = ((p.y - origin.y) / CELL).floor();
        (col >= 0.0 && row >= 0.0 && (col as usize) < n && (row as usize) < n && (row as usize) + (col as usize) < n)
            .then_some((row as usize, col as usize))
    });
    // Столбцы идут в обратном порядке: столбец c — слой layers[n - 1 - c].
    let column_layer = |c: usize| n - 1 - c;
    for c in 0..n {
        let (_, name) = &layers[column_layer(c)];
        let lit = hovered.is_some_and(|(r, hc)| hc == c || column_layer(c) == r);
        let galley = painter.layout_no_wrap(name.clone(), font.clone(), if lit { theme::TEXT_STRONG } else { theme::TEXT_MUTED });
        // Подпись столбца вертикально, снизу вверх, у основания столбца.
        let x = origin.x + c as f32 * CELL + CELL * 0.5 - galley.size().y * 0.5;
        let y = origin.y - theme::SP_S;
        painter.add(egui::epaint::TextShape::new(pos2(x, y), galley, theme::TEXT_MUTED).with_angle(-std::f32::consts::FRAC_PI_2));
    }
    for (r, (layer_a, name)) in layers.iter().enumerate() {
        let y = origin.y + r as f32 * CELL;
        let lit = hovered.is_some_and(|(hr, hc)| hr == r || column_layer(hc) == r);
        painter.text(
            pos2(origin.x - theme::SP_S, y + CELL * 0.5),
            Align2::RIGHT_CENTER,
            name,
            font.clone(),
            if lit { theme::TEXT_STRONG } else { theme::TEXT_MUTED },
        );
        for c in 0..n - r {
            let layer_b = layers[column_layer(c)].0;
            let cell = Rect::from_min_size(pos2(origin.x + c as f32 * CELL, y), vec2(CELL, CELL));
            let id = Editor::layer_matrix_cell_id(dimension, *layer_a, layer_b);
            let click = ui.interact(cell, id, Sense::click());
            let on = matrix.collides(*layer_a, layer_b);
            let box_rect = cell.shrink(2.5);
            let fill = if on { theme::accent_soft() } else { theme::BG_INSET };
            let stroke = if click.hovered() { theme::LINE_HOVER } else { theme::LINE_STRONG };
            painter.rect(box_rect, theme::R_S, fill, Stroke::new(1.0_f32, stroke));
            if on {
                icons::paint(&painter, box_rect.shrink(1.5), Icon::Check, theme::TEXT_STRONG);
            }
            let other = layers[column_layer(c)].1.clone();
            if click.on_hover_text(format!("{name} × {other}")).clicked() {
                matrix.set(*layer_a, layer_b, !on);
            }
        }
    }
}
