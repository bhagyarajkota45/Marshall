use std::collections::{BTreeMap, BTreeSet, HashMap};

use egui::{Align2, Color32, Rect, Sense, Ui, Vec2, pos2, vec2};

use super::{Action, App, Page};
use crate::listening_stats::{self, Entry};
use crate::theme::{self};
use crate::ui::{discovery, pages, widgets};

const RANGES: [&str; 3] = ["7 Days", "30 Days", "All Time"];
const TABS: [&str; 6] = [
    "Overview",
    "Top Tracks",
    "Top Artists",
    "Top Albums",
    "History",
    "Genres",
];

#[derive(Default)]
struct Summary {
    entries: Vec<Entry>,
    plays: usize,
    seconds: u64,
    artists: usize,
    tracks: Vec<(crate::model::Track, usize, u64)>,
    top_artists: Vec<(String, usize, crate::model::Track)>,
    albums: Vec<(String, usize, crate::model::Track)>,
    daily: BTreeMap<i32, f64>,
    hours: [f64; 24],
    day_ids: BTreeSet<i32>,
}

pub(super) fn render(app: &mut App, ui: &mut Ui) {
    ui.add_space(18.0);
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new("Statistics")
                .font(theme::bold(34.0))
                .color(theme::text()),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            for (i, label) in RANGES.iter().enumerate().rev() {
                if widgets::chip(ui, label, app.statistics_period as usize == i).clicked() {
                    app.statistics_period = i as u8;
                }
            }
        });
    });
    ui.add_space(12.0);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        for (i, label) in TABS.iter().enumerate() {
            if widgets::chip(ui, label, app.statistics_tab as usize == i).clicked() {
                app.statistics_tab = i as u8;
            }
        }
    });
    let summary = summarize(app.listening_stats.entries(), app.statistics_period);
    match app.statistics_tab {
        1 => ranked_tracks(app, ui, &summary),
        2 => ranked_artists(app, ui, &summary),
        3 => ranked_albums(app, ui, &summary),
        4 => listening_history(app, ui, &summary),
        5 => discovery::render(app, ui),
        _ => overview(app, ui, &summary),
    }
}

fn summarize(entries: &[Entry], period: u8) -> Summary {
    let now = listening_stats::unix_now();
    let cutoff = match period {
        0 => Some(now - 7 * 86_400),
        1 => Some(now - 30 * 86_400),
        _ => None,
    };
    let mut out = Summary::default();
    let mut track_map: HashMap<String, (crate::model::Track, usize, u64)> = HashMap::new();
    let mut artist_map: HashMap<String, (String, usize, crate::model::Track)> = HashMap::new();
    let mut album_map: HashMap<String, (String, usize, crate::model::Track)> = HashMap::new();
    for entry in entries
        .iter()
        .filter(|e| cutoff.is_none_or(|c| e.started_at >= c))
    {
        out.seconds = out
            .seconds
            .saturating_add(entry.listened_seconds.max(0.0) as u64);
        let threshold = entry
            .track
            .duration
            .map_or(30.0, |d| ((d as f64 / 2.0).min(240.0)).max(30.0));
        let counted = entry.listened_seconds >= threshold;
        if counted {
            out.plays += 1;
            let slot = track_map
                .entry(entry.track.id.clone())
                .or_insert_with(|| (entry.track.clone(), 0, 0));
            slot.1 += 1;
            slot.2 = slot
                .2
                .saturating_add(entry.listened_seconds.max(0.0) as u64);
            for artist in &entry.track.artists {
                let key = artist.name.trim().to_lowercase();
                if !key.is_empty() {
                    let slot = artist_map
                        .entry(key)
                        .or_insert_with(|| (artist.name.clone(), 0, entry.track.clone()));
                    slot.1 += 1;
                }
            }
            if let Some(album) = &entry.track.album {
                let key = album.name.trim().to_lowercase();
                if !key.is_empty() {
                    let slot = album_map
                        .entry(key)
                        .or_insert_with(|| (album.name.clone(), 0, entry.track.clone()));
                    slot.1 += 1;
                }
            }
        }
        let (_, _, _, hour, day) = listening_stats::time_parts(entry.started_at);
        out.day_ids.insert(day);
        out.daily
            .entry(day)
            .and_modify(|v| *v += entry.listened_seconds)
            .or_insert(entry.listened_seconds);
        out.hours[hour as usize] += entry.listened_seconds;
        out.entries.push(entry.clone());
    }
    out.entries.sort_by_key(|e| std::cmp::Reverse(e.started_at));
    out.tracks = track_map.into_values().collect();
    out.tracks
        .sort_by(|a, b| b.1.cmp(&a.1).then_with(|| b.2.cmp(&a.2)));
    out.artists = artist_map.len();
    out.top_artists = artist_map.into_values().collect();
    out.top_artists.sort_by(|a, b| {
        b.1.cmp(&a.1)
            .then_with(|| a.0.to_lowercase().cmp(&b.0.to_lowercase()))
    });
    out.albums = album_map.into_values().collect();
    out.albums.sort_by(|a, b| {
        b.1.cmp(&a.1)
            .then_with(|| a.0.to_lowercase().cmp(&b.0.to_lowercase()))
    });
    out
}

