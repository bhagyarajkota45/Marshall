use egui::{Color32, Sense, Ui, pos2, vec2};

use super::{Action, App, Page};
use crate::theme::{self, icon};
use crate::ui::widgets::{self, named};

pub const CATEGORIES: &[&str] = &[
    "Hip-Hop/Rap",
    "Pop",
    "R&B",
    "Electronic",
    "Rock",
    "Jazz",
    "Indie",
    "K-Pop",
    "Latin",
    "Classical",
    "Country",
    "Afrobeats",
    "Charts",
    "Chill",
    "Workout",
];

const TINTS: [Color32; 15] = [
    Color32::from_rgb(188, 104, 57),
    Color32::from_rgb(167, 75, 112),
    Color32::from_rgb(118, 76, 163),
    Color32::from_rgb(46, 135, 132),
    Color32::from_rgb(158, 57, 78),
    Color32::from_rgb(47, 98, 136),
    Color32::from_rgb(46, 123, 111),
    Color32::from_rgb(174, 73, 133),
    Color32::from_rgb(163, 96, 39),
    Color32::from_rgb(112, 125, 145),
    Color32::from_rgb(143, 112, 61),
    Color32::from_rgb(174, 131, 38),
    Color32::from_rgb(57, 116, 171),
    Color32::from_rgb(78, 133, 115),
    Color32::from_rgb(173, 65, 59),
];

/// Recognizable performers for category artwork. Portraits come from Marshall's
/// ordinary artist search results and remain cached for the current session.
const GENRE_ARTISTS: [(&str, &str); 15] = [
    ("Hip-Hop/Rap", "Drake"),
    ("Pop", "Taylor Swift"),
    ("R&B", "SZA"),
    ("Electronic", "Calvin Harris"),
    ("Rock", "The Beatles"),
    ("Jazz", "Miles Davis"),
    ("Indie", "Mitski"),
    ("K-Pop", "BTS"),
    ("Latin", "Bad Bunny"),
    ("Classical", "Ludovico Einaudi"),
    ("Country", "Luke Combs"),
    ("Afrobeats", "Burna Boy"),
    ("Charts", "The Weeknd"),
    ("Chill", "Billie Eilish"),
    ("Workout", "Eminem"),
];

pub(super) fn schedule_artist_images(app: &mut App) {
    while app.genre_artist_pending.len() < 4 && app.genre_artist_cursor < GENRE_ARTISTS.len() {
        let (category, artist) = GENRE_ARTISTS[app.genre_artist_cursor];
        app.genre_artist_cursor += 1;
        let query = format!("{artist} official artist");
        app.genre_artist_pending
            .insert(query.clone(), category.to_owned());
        app.backend.request(crate::backend::Req::Search(query));
    }
}

pub(super) fn render(app: &mut App, ui: &mut Ui) {
    schedule_artist_images(app);
    ui.add_space(14.0);
    ui.label(
        egui::RichText::new("Browse Categories")
            .font(theme::bold(20.0))
            .color(theme::text()),
    );
    ui.add_space(12.0);
    let available = ui.available_width();
    let columns = if available >= 1080.0 {
        5
    } else if available >= 760.0 {
        4
    } else {
        3
    };
    let gap = 10.0;
    let card_w = ((available - gap * (columns as f32 - 1.0)) / columns as f32).clamp(132.0, 250.0);
    let mut picked = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(gap, gap);
        for (i, label) in CATEGORIES.iter().enumerate() {
            let (rect, resp) = ui.allocate_exact_size(vec2(card_w, 116.0), Sense::click());
            let resp = named(resp, &format!("Browse {label}"))
                .on_hover_cursor(egui::CursorIcon::PointingHand);
            let hover = theme::anim_bool(
                ui.ctx(),
                resp.id.with("category-hover"),
                resp.hovered(),
                0.14,
            );
            let portrait = app
                .genre_artist_images
                .get(*label)
                .filter(|thumbs| !thumbs.is_empty());
            if let Some(thumbs) = portrait {
                widgets::paint_cover_fill(ui, rect, thumbs, Color32::WHITE);
                ui.painter()
                    .rect_filled(rect, 13.0, TINTS[i].gamma_multiply(0.40 + hover * 0.08));
                ui.painter()
                    .rect_filled(rect, 13.0, Color32::from_black_alpha(44));
            } else {
                ui.painter()
                    .rect_filled(rect, 13.0, TINTS[i].gamma_multiply(0.68 + hover * 0.20));
                ui.painter()
                    .rect_filled(rect, 13.0, Color32::from_black_alpha(18));
                let glyph = [
                    icon::MUSIC,
                    icon::SPARKLES,
                    icon::DISC_3,
                    icon::RADIO,
                    icon::HEADPHONES,
                ][i % 5];
                widgets::icon_at(
                    ui,
                    pos2(rect.left() + 22.0, rect.top() + 24.0),
                    glyph,
                    20.0,
                    Color32::from_white_alpha(166),
                );
            }
            ui.painter().text(
                rect.left_bottom() + vec2(14.0, -14.0),
                egui::Align2::LEFT_BOTTOM,
                *label,
                theme::semibold(15.0),
                Color32::WHITE,
            );
            if resp.clicked() {
                picked = Some((*label).to_owned());
            }
        }
    });
    if let Some(query) = picked {
        app.actions.push(Action::Go(Page::Search(query)));
    }
}
