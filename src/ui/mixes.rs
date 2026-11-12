use std::collections::{HashMap, HashSet};

use egui::{Ui, vec2};

use super::{Action, App};
use crate::backend::Req;
use crate::model::{ShelfKind, Target, Track};
use crate::theme::{self, icon};
use crate::ui::{discovery, pages, widgets};

const MODES: [&str; 9] = [
    "Top tracks",
    "Recent",
    "Song radio",
    "Similar artists",
    "Tag",
    "Mix",
    "Recommendations",
    "Never heard",
    "Library",
];
const PERIODS: [&str; 6] = [
    "Overall",
    "12 months",
    "6 months",
    "3 months",
    "1 month",
    "7 days",
];
const PERIOD_DAYS: [Option<i64>; 6] = [None, Some(365), Some(180), Some(90), Some(30), Some(7)];

pub(super) fn render(app: &mut App, ui: &mut Ui) {
    pages::page_title(ui, "Mix Lab");
    let mode = (app.mix_mode as usize).min(MODES.len() - 1);
    let period = (app.mix_period as usize).min(PERIODS.len() - 1);
    let period_days = PERIOD_DAYS[period];

    ui.label(
        egui::RichText::new(format!("{} tracks · {}", app.mix_tracks.len(), MODES[mode]))
            .font(theme::regular(13.0))
            .color(theme::text_dim()),
    );
    ui.add_space(12.0);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
        for (i, label) in MODES.iter().enumerate() {
            if widgets::chip(ui, label, mode == i).clicked() {
                app.mix_mode = i as u8;
                app.mix_tracks.clear();
            }
        }
    });
    ui.add_space(8.0);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
        for (i, label) in PERIODS.iter().enumerate() {
            if widgets::chip(ui, label, period == i).clicked() {
                app.mix_period = i as u8;
                app.mix_tracks.clear();
            }
        }
    });

    if mode == 4 {
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("Tag")
                    .font(theme::semibold(13.0))
                    .color(theme::text_dim()),
            );
            egui::ComboBox::from_id_salt("mix-lab-tag")
                .selected_text(&app.mix_tag)
                .show_ui(ui, |ui| {
                    for tag in discovery::CATEGORIES {
                        ui.selectable_value(&mut app.mix_tag, (*tag).to_owned(), *tag);
                    }
                });
        });
    }

    ui.add_space(12.0);
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(format!("{} tracks", app.mix_limit))
                .font(theme::regular(13.0))
                .color(theme::text()),
        );
        ui.add(
            egui::Slider::new(&mut app.mix_limit, 5..=100)
                .show_value(false)
                .text("Playlist size"),
        );
    });
    ui.add_space(10.0);
    let loading = app.mix_fetch_radio.is_some() || app.mix_fetch_search.is_some();
    if widgets::pill(
        ui,
        if loading {
            "Building mix…"
        } else {
            "Generate"
        },
        Some(icon::SPARKLES),
        !loading,
    )
    .clicked()
        && !loading
    {
        app.mix_tracks.clear();
        match mode {
            2 => request_radio(app, period_days),
            4 => {
                app.mix_fetch_search = Some(app.mix_tag.clone());
                app.backend.request(Req::Search(app.mix_tag.clone()));
            }
            _ => app.mix_tracks = generate_local(app, mode, period_days, app.mix_limit as usize),
        }
    }

    if loading {
        ui.add_space(28.0);
        ui.spinner();
        ui.label(
            egui::RichText::new("Finding tracks for your mix…")
                .font(theme::regular(13.0))
                .color(theme::text_dim()),
        );
        return;
    }
    if app.mix_tracks.is_empty() {
        ui.add_space(48.0);
        ui.vertical_centered(|ui| {
            widgets::icon_at(
                ui,
                ui.cursor().min + vec2(ui.available_width() * 0.5, 22.0),
                icon::SPARKLES,
                28.0,
                theme::text_dim(),
            );
            ui.add_space(28.0);
            ui.label(
                egui::RichText::new("No mix yet")
                    .font(theme::bold(19.0))
                    .color(theme::text()),
            );
            ui.label(
                egui::RichText::new("Choose a mode and generate a playlist from your listening.")
                    .font(theme::regular(13.0))
                    .color(theme::text_dim()),
            );
        });
        return;
    }

    ui.add_space(14.0);
    ui.horizontal(|ui| {
        let tracks = app.mix_tracks.clone();
        if widgets::pill(ui, "Play", Some(icon::PLAY), true).clicked() {
            app.actions.push(Action::Play(tracks.clone(), 0));
        }
        if widgets::pill(ui, "Shuffle", Some(icon::SHUFFLE), false).clicked() {
            let mut shuffled = tracks.clone();
            super::shuffle(&mut shuffled);
            app.actions.push(Action::Play(shuffled, 0));
        }
        if widgets::pill(ui, "Save as playlist", Some(icon::LIST_PLUS), false).clicked() {
            app.actions.push(Action::NewPlaylist(tracks));
        }
    });
    ui.add_space(12.0);
    let accent = app.look().accent(app.accent);
    let time = ui.input(|i| i.time);
    let mut play = None;
    widgets::visible_rows(ui, app.mix_tracks.len(), |ui, i| {
        let track = &app.mix_tracks[i];
        let opts = widgets::RowOpts {
            number: Some(i + 1),
            show_album: true,
            current: app.current_id() == Some(track.id.as_str()),
            playing: app.playing(),
            accent,
            time,
            liked: app.liked.contains(&track.id),
            added_by: None,
            pick: false,
            selected: None,
            remove: None,
        };
        if widgets::track_row(ui, track, &opts, &mut app.actions).clicked() {
            play = Some(i);
        }
    });
    if let Some(i) = play {
        app.actions.push(Action::Play(app.mix_tracks.clone(), i));
    }
}