fn overview(app: &mut App, ui: &mut Ui, s: &Summary) {
    ui.label(
        egui::RichText::new(if s.entries.is_empty() {
            "Plays on this device will build your listening charts here.".to_owned()
        } else {
            format!(
                "{} listening sessions recorded on this device.",
                s.entries.len()
            )
        })
        .font(theme::regular(13.0))
        .color(theme::text_dim()),
    );
    ui.add_space(12.0);
    pages::settings_card(ui, |ui| {
        ui.horizontal(|ui| {
            let width = ((ui.available_width() - 24.0) / 3.0).max(100.0);
            metric(ui, width, &duration_label(s.seconds), "Time listened");
            metric(ui, width, &s.plays.to_string(), "Plays");
            let peak = s
                .hours
                .iter()
                .enumerate()
                .max_by(|a, b| a.1.total_cmp(b.1))
                .filter(|(_, v)| **v > 0.0)
                .map(|(h, _)| hour_label(h as u8))
                .unwrap_or_else(|| "—".to_owned());
            metric(ui, width, &peak, "Rhythm · peak hour");
        });
    });
    ui.add_space(16.0);
    ui.columns(2, |cols| {
        chart_panel(
            &mut cols[0],
            "Daily activity",
            &daily_values(s, app.statistics_period),
            false,
        );
        chart_panel(&mut cols[1], "Time of day · UTC", &s.hours.to_vec(), true);
    });
    ui.add_space(14.0);
    ui.columns(2, |cols| {
        pages::section_label(&mut cols[0], "Top tracks", Some("View all →"));
        ranked_track_rows(app, &mut cols[0], s, 3);
        pages::section_label(&mut cols[1], "Top artists", Some("View all →"));
        ranked_artist_rows(app, &mut cols[1], s, 3);
    });
}

fn metric(ui: &mut Ui, width: f32, value: &str, label: &str) {
    ui.allocate_ui_with_layout(
        vec2(width, 82.0),
        egui::Layout::top_down(egui::Align::Min),
        |ui| {
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new(label.to_uppercase())
                    .font(theme::semibold(11.5))
                    .color(theme::text_dim()),
            );
            ui.add_space(6.0);
            ui.label(
                egui::RichText::new(value)
                    .font(theme::bold(24.0))
                    .color(theme::text()),
            );
        },
    );
}

fn chart_panel(ui: &mut Ui, title: &str, values: &[f64], hourly: bool) {
    pages::settings_card(ui, |ui| {
        ui.label(
            egui::RichText::new(title)
                .font(theme::semibold(16.0))
                .color(theme::text()),
        );
        ui.add_space(4.0);
        let peak = values.iter().copied().fold(0.0_f64, f64::max).max(1.0);
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 134.0), Sense::hover());
        let bars = if hourly { 24 } else { values.len().max(1) };
        let gap = 4.0;
        let w = ((rect.width() - gap * (bars - 1) as f32) / bars as f32).max(2.0);
        let accent = Color32::from_rgb(73, 190, 244);
        for i in 0..bars {
            let val = values.get(i).copied().unwrap_or_default();
            let h = (val / peak) as f32 * 88.0;
            let x = rect.left() + i as f32 * (w + gap);
            let bar = Rect::from_min_max(
                pos2(x, rect.bottom() - 22.0 - h),
                pos2(x + w, rect.bottom() - 22.0),
            );
            ui.painter().rect_filled(
                bar,
                3.0,
                accent.gamma_multiply(if val > 0.0 {
                    0.56 + (h / 88.0) * 0.44
                } else {
                    0.10
                }),
            );
        }
        ui.painter().text(
            rect.left_bottom() + vec2(0.0, -2.0),
            Align2::LEFT_BOTTOM,
            if hourly { "12 AM" } else { "Earlier" },
            theme::regular(10.0),
            theme::text_dim(),
        );
        ui.painter().text(
            rect.right_bottom() + vec2(0.0, -2.0),
            Align2::RIGHT_BOTTOM,
            if hourly { "11 PM" } else { "Today" },
            theme::regular(10.0),
            theme::text_dim(),
        );
    });
}

fn daily_values(s: &Summary, period: u8) -> Vec<f64> {
    let today = listening_stats::time_parts(listening_stats::unix_now()).4;
    let count = if period == 0 {
        7
    } else if period == 1 {
        30
    } else {
        14
    };
    (0..count)
        .map(|i| {
            s.daily
                .get(&(today - count as i32 + 1 + i as i32))
                .copied()
                .unwrap_or_default()
        })
        .collect()
}

fn ranked_tracks(app: &mut App, ui: &mut Ui, s: &Summary) {
    pages::section_label(ui, "Top tracks", None);
    ranked_track_rows(app, ui, s, usize::MAX);
}

fn ranked_track_rows(app: &mut App, ui: &mut Ui, s: &Summary, limit: usize) {
    if s.tracks.is_empty() {
        empty(ui);
        return;
    }
    pages::settings_card(ui, |ui| {
        for (i, (track, count, _)) in s.tracks.iter().take(limit).enumerate() {
            stat_track_row(
                ui,
                i + 1,
                track,
                &format!("{count} plays"),
                &mut app.actions,
            );
        }
    });
}

fn ranked_artists(app: &mut App, ui: &mut Ui, s: &Summary) {
    pages::section_label(ui, "Top artists", None);
    ranked_artist_rows(app, ui, s, usize::MAX);
}

fn ranked_artist_rows(app: &mut App, ui: &mut Ui, s: &Summary, limit: usize) {
    if s.top_artists.is_empty() {
        empty(ui);
        return;
    }
    pages::settings_card(ui, |ui| {
        for (i, (name, plays, track)) in s.top_artists.iter().take(limit).enumerate() {
            simple_rank_row(
                ui,
                i + 1,
                name,
                &format!("{plays} plays"),
                &track.thumbs,
                || {
                    app.actions.push(Action::Go(Page::Search(name.clone())));
                },
            );
        }
    });
}

fn ranked_albums(app: &mut App, ui: &mut Ui, s: &Summary) {
    pages::section_label(ui, "Top albums", None);
    if s.albums.is_empty() {
        empty(ui);
        return;
    }
    pages::settings_card(ui, |ui| {
        for (i, (name, plays, track)) in s.albums.iter().enumerate() {
            simple_rank_row(
                ui,
                i + 1,
                name,
                &format!("{plays} plays"),
                &track.thumbs,
                || {
                    if let Some(album) = &track.album {
                        if let Some(id) = &album.id {
                            app.actions
                                .push(Action::Open(crate::model::Target::Album(id.clone())));
                        } else {
                            app.actions.push(Action::Go(Page::Search(name.clone())));
                        }
                    }
                },
            );
        }
    });
}

fn listening_history(app: &mut App, ui: &mut Ui, s: &Summary) {
    pages::section_label(ui, "History on this device", None);
    if s.entries.is_empty() {
        empty(ui);
        return;
    }
    pages::settings_card(ui, |ui| {
        for (i, entry) in s.entries.iter().take(100).enumerate() {
            stat_track_row(
                ui,
                i + 1,
                &entry.track,
                &format!(
                    "{} · {} listened",
                    date_label(entry.started_at),
                    duration_label(entry.listened_seconds as u64)
                ),
                &mut app.actions,
            );
        }
    });
}