fn request_radio(app: &mut App, period_days: Option<i64>) {
    let seed = app
        .playing_track()
        .cloned()
        .or_else(|| recent_tracks(app, period_days).into_iter().next());
    let Some(seed) = seed else {
        app.toast("Play or listen to a song first to build a song radio");
        return;
    };
    app.mix_fetch_radio = Some(seed.id.clone());
    app.backend.request(Req::MixRadio {
        seed: seed.id.clone(),
        song_of: app.song_of(&seed),
    });
}

fn generate_local(app: &App, mode: usize, period_days: Option<i64>, limit: usize) -> Vec<Track> {
    let recommendations = recommendation_tracks(app);
    let all = all_available_tracks(app);
    let recent = recent_tracks(app, period_days);
    let top = top_tracks(app, period_days);
    let mut result = match mode {
        0 => {
            if top.is_empty() {
                recent.clone()
            } else {
                top
            }
        }
        1 => recent,
        3 => {
            let seed = app
                .playing_track()
                .cloned()
                .or_else(|| recent_tracks(app, period_days).into_iter().next());
            let names: HashSet<String> = seed
                .as_ref()
                .into_iter()
                .flat_map(|t| t.artists.iter())
                .map(|a| a.name.to_lowercase())
                .collect();
            all.into_iter()
                .filter(|t| {
                    !seed.as_ref().is_some_and(|s| s.id == t.id)
                        && t.artists
                            .iter()
                            .any(|a| names.contains(&a.name.to_lowercase()))
                })
                .collect()
        }
        5 => {
            let mut mixed = Vec::new();
            let mut seen = HashSet::new();
            for i in 0..limit {
                let source = match i % 3 {
                    0 => &top,
                    1 => &recent,
                    _ => &recommendations,
                };
                if let Some(track) = source
                    .get(i / 3)
                    .or_else(|| source.get(i % source.len().max(1)))
                    && seen.insert(track.id.clone())
                {
                    mixed.push(track.clone());
                }
            }
            mixed
        }
        6 => recommendations.clone(),
        7 => {
            let heard: HashSet<String> = app
                .listening_stats
                .entries()
                .iter()
                .map(|e| e.track.id.clone())
                .collect();
            recommendations
                .iter()
                .filter(|t| !heard.contains(&t.id))
                .cloned()
                .collect()
        }
        8 => {
            let liked = app.liked.clone();
            let mut library = app
                .lists
                .get(super::lists::LIKED_MUSIC)
                .map(|o| o.c.tracks.clone())
                .unwrap_or_default();
            library.extend(all.into_iter().filter(|t| liked.contains(&t.id)));
            unique(library)
        }
        _ => all,
    };
    if result.is_empty() {
        result = recommendations;
    }
    result.truncate(limit);
    result
}