fn stat_track_row(
    ui: &mut Ui,
    rank: usize,
    track: &crate::model::Track,
    subtitle: &str,
    actions: &mut Vec<Action>,
) {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 64.0), Sense::click());
    let response = widgets::named(response, &format!("Play {}", track.title));
    if response.hovered() {
        ui.painter().rect_filled(rect, 8.0, theme::glass(0.08));
    }
    let cover = Rect::from_min_size(rect.min + vec2(8.0, 8.0), Vec2::splat(48.0));
    ui.painter().text(
        pos2(rect.left() + 10.0, rect.center().y),
        Align2::LEFT_CENTER,
        rank.to_string(),
        theme::semibold(13.0),
        theme::text_dim(),
    );
    let cover = cover.translate(vec2(26.0, 0.0));
    widgets::paint_cover(ui, cover, &track.thumbs, theme::thumb_radius(), 1.0);
    widgets::text_at(
        ui,
        pos2(cover.right() + 12.0, rect.top() + 13.0),
        &track.title,
        theme::semibold(14.0),
        theme::text(),
        (rect.width() - 180.0).max(80.0),
    );
    widgets::text_at(
        ui,
        pos2(cover.right() + 12.0, rect.top() + 38.0),
        &track.artist_line(),
        theme::regular(12.0),
        theme::text_dim(),
        (rect.width() - 180.0).max(80.0),
    );
    ui.painter().text(
        pos2(rect.right() - 10.0, rect.center().y),
        Align2::RIGHT_CENTER,
        subtitle,
        theme::regular(12.0),
        theme::text_dim(),
    );
    if response.clicked() {
        actions.push(Action::Play(vec![track.clone()], 0));
    }
}

fn simple_rank_row(
    ui: &mut Ui,
    rank: usize,
    title: &str,
    subtitle: &str,
    thumbs: &[crate::model::Thumb],
    mut on_click: impl FnMut(),
) {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 62.0), Sense::click());
    let response = widgets::named(response, title);
    if response.hovered() {
        ui.painter().rect_filled(rect, 8.0, theme::glass(0.08));
    }
    let cover = Rect::from_min_size(rect.min + vec2(34.0, 7.0), Vec2::splat(48.0));
    ui.painter().text(
        pos2(rect.left() + 10.0, rect.center().y),
        Align2::LEFT_CENTER,
        rank.to_string(),
        theme::semibold(13.0),
        theme::text_dim(),
    );
    widgets::paint_cover(ui, cover, thumbs, theme::thumb_radius(), 1.0);
    widgets::text_at(
        ui,
        pos2(cover.right() + 12.0, rect.top() + 14.0),
        title,
        theme::semibold(14.0),
        theme::text(),
        (rect.width() - 160.0).max(80.0),
    );
    widgets::text_at(
        ui,
        pos2(cover.right() + 12.0, rect.top() + 38.0),
        subtitle,
        theme::regular(12.0),
        theme::text_dim(),
        (rect.width() - 160.0).max(80.0),
    );
    if response.clicked() {
        on_click();
    }
}

fn empty(ui: &mut Ui) {
    ui.add_space(14.0);
    ui.label(egui::RichText::new("No listening data for this period yet. Keep playing music in Marshall to build these charts.").font(theme::regular(14.0)).color(theme::text_dim()));
}

fn duration_label(seconds: u64) -> String {
    let hours = seconds / 3600;
    let mins = (seconds % 3600) / 60;
    if hours > 0 {
        format!("{hours} h {mins} min")
    } else {
        format!("{mins} min")
    }
}

fn hour_label(hour: u8) -> String {
    let h = hour % 12;
    format!(
        "{}:{} {}",
        if h == 0 { 12 } else { h },
        "00",
        if hour < 12 { "AM" } else { "PM" }
    )
}

fn date_label(timestamp: i64) -> String {
    let (year, month, day, _, _) = listening_stats::time_parts(timestamp);
    format!("{year}-{month:02}-{day:02}")
}