fn top_tracks(app: &App, period_days: Option<i64>) -> Vec<Track> {
    let cutoff = period_days.map(|days| crate::listening_stats::unix_now() - days * 86_400);
    let mut counts: HashMap<String, (Track, usize)> = HashMap::new();
    for entry in app
        .listening_stats
        .entries()
        .iter()
        .filter(|e| cutoff.is_none_or(|c| e.started_at >= c))
    {
        let slot = counts
            .entry(entry.track.id.clone())
            .or_insert_with(|| (entry.track.clone(), 0));
        slot.1 += 1;
    }
    let mut tracks: Vec<_> = counts.into_values().collect();
    tracks.sort_by(|a, b| b.1.cmp(&a.1));
    let mut out: Vec<_> = tracks.into_iter().map(|(t, _)| t).collect();
    if out.is_empty() {
        out.extend(app.history.iter().flatten().cloned());
    }
    unique(out)
}

fn recent_tracks(app: &App, period_days: Option<i64>) -> Vec<Track> {
    let cutoff = period_days.map(|days| crate::listening_stats::unix_now() - days * 86_400);
    let mut entries: Vec<_> = app
        .listening_stats
        .entries()
        .iter()
        .filter(|e| cutoff.is_none_or(|c| e.started_at >= c))
        .collect();
    entries.sort_by_key(|e| std::cmp::Reverse(e.started_at));
    let recorded = unique(entries.into_iter().map(|e| e.track.clone()).collect());
    if recorded.is_empty() {
        app.history.clone().unwrap_or_default()
    } else {
        recorded
    }
}

fn recommendation_tracks(app: &App) -> Vec<Track> {
    let mut out = Vec::new();
    if let Some(home) = &app.home {
        for shelf in &home.shelves {
            match &shelf.kind {
                ShelfKind::Songs { tracks, .. } => out.extend(tracks.iter().cloned()),
                ShelfKind::Cards(cards) => {
                    out.extend(cards.iter().filter_map(|c| match &c.target {
                        Target::Song(t) => Some(t.clone()),
                        _ => None,
                    }))
                }
            }
        }
    }
    if let Some(explore) = &app.explore {
        out.extend(explore.top.iter().cloned());
    }
    unique(out)
}

fn all_available_tracks(app: &App) -> Vec<Track> {
    let mut out: Vec<_> = app
        .listening_stats
        .entries()
        .iter()
        .map(|e| e.track.clone())
        .collect();
    out.extend(app.history.iter().flatten().cloned());
    out.extend(recommendation_tracks(app));
    out.extend(app.playing_track().cloned());
    if let Some(library) = app.lists.get(super::lists::LIKED_MUSIC) {
        out.extend(library.c.tracks.iter().cloned());
    }
    unique(out)
}

fn unique(tracks: Vec<Track>) -> Vec<Track> {
    let mut ids = HashSet::new();
    tracks
        .into_iter()
        .filter(|t| !t.id.is_empty() && ids.insert(t.id.clone()))
        .collect()
}
