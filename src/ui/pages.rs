//! Page bodies. Every page fades and slides in when navigated to.

use std::collections::HashSet;
use std::sync::atomic::Ordering;

use egui::{Align2, Color32, Rect, RichText, Sense, Ui, Vec2, pos2, vec2};

use super::widgets::{
    self, RowOpts, card, chip, h_scroll, named, paint_cover, pill, shelf_header, track_row,
};
use super::{Action, App, LibTab, Page, Selection};
use crate::backend::compact;
use crate::model::{Card, ShelfKind, Target, Track};
use crate::theme::{self, icon};

const CARD: f32 = 180.0;

/// How many tiles Apple fits across a feed row at this content width. The site
/// measures from its own viewport, which includes the navigation column this page
/// already sits past.
fn feed_columns(width: f32, square: bool) -> usize {
    let viewport = width + 260.0;
    match square {
        true if viewport < 1260.0 => 4,
        true if viewport < 1580.0 => 5,
        true => 6,
        false if viewport < 1260.0 => 2,
        false if viewport < 1580.0 => 3,
        false => 4,
    }
}

/// One tile of an Apple feed row: a share of the content on wide windows, the site's
/// own narrow width below its breakpoint.
fn feed_tile(ui: &Ui, square: bool) -> f32 {
    let width = ui.available_width();
    let min = if square { 144.0 } else { 270.0 };
    if width < 1024.0 {
        return min;
    }
    let cols = feed_columns(width, square);
    ((width - 20.0 * (cols - 1) as f32) / cols as f32).max(min)
}

pub fn page(app: &mut App, ui: &mut Ui, now: f64) {
    let t = theme::ease_out(((now - app.page_changed) / 0.32) as f32);
    if t < 1.0 {
        // The fade-in finishes even when nothing else is drawing frames.
        ui.ctx().request_repaint();
    }
    ui.set_opacity(0.15 + 0.85 * t);
    let page = app.page.clone();
    // Each page (each album, each search) keeps its own scroll position.
    let mut area = egui::ScrollArea::vertical()
        .id_salt(("page", &page))
        .auto_shrink(false);
    // At the window's right edge: clear of where it resizes the window.
    ui.spacing_mut().scroll.bar_outer_margin += super::titlebar::edge_inset();
    // Dev hook: YTM_SCROLL=<px> pins the page scroll, for screenshots of lower sections.
    if let Some(y) = super::dev_var("YTM_SCROLL").and_then(|v| v.parse::<f32>().ok()) {
        area = area.vertical_scroll_offset(y);
    }
    let out = area.show(ui, |ui| {
        ui.set_max_width((ui.available_width() - 20.0).max(0.0));
        ui.add_space(18.0 * (1.0 - t));
        match &page {
            Page::Home => home(app, ui, now),
            Page::New => new(app, ui, now),
            Page::Statistics => super::statistics::render(app, ui),
            Page::Search(q) => search(app, ui, q, now),
            Page::Album(id) | Page::Playlist(id) => collection(app, ui, id, now),
            Page::Artist(id) => artist(app, ui, id, now),
            Page::Library(tab) => library(app, ui, *tab, now),
            Page::MixLab => super::mixes::render(app, ui),
            Page::Podcasts => podcasts(app, ui, now),
            Page::Downloads => downloads(app, ui, now),
            Page::LocalMusic => local_music(app, ui),
            Page::Settings => settings(app, ui),
            Page::Transfer => super::transfer::page(app, ui, now),
            Page::Jam => super::jam::page(app, ui),
        }
        // Room to scroll the last rows clear of the player's frosted strip.
        ui.add_space(96.0);
    });
    selection_bar(app, ui.ctx(), &page, out.inner_rect);
    // Infinite scroll on home: fetch the next shelves when nearing the bottom.
    if page == Page::Home {
        let remaining =
            out.content_size.y - (out.state.clamped_offset().y + out.inner_rect.height());
        if let Some(h) = &mut app.home {
            // A failed fetch reports nothing (it's a quiet request): allow a retry later.
            if h.loading_more.is_some_and(|t| now - t > 20.0) {
                h.loading_more = None;
            }
            if remaining < 700.0
                && h.loading_more.is_none()
                && let Some(token) = h.continuation.clone()
            {
                h.loading_more = Some(now);
                app.backend.request(crate::backend::Req::HomeMore(token));
            }
        }
    }
}

/// What every song row needs from the app, gathered once per frame.
pub(super) struct Rows {
    current: Option<String>,
    playing: bool,
    accent: Color32,
    now: f64,
}

impl Rows {
    pub(super) fn new(app: &App, now: f64) -> Self {
        Self {
            current: app.current_id().map(str::to_owned),
            playing: app.audio.status.playing.load(Ordering::Relaxed),
            accent: app.accent_color(),
            now,
        }
    }

    pub(super) fn opts(
        &self,
        liked: &HashSet<String>,
        t: &Track,
        number: Option<usize>,
        show_album: bool,
    ) -> RowOpts {
        RowOpts {
            number,
            show_album,
            current: self.current.as_deref() == Some(t.id.as_str()),
            playing: self.playing,
            accent: self.accent,
            time: self.now,
            liked: liked.contains(&t.id),
            added_by: None,
            pick: false,
            selected: None,
            remove: None,
        }
    }
}

/// A page's data hasn't arrived: shimmering placeholders while it loads, a retry if the
/// request failed.
fn placeholder(app: &mut App, ui: &mut Ui, now: f64, rows: usize, card: f32) {
    if app.load_failed {
        ui.add_space(40.0);
        ui.label(RichText::new("Couldn't load this page").color(theme::text_dim()));
        ui.add_space(12.0);
        if pill(ui, "Try again", None, false).clicked() {
            app.refresh();
        }
    } else {
        widgets::skeleton(ui, now, rows, card);
    }
}

/// What a collection header's round buttons draw: a font glyph, or the three dots.
enum CircleGlyph {
    Char(char),
    More,
}

/// A round glass action on a collection header (shuffle, search, more).
fn circle_action(ui: &mut Ui, glyph: CircleGlyph, name: &str, active: bool) -> egui::Response {
    // Same 38 px height as the Play pill beside it, so the row shares one centre line.
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(38.0), Sense::click());
    let resp = named(resp, name)
        .on_hover_text(name)
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    let open = egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&resp));
    let h = theme::anim_bool(ui.ctx(), resp.id.with("h"), resp.hovered() || open, 0.15);
    let press = if resp.is_pointer_button_down_on() {
        18.3
    } else {
        19.0
    };
    ui.painter()
        .circle_filled(rect.center(), press, theme::glass(0.10 + 0.09 * h));
    let color = theme::lerp_color(theme::text_dim(), theme::text(), h.max(active as u8 as f32));
    match glyph {
        CircleGlyph::Char(g) => widgets::icon_at(ui, rect.center(), g, 16.0, color),
        CircleGlyph::More => {
            for i in -1..=1 {
                ui.painter()
                    .circle_filled(rect.center() + vec2(i as f32 * 5.2, 0.0), 1.8, color);
            }
        }
    }
    resp
}

/// A page's main Play button. Shows Pause while this page's album / playlist / artist
/// is playing, and toggles instead of restarting when it's already loaded.
fn play_pill(ui: &mut Ui, acts: &mut Vec<Action>, target: &Target, tracks: &[Track]) {
    let active = widgets::active_state(ui, target);
    let (label, glyph) = if active == Some(true) {
        ("Pause", icon::PAUSE)
    } else {
        ("Play", icon::PLAY)
    };
    if pill(ui, label, Some(glyph), true).clicked() {
        if active.is_some() {
            acts.push(Action::TogglePlay);
        } else if !tracks.is_empty() {
            acts.push(Action::Play(tracks.to_vec(), 0));
        }
    }
}

fn card_row(ui: &mut Ui, id: &str, title: &str, cards: &[Card], acts: &mut Vec<Action>) {
    if cards.is_empty() {
        return;
    }
    let (l, r) = shelf_header(ui, title, None, None, true);
    let nudge = r as i32 - l as i32;
    let tile = feed_tile(ui, true);
    h_scroll(ui, id, nudge, cards.len(), tile, 20.0, |ui| {
        ui.spacing_mut().item_spacing.x = 20.0;
        for c in cards {
            card(ui, c, tile, false, acts);
        }
    });
}

/// Songs laid out in columns of `rows`, scrolling sideways (Quick picks). Clicking a
/// song starts its radio. Returns whether the arrows reached the end.
#[allow(clippy::too_many_arguments)]
fn song_grid(
    ui: &mut Ui,
    rows_ctx: &Rows,
    liked: &HashSet<String>,
    acts: &mut Vec<Action>,
    id: &str,
    title: &str,
    strap: Option<&str>,
    tracks: &[Track],
    rows: usize,
) -> bool {
    let (l, r) = shelf_header(ui, title, strap, None, true);
    let nudge = r as i32 - l as i32;
    let col_w = feed_tile(ui, false);
    let mut picked = None;
    let (_, reached_end) = h_scroll(
        ui,
        id,
        nudge,
        tracks.len().div_ceil(rows.max(1)),
        col_w,
        20.0,
        |ui| {
            ui.spacing_mut().item_spacing.x = 20.0;
            for (c, col) in tracks.chunks(rows.max(1)).enumerate() {
                ui.allocate_ui(vec2(col_w, rows as f32 * 60.0), |ui| {
                    ui.set_width(col_w);
                    ui.vertical(|ui| {
                        for (i, t) in col.iter().enumerate() {
                            let o = rows_ctx.opts(liked, t, None, false);
                            if track_row(ui, t, &o, acts).clicked() {
                                picked = Some(c * rows + i);
                            }
                        }
                    });
                });
            }
        },
    );
    if let Some(i) = picked {
        acts.push(Action::Radio(tracks[i].clone()));
    }
    reached_end
}

/// Where a song list lives: the page its selection belongs to, and whether it's your own
/// playlist (so songs can be taken out of it).
struct ListCtx<'a> {
    sel: &'a mut Selection,
    page: &'a Page,
    own: bool,
}

/// A long song list; only the rows on screen are built. Clicking one plays `tracks` from it,
/// or picks it while selecting: Ctrl-click picks one song, Shift-click a run of them.
#[allow(clippy::too_many_arguments)]
fn list(
    ui: &mut Ui,
    rows: &Rows,
    liked: &HashSet<String>,
    tracks: &[Track],
    show_album: bool,
    number: impl Fn(usize, &Track) -> Option<usize>,
    cx: ListCtx,
    acts: &mut Vec<Action>,
) {
    let ListCtx { sel, page, own } = cx;
    let selecting = sel.on(page);
    let mods = ui.input(|i| i.modifiers);
    widgets::visible_rows(ui, tracks.len(), |ui, i| {
        let t = &tracks[i];
        let o = RowOpts {
            selected: selecting.then(|| sel.rows.contains(&i)),
            remove: own.then_some(i),
            ..rows.opts(liked, t, number(i, t), show_album)
        };
        if track_row(ui, t, &o, acts).clicked() {
            if selecting || mods.command || mods.shift {
                sel.pick(page, i, mods.shift);
            } else {
                acts.push(Action::Play(tracks.to_vec(), i));
            }
        }
    });
}

/// A list page's "Select" button: turns select mode on and off.
fn select_pill(ui: &mut Ui, sel: &mut Selection, page: &Page) {
    let on = sel.on(page);
    if pill(
        ui,
        if on { "Done" } else { "Select" },
        Some(icon::LIST_CHECKS),
        false,
    )
    .clicked()
    {
        if on { sel.clear() } else { sel.start(page) }
    }
}

/// The songs a page lists, and whether they're your own playlist's.
fn selectable<'a>(app: &'a App, page: &Page) -> Option<(&'a [Track], bool)> {
    match page {
        Page::Album(id) | Page::Playlist(id) => app
            .opened(id)
            .map(|o| &o.c)
            .map(|c| (c.tracks.as_slice(), app.logged_in && c.owned && !c.is_album)),
        Page::Library(LibTab::Songs) => app
            .opened(super::LIKED_MUSIC)
            .map(|o| (o.c.tracks.as_slice(), false)),
        Page::Library(LibTab::History) => app.history.as_deref().map(|h| (h, false)),
        Page::Library(LibTab::Uploads) => app.uploads.as_deref().map(|u| (u, false)),
        _ => None,
    }
}

/// While selecting: a bar floating over the bottom of the page with what to do with the picks.
fn selection_bar(app: &mut App, ctx: &egui::Context, page: &Page, area: Rect) {
    if !app.selection.on(page) {
        return;
    }
    let Some((tracks, own)) = selectable(app, page) else {
        return;
    };
    let n = tracks.len();
    let picked: Vec<Track> = app
        .selection
        .rows
        .iter()
        .filter_map(|&i| tracks.get(i).cloned())
        .collect();
    let all = !picked.is_empty() && picked.len() == n;
    let all_liked = !picked.is_empty() && picked.iter().all(|t| app.liked.contains(&t.id));
    // Labelled buttons when there's room, icons with tooltips when the window is narrow.
    let wide = area.width() > 820.0;
    let button = |ui: &mut Ui, label: &str, glyph: char, primary: bool| {
        if wide {
            pill(ui, label, Some(glyph), primary)
        } else {
            widgets::icon_button(ui, glyph, 18.0, label, false)
        }
    };
    let mut acts = Vec::new();
    let mut toggle_all = false;
    let mut done = ctx.input(|i| i.key_pressed(egui::Key::Escape));
    egui::Area::new(egui::Id::unique("selection-bar"))
        .order(egui::Order::Foreground)
        .pivot(Align2::CENTER_BOTTOM)
        .fixed_pos(pos2(area.center().x, area.bottom() - 18.0))
        .show(ctx, |ui| {
            egui::Frame::new()
                .fill(theme::menu_fill())
                .corner_radius(14)
                .inner_margin(egui::Margin {
                    left: 20,
                    right: 10,
                    top: 8,
                    bottom: 8,
                })
                .shadow(egui::epaint::Shadow {
                    offset: [0, 8],
                    blur: 24,
                    spread: 0,
                    color: theme::shade(0.45),
                })
                .show(ui, |ui| {
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 8.0;
                        let count = if picked.is_empty() {
                            "Select songs".to_owned()
                        } else {
                            format!("{} selected", picked.len())
                        };
                        ui.label(
                            RichText::new(count)
                                .font(theme::semibold(14.0))
                                .color(theme::text()),
                        );
                        ui.add_space(10.0);
                        if !picked.is_empty() {
                            if button(ui, "Add to playlist", icon::LIST_PLUS, true).clicked() {
                                acts.push(Action::AddToPlaylist(picked.clone()));
                            }
                            if own
                                && button(ui, "Remove from playlist", icon::LIST_MINUS, false)
                                    .clicked()
                            {
                                acts.push(Action::RemoveFromPlaylist(
                                    app.selection.rows.iter().copied().collect(),
                                ));
                            }
                            if button(
                                ui,
                                if all_liked { "Remove like" } else { "Like" },
                                icon::HEART,
                                false,
                            )
                            .clicked()
                            {
                                acts.push(Action::LikeMany(picked.clone(), !all_liked));
                            }
                            if button(ui, "Add to queue", icon::LIST_MUSIC, false).clicked() {
                                acts.extend(picked.iter().cloned().map(Action::Enqueue));
                            }
                        }
                        toggle_all = button(
                            ui,
                            if all { "Select none" } else { "Select all" },
                            icon::LIST_CHECKS,
                            false,
                        )
                        .clicked();
                        done |= widgets::icon_button(ui, icon::X, 18.0, "Done selecting", false)
                            .clicked();
                    });
                });
        });
    if toggle_all {
        let page = page.clone();
        app.selection.start(&page);
        if !all {
            app.selection.rows = (0..n).collect();
        }
    }
    if done || !acts.is_empty() {
        app.selection.clear();
    }
    app.actions.extend(acts);
}

// ------------------------------------------------------------------ home

fn home(app: &mut App, ui: &mut Ui, now: f64) {
    page_title(ui, "Home");
    personalized_recommendation(app, ui);
    // A refresh keeps showing the current feed; the skeleton is only for an empty one.
    if app
        .home
        .as_ref()
        .is_none_or(|h| app.home_loading && h.shelves.is_empty())
    {
        placeholder(app, ui, now, 3, CARD);
        return;
    }
    let rows = Rows::new(app, now);
    let Some(h) = &app.home else { return };
    if h.shelves.is_empty() {
        ui.add_space(40.0);
        ui.label(RichText::new("Nothing here yet").color(theme::text_dim()));
    }
    let mut more_picks = None;
    for (i, s) in h.shelves.iter().enumerate() {
        let id = format!("home-{i}-{}", s.title);
        match &s.kind {
            ShelfKind::Cards(cards) => {
                let (strap, thumb) = if s.title == "Listen again" {
                    (None, None)
                } else {
                    (s.strapline.as_deref(), s.strap_thumb.as_deref())
                };
                let (l, r) = shelf_header(ui, &s.title, strap, thumb, true);
                let nudge = r as i32 - l as i32;
                let tile = feed_tile(ui, true);
                let acts = &mut app.actions;
                let liked = &app.liked;
                h_scroll(ui, &id, nudge, cards.len(), tile, 20.0, |ui| {
                    ui.spacing_mut().item_spacing.x = 20.0;
                    for c in cards {
                        let is_liked =
                            matches!(&c.target, Target::Song(t) if liked.contains(&t.id));
                        card(ui, c, tile, is_liked, acts);
                    }
                });
            }
            ShelfKind::Songs { tracks, rows: n } => {
                let reached_end = song_grid(
                    ui,
                    &rows,
                    &app.liked,
                    &mut app.actions,
                    &id,
                    &s.title,
                    s.strapline.as_deref(),
                    tracks,
                    *n,
                );
                if s.title == "Quick picks" && reached_end && !h.picks_loading && !h.picks_expanded
                {
                    more_picks = tracks.last().map(|t| t.id.clone());
                }
            }
        }
    }
    if !h.chips.is_empty() {
        ui.add_space(16.0);
        section_label(ui, "Explore by mood", None);
        let mut picked = None;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
            for mood in &h.chips {
                if chip(ui, &mood.title, mood.selected).clicked() {
                    picked = Some(mood.params.clone());
                }
            }
        });
        if let Some(params) = picked {
            app.actions.push(Action::HomeChip(params));
        }
    }
    let loading_more = h.loading_more.is_some();
    if let Some(seed) = more_picks
        && let Some(h) = &mut app.home
    {
        h.picks_loading = true;
        app.backend
            .request(crate::backend::Req::MoreQuickPicks(seed));
    }
    if loading_more {
        widgets::skeleton(ui, now, 1, CARD);
    }
}

/// A listener with little account history starts with a radio from the last track they
/// played here. Dismissing a seed is stored locally so the next recent pick can lead.
fn personalized_recommendation(app: &mut App, ui: &mut Ui) {
    let seed = app
        .listening_stats
        .entries()
        .iter()
        .rev()
        .find(|entry| !app.discovery_feedback.contains(&entry.track.id))
        .map(|entry| entry.track.clone())
        .or_else(|| {
            app.current()
                .filter(|track| !app.discovery_feedback.contains(&track.id))
                .cloned()
        });
    let Some(track) = seed else { return };
    settings_card(ui, |ui| {
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(
                    RichText::new("Made from what you play")
                        .font(theme::semibold(15.0))
                        .color(theme::text()),
                );
                ui.label(
                    RichText::new(format!(
                        "Start a radio from {} • {}",
                        track.title,
                        track.artist_line()
                    ))
                    .font(theme::regular(12.5))
                    .color(theme::text_dim()),
                );
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if pill(ui, "Not for me", None, false).clicked() {
                    app.discovery_feedback.insert(track.id.clone());
                    app.toast("We’ll use another recent pick");
                }
                if pill(ui, "Play similar", Some(icon::RADIO), true).clicked() {
                    app.actions.push(Action::Radio(track.clone()));
                }
            });
        });
    });
    ui.add_space(12.0);
}

// ------------------------------------------------------------------ new

fn new(app: &mut App, ui: &mut Ui, now: f64) {
    page_title(ui, "New");
    if app.explore.is_none() {
        placeholder(app, ui, now, 2, CARD);
        return;
    }
    let rows = Rows::new(app, now);
    let Some(e) = &app.explore else { return };
    card_row(
        ui,
        "ex-new",
        if app.logged_in {
            "New releases for you"
        } else {
            "Popular new releases"
        },
        &e.new_albums,
        &mut app.actions,
    );
    if !e.top.is_empty() {
        song_grid(
            ui,
            &rows,
            &app.liked,
            &mut app.actions,
            "ex-top",
            "Top songs",
            None,
            &e.top,
            4,
        );
    }
    card_row(ui, "ex-charts", "Charts", &e.charts, &mut app.actions);
}

pub(super) fn page_title(ui: &mut Ui, title: &str) {
    // Left-aligned with the page's content, a little below the top edge.
    ui.add_space(18.0);
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 42.0), Sense::hover());
    ui.painter().text(
        pos2(rect.left(), rect.center().y),
        Align2::LEFT_CENTER,
        title,
        theme::bold(34.0),
        theme::text(),
    );
}

// ------------------------------------------------------------------ search

fn search(app: &mut App, ui: &mut Ui, q: &str, now: f64) {
    let pick = super::transfer::pick_banner(app, ui);
    page_title(ui, "Search");
    ui.add_space(10.0);
    // Coming from a suggestion, the sidebar or a link, the field says what's shown.
    // Only when the route changed: clearing the box leaves the route alone until Enter.
    if app.search_synced != q {
        app.search_synced = q.to_owned();
        app.search_text = q.trim().to_owned();
    }
    super::shell::search_field(app, ui, ui.available_width().min(560.0));
    if q.trim().is_empty() {
        super::discovery::render(app, ui);
        ui.add_space(26.0);
        recent_searches(ui, app);
        return;
    }
    if app.search.as_ref().is_none_or(|r| r.query != q) {
        ui.add_space(14.0);
        placeholder(app, ui, now, 2, CARD);
        return;
    }
    let rows = Rows::new(app, now);
    let Some(r) = &app.search else { return };
    let acts = &mut app.actions;
    let liked = &app.liked;
    if r.tracks.is_empty() && r.artists.is_empty() && r.albums.is_empty() && r.playlists.is_empty()
    {
        ui.add_space(18.0);
        ui.label(
            RichText::new("No results")
                .font(theme::bold(20.0))
                .color(theme::text()),
        );
        return;
    }
    ui.add_space(14.0);
    // Top result: the first artist when the query names one, otherwise the first song.
    ui.horizontal_top(|ui| {
        if let Some(top) = r
            .artists
            .first()
            .filter(|a| a.title.eq_ignore_ascii_case(q.trim()))
        {
            top_result(ui, top, false, acts);
        } else if let Some(t) = r.tracks.first() {
            let c = Card {
                title: t.title.clone(),
                subtitle: format!("Song • {}", t.artist_line()),
                thumbs: t.thumbs.clone(),
                target: Target::Song(t.clone()),
                round: false,
                links: t.artists.clone(),
            };
            top_result(ui, &c, pick, acts);
        }
        ui.add_space(24.0);
        ui.vertical(|ui| {
            ui.label(RichText::new("Songs").font(theme::bold(24.0)));
            ui.add_space(8.0);
            for (i, t) in r.tracks.iter().take(4).enumerate() {
                let o = RowOpts {
                    pick,
                    ..rows.opts(liked, t, None, false)
                };
                if track_row(ui, t, &o, acts).clicked() {
                    acts.push(if pick {
                        Action::TransferPick(t.clone())
                    } else {
                        Action::Play(r.tracks.clone(), i)
                    });
                }
            }
        });
    });
    card_row(ui, "s-artists", "Artists", &r.artists, acts);
    card_row(ui, "s-albums", "Albums", &r.albums, acts);
    if r.tracks.len() > 4 {
        ui.add_space(26.0);
        ui.label(RichText::new("More songs").font(theme::bold(24.0)));
        ui.add_space(10.0);
        for (i, t) in r.tracks.iter().enumerate().skip(4) {
            let o = RowOpts {
                pick,
                ..rows.opts(liked, t, None, true)
            };
            if track_row(ui, t, &o, acts).clicked() {
                acts.push(if pick {
                    Action::TransferPick(t.clone())
                } else {
                    Action::Play(r.tracks.clone(), i)
                });
            }
        }
    }
    card_row(ui, "s-pl", "Playlists", &r.playlists, acts);
}

fn podcasts(app: &mut App, ui: &mut Ui, now: f64) {
    page_title(ui, "Podcasts");
    ui.add_space(4.0);
    ui.label(
        RichText::new("Browse podcast shows and episodes from YouTube Music.")
            .font(theme::regular(13.0))
            .color(theme::text_dim()),
    );
    let Some(results) = app.search.as_ref().filter(|r| r.query == "podcasts") else {
        ui.add_space(16.0);
        placeholder(app, ui, now, 2, CARD);
        return;
    };
    let (tracks, playlists, albums) = (
        results.tracks.clone(),
        results.playlists.clone(),
        results.albums.clone(),
    );
    if tracks.is_empty() && playlists.is_empty() && albums.is_empty() {
        ui.add_space(20.0);
        ui.label(
            RichText::new("No podcast results yet.")
                .font(theme::bold(20.0))
                .color(theme::text()),
        );
        return;
    }
    let actions = &mut app.actions;
    card_row(ui, "podcast-shows", "Shows", &playlists, actions);
    card_row(ui, "podcast-series", "Series", &albums, actions);
    if !tracks.is_empty() {
        ui.add_space(22.0);
        ui.label(
            RichText::new("Episodes and tracks")
                .font(theme::bold(22.0))
                .color(theme::text()),
        );
        ui.add_space(10.0);
        let rows = Rows::new(app, now);
        list(
            ui,
            &rows,
            &app.liked,
            &tracks,
            true,
            |_, _| None,
            ListCtx {
                sel: &mut app.selection,
                page: &app.page,
                own: false,
            },
            &mut app.actions,
        );
    }
}

/// Apple's search landing: what was opened from the last few searches, with its artwork.
fn recent_searches(ui: &mut Ui, app: &mut App) {
    if app.recent_searches.is_empty() {
        ui.label(
            RichText::new("Search for artists, songs, albums and playlists.")
                .font(theme::regular(15.0))
                .color(theme::text_dim()),
        );
        return;
    }
    ui.label(
        RichText::new("Recently Searched")
            .font(theme::bold(22.0))
            .color(theme::text()),
    );
    ui.add_space(12.0);
    let w = ui.available_width().min(620.0);
    // The rows are cloned so a removal can be applied after the list is drawn.
    let recents = app.recent_searches.clone();
    let mut remove = None;
    for (i, r) in recents.iter().enumerate() {
        let (rect, resp) = ui.allocate_exact_size(vec2(w, 64.0), Sense::click());
        let resp = named(resp, &r.title).on_hover_cursor(egui::CursorIcon::PointingHand);
        let h = theme::anim_bool(ui.ctx(), resp.id.with("h"), resp.contains_pointer(), 0.12);
        ui.painter()
            .rect_filled(rect, 10.0, theme::glass(0.05 + 0.06 * h));
        let art = Rect::from_min_size(rect.left_top() + vec2(8.0, 8.0), Vec2::splat(48.0));
        let radius = if r.round { 24.0 } else { 6.0 };
        match r.thumb.as_deref().filter(|u| !u.is_empty()) {
            Some(url) => {
                let thumbs = [crate::model::Thumb {
                    url: url.to_owned(),
                    width: 300,
                    height: 300,
                }];
                paint_cover(ui, art, &thumbs, radius, 1.0);
            }
            None => {
                ui.painter().rect_filled(art, radius, theme::glass(0.1));
            }
        }
        let x = art.right() + 14.0;
        widgets::text_at(
            ui,
            pos2(x, rect.center().y - 17.0),
            &r.title,
            theme::semibold(15.0),
            theme::text(),
            rect.right() - x - 12.0,
        );
        widgets::text_at(
            ui,
            pos2(x, rect.center().y + 4.0),
            &r.subtitle,
            theme::regular(13.0),
            theme::text_dim(),
            rect.right() - x - 12.0,
        );
        egui::Popup::context_menu(&resp).show(|ui| {
            ui.set_min_width(200.0);
            if ui.button("Remove from Recently Searched").clicked() {
                remove = Some(i);
                ui.close();
            }
        });
        if resp.clicked() {
            app.actions.push(match &r.target {
                Some(t) => Action::Open(t.clone()),
                // An entry from an earlier version kept only the search.
                None => Action::Go(Page::Search(r.query.clone())),
            });
        }
        ui.add_space(6.0);
    }
    if let Some(i) = remove {
        app.recent_searches.remove(i);
    }
}

/// `pick`: choosing a transfer match, so clicking a song picks it.
fn top_result(ui: &mut Ui, c: &Card, pick: bool, acts: &mut Vec<Action>) {
    ui.vertical(|ui| {
        ui.label(RichText::new("Top result").font(theme::bold(24.0)));
        ui.add_space(8.0);
        let (rect, resp) = ui.allocate_exact_size(vec2(420.0, 236.0), Sense::click());
        let resp = named(resp, &c.title).on_hover_cursor(egui::CursorIcon::PointingHand);
        let h = theme::anim_bool(ui.ctx(), resp.id.with("h"), resp.contains_pointer(), 0.2);
        ui.painter()
            .rect_filled(rect, 14.0, theme::glass(0.07 + 0.05 * h));
        let img = Rect::from_min_size(rect.min + vec2(20.0, 20.0), Vec2::splat(110.0));
        paint_cover(
            ui,
            img,
            &c.thumbs,
            if c.round { 55.0 } else { theme::card_radius() },
            1.0,
        );
        let x = rect.left() + 20.0;
        widgets::text_at(
            ui,
            pos2(x, img.bottom() + 16.0),
            &c.title,
            theme::bold(26.0),
            theme::text(),
            rect.width() - 40.0,
        );
        widgets::linked_text(
            ui,
            pos2(x, img.bottom() + 52.0),
            &c.subtitle,
            &c.links,
            theme::regular(14.0),
            theme::text_dim(),
            rect.width() - 40.0,
            acts,
        );
        // Floating play button
        let pc = rect.right_bottom() + vec2(-44.0, -44.0 + 6.0 * (1.0 - h));
        let pr = Rect::from_center_size(pc, Vec2::splat(52.0));
        let active = widgets::active_state(ui, &c.target);
        let playing = active == Some(true);
        let label = format!("{} {}", if playing { "Pause" } else { "Play" }, c.title);
        let p = named(
            ui.interact(pr, resp.id.with("play"), Sense::click()),
            &label,
        )
        .on_hover_cursor(egui::CursorIcon::PointingHand);
        ui.painter()
            .circle_filled(pc, 26.0, theme::with_alpha(Color32::WHITE, 0.4 + 0.6 * h));
        let nudge = if playing { 0.0 } else { 1.5 };
        widgets::play_pause_glyph(
            ui,
            pc + vec2(nudge, 0.0),
            10.0,
            Color32::from_gray(15),
            playing,
        );
        if p.clicked() {
            acts.push(match &c.target {
                _ if active.is_some() => Action::TogglePlay,
                Target::Song(t) => Action::Radio(t.clone()),
                t => Action::PlayCollection(t.clone(), false),
            });
        } else if resp.clicked() {
            acts.push(match &c.target {
                Target::Song(t) if pick => Action::TransferPick(t.clone()),
                t => Action::Open(t.clone()),
            });
        }
    });
}

// ------------------------------------------------------------------ album / playlist

fn collection(app: &mut App, ui: &mut Ui, id: &str, now: f64) {
    if app.opened(id).is_none() {
        placeholder(app, ui, now, 1, 232.0);
        return;
    }
    let rows = Rows::new(app, now);
    // The header's motion sleeve, asked for before the release is borrowed below.
    let (release, artist) = match app.opened(id) {
        Some(o) => (
            o.c.title.clone(),
            o.c.artists
                .first()
                .map(|a| a.name.clone())
                .unwrap_or_else(|| o.c.byline()),
        ),
        None => (String::new(), String::new()),
    };
    let px = (232.0 * ui.ctx().pixels_per_point()).round().max(256.0) as u32;
    let motion_enabled = super::MOTION_COVERS_ON_OTHER_SURFACES;
    app.apple_cover(motion_enabled, &release, &artist, None);
    let playing = app.playing();
    let sprite = app.apple_sprite(motion_enabled, playing, ui.ctx(), &release, &artist, px);
    let Some(o) = app.lists.get(id) else { return };
    let (c, more) = (&o.c, app.receiving(id));
    let acts = &mut app.actions;
    let tracks = &c.tracks;
    let is_album = c.is_album;
    let filter_on = app.coll_search.as_ref().is_some_and(|(fid, _)| fid == id);
    let selecting = app.selection.on(&app.page);
    let mut toggle_search = false;
    let mut start_select = false;
    let mut stop_select = false;
    ui.add_space(18.0);
    ui.horizontal_top(|ui| {
        let (img, _) = ui.allocate_exact_size(Vec2::splat(232.0), Sense::hover());
        widgets::soft_shadow(ui, img, 10.0, 40.0, 0.5);
        paint_cover(ui, img, &c.thumbs, theme::card_radius() + 2.0, 1.0);
        if let Some(s) = &sprite {
            widgets::paint_motion(ui, img, theme::card_radius() + 2.0, s);
        }
        ui.add_space(26.0);
        ui.vertical(|ui| {
            ui.set_max_width(ui.available_width().min(760.0));
            ui.add_space(24.0);
            ui.label(
                RichText::new(if is_album { "ALBUM" } else { "PLAYLIST" })
                    .font(theme::semibold(12.0))
                    .color(theme::text_dim()),
            );
            ui.label(
                RichText::new(&c.title)
                    .font(theme::bold(40.0))
                    .color(theme::text()),
            );
            ui.add_space(2.0);
            let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 20.0), Sense::hover());
            widgets::linked_text(
                ui,
                r.min,
                &c.byline(),
                &c.artists,
                theme::regular(15.0),
                theme::text_dim(),
                r.width(),
                acts,
            );
            let total: u32 = tracks.iter().filter_map(|t| t.duration).sum();
            if total > 0 {
                ui.label(
                    RichText::new(format!("{} songs • {} min", tracks.len(), total / 60))
                        .font(theme::regular(13.5))
                        .color(theme::text_faint()),
                );
            }
            if let Some(d) = c.description.as_deref().filter(|d| !d.trim().is_empty()) {
                ui.add_space(6.0);
                let g = widgets::galley_wrapped(
                    ui,
                    d.trim(),
                    theme::regular(13.5),
                    theme::text_dim(),
                    ui.available_width(),
                    2,
                );
                ui.label(g);
            }
            ui.add_space(16.0);
            let target = if is_album {
                Target::Album(id.to_owned())
            } else {
                Target::Playlist(id.to_owned())
            };
            // Liked Music and Episodes for later are the account's own lists: they can't be
            // saved, removed, edited or deleted.
            let auto = matches!(id, "LM" | "SE");
            let saved = app.library.as_ref().is_some_and(|l| {
                let cards = if is_album { &l.albums } else { &l.playlists };
                cards.iter().any(|card| card.target == target)
            });
            // Saving only applies to someone else's playlist or an album.
            let save_id = if is_album {
                c.save_id.clone()
            } else {
                (!c.owned).then(|| id.to_owned())
            };
            let owned = !is_album && c.owned;
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing = vec2(10.0, 0.0);
                play_pill(ui, acts, &target, tracks);
                if circle_action(ui, CircleGlyph::Char(icon::SHUFFLE), "Shuffle", false).clicked()
                    && !tracks.is_empty()
                {
                    let mut t = tracks.clone();
                    super::shuffle(&mut t);
                    acts.push(Action::Play(t, 0));
                }
                if circle_action(
                    ui,
                    CircleGlyph::Char(icon::SEARCH),
                    "Search in this list",
                    filter_on,
                )
                .clicked()
                {
                    toggle_search = true;
                }
                let more = circle_action(ui, CircleGlyph::More, "More", false);
                egui::Popup::menu(&more).show(|ui| {
                    ui.set_width(226.0);
                    ui.add_space(3.0);
                    if let Some(first) = tracks.first()
                        && widgets::menu_action(ui, Some(icon::RADIO), "Start radio")
                    {
                        acts.push(Action::Radio(first.clone()));
                    }
                    if let Some(save_id) = save_id.clone()
                        && widgets::menu_action(
                            ui,
                            Some(icon::LIBRARY),
                            if saved {
                                "Remove from library"
                            } else {
                                "Save to library"
                            },
                        )
                    {
                        acts.push(Action::SaveCollection(save_id, !saved));
                    }
                    if app.logged_in && !auto && !tracks.is_empty() {
                        if widgets::menu_action(
                            ui,
                            Some(icon::LIST_CHECKS),
                            if selecting {
                                "Done selecting"
                            } else {
                                "Select"
                            },
                        ) {
                            if selecting {
                                stop_select = true
                            } else {
                                start_select = true
                            }
                        }
                    }
                    if app.logged_in && !auto && (save_id.is_some() || owned) {
                        ui.separator();
                    }
                    if owned {
                        if widgets::menu_action(ui, Some(icon::MUSIC), "Edit playlist") {
                            acts.push(Action::EditPlaylist);
                        }
                        if widgets::menu_action(ui, Some(icon::USERS), "Collaborate") {
                            acts.push(Action::Collaborate);
                        }
                        if widgets::menu_action(ui, Some(icon::TRASH_2), "Delete playlist") {
                            acts.push(Action::DeletePlaylist);
                        }
                    }
                    if app.logged_in && !auto && !owned && save_id.is_none() {
                        ui.separator();
                    }
                    if widgets::menu_action(ui, Some(icon::LINK), "Share") {
                        acts.push(Action::Share(target.clone()));
                    }
                    ui.add_space(3.0);
                });
            });
            if toggle_search {
                let closing = app.coll_search.as_ref().is_some_and(|(fid, _)| fid == id);
                app.coll_search = (!closing).then(|| (id.to_owned(), String::new()));
            }
            if start_select {
                app.selection.start(&app.page);
            }
            if stop_select {
                app.selection.clear();
            }
        });
    });
    let mut dismiss_search = false;
    if let Some((fid, query)) = &mut app.coll_search
        && fid == id
    {
        ui.add_space(14.0);
        let (rect, _) =
            ui.allocate_exact_size(vec2(ui.available_width().min(420.0), 40.0), Sense::hover());
        ui.painter().rect_filled(rect, 20.0, theme::glass(0.09));
        widgets::icon_at(
            ui,
            pos2(rect.left() + 20.0, rect.center().y),
            icon::SEARCH,
            16.0,
            theme::text_dim(),
        );
        let clearable = !query.is_empty();
        let inner = Rect::from_min_max(
            pos2(rect.left() + 40.0, rect.top() + 4.0),
            pos2(
                rect.right() - if clearable { 40.0 } else { 16.0 },
                rect.bottom() - 4.0,
            ),
        );
        let edit = ui.put(
            inner,
            egui::TextEdit::singleline(query)
                .hint_text(egui::RichText::new("Search in this list").color(theme::text_faint()))
                .font(theme::regular(14.0))
                .align(egui::Align2::CENTER_CENTER)
                .frame(egui::Frame::NONE)
                .desired_width(inner.width()),
        );
        if toggle_search {
            edit.request_focus();
        } else if !edit.has_focus() && query.is_empty() {
            // Left alone empty, the field goes away instead of sitting over the list.
            dismiss_search = true;
        }
        if clearable {
            let x = Rect::from_center_size(
                pos2(rect.right() - 20.0, rect.center().y),
                Vec2::splat(26.0),
            );
            let resp = named(
                ui.interact(x, ui.scope_id().with("coll-clear"), Sense::click()),
                "Clear search",
            )
            .on_hover_cursor(egui::CursorIcon::PointingHand);
            widgets::icon_at(ui, x.center(), icon::X, 14.0, theme::text_dim());
            if resp.clicked() {
                query.clear();
            }
        }
        ui.add_space(18.0);
    } else {
        ui.add_space(26.0);
    }
    if dismiss_search {
        app.coll_search = None;
    }
    let needle = app
        .coll_search
        .as_ref()
        .filter(|(fid, _)| fid == id)
        .map(|(_, q)| q.trim().to_lowercase())
        .unwrap_or_default();
    let filtered: Vec<Track> = if needle.is_empty() {
        Vec::new()
    } else {
        c.tracks
            .iter()
            .filter(|t| {
                t.title.to_lowercase().contains(&needle)
                    || t.artist_line().to_lowercase().contains(&needle)
                    || t.album
                        .as_ref()
                        .is_some_and(|a| a.name.to_lowercase().contains(&needle))
            })
            .cloned()
            .collect()
    };
    let shown: &[Track] = if needle.is_empty() { tracks } else { &filtered };
    let number = |i: usize, t: &Track| is_album.then(|| t.track_nr.map_or(i + 1, |n| n as usize));
    let own = app.logged_in && c.owned && !is_album;
    list(
        ui,
        &rows,
        &app.liked,
        shown,
        !is_album,
        number,
        ListCtx {
            sel: &mut app.selection,
            page: &app.page,
            own,
        },
        acts,
    );
    if more {
        widgets::skeleton_rows(ui, now, 3);
    }
}

// ------------------------------------------------------------------ artist

/// Apple's wordmark for the artist, bottom-left at `base`, no wider than `max_w`.
/// False until the image is ready, so the caller can draw the name instead.
fn wordmark(ui: &Ui, base: egui::Pos2, logo: &crate::apple::LogoSrc, max_w: f32) -> bool {
    let aspect = logo.w.max(1) as f32 / logo.h.max(1) as f32;
    let mut size = vec2(72.0 * aspect, 72.0);
    if size.x > max_w {
        size = vec2(max_w, max_w / aspect);
    }
    let url = logo.at_height((size.y * 2.0) as u32);
    let ready = matches!(
        egui::Image::new(url.as_str()).load_for_size(ui.ctx(), size),
        Ok(egui::load::TexturePoll::Ready { .. })
    );
    if ready {
        let rect = Rect::from_min_size(pos2(base.x, base.y - 40.0 - size.y), size);
        let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
        widgets::paint_remote(ui, &url, rect, 0.0, uv, Color32::WHITE);
    }
    ready
}

fn artist(app: &mut App, ui: &mut Ui, id: &str, now: f64) {
    if app.artist.as_ref().is_none_or(|a| a.id != id) {
        placeholder(app, ui, now, 2, CARD);
        return;
    }
    let rows = Rows::new(app, now);
    let accent = app.accent_color();
    // Apple's own page for this artist: their display name and the hero clip. Artists
    // Apple has no clip for (Charli xcx) keep the still banner.
    let apple = app.apple_artists.get(id).and_then(|o| o.clone());
    let name = apple.as_ref().map(|x| x.name.clone()).unwrap_or_else(|| {
        app.artist
            .as_ref()
            .map(|a| a.name.clone())
            .unwrap_or_default()
    });
    // Apple's search misses once now and then. While the page is open, ask again after
    // the cooldown rather than waiting for the listener to come back to it.
    if apple.is_none() {
        let key = format!("artist:{id}");
        let cooling = app
            .apple_missed
            .get(&key)
            .is_some_and(|t| t.elapsed() < std::time::Duration::from_secs(120));
        if !cooling && app.apple_waiting.insert(key) {
            app.backend.request(crate::backend::Req::AppleArtist {
                id: id.to_owned(),
                name: name.clone(),
            });
        }
    }
    // Apple's wordmark wins the hero; failing one, the listener's signature service;
    // failing that, the name in type. A lettered artist still has their signature
    // asked for, because it signs the About section below instead of the hero.
    let signature = app.signature_texture(ui.ctx(), &name);
    let Some(a) = &app.artist else { return };
    let acts = &mut app.actions;
    let top = &a.top;
    // Hero banner fading into the page.
    let w = ui.available_width();
    let hero_h = (w * 0.34).clamp(240.0, 380.0);
    let (hero, _) = ui.allocate_exact_size(vec2(w, hero_h), Sense::hover());
    if let Some(t) = a.banner.iter().max_by_key(|t| t.width) {
        let img_ratio = if t.height > 0 {
            t.width as f32 / t.height as f32
        } else {
            16.0 / 9.0
        };
        // Cover-fit: crop the image's top band to the hero's aspect.
        let vis_h = (img_ratio / (w / hero_h)).min(1.0);
        let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, vis_h));
        ui.painter().rect_filled(hero, 14.0, theme::glass(0.07));
        widgets::paint_remote(ui, &t.url, hero, 14.0, uv, Color32::WHITE);
    } else {
        ui.painter()
            .rect_filled(hero, 14.0, theme::with_alpha(accent, 0.25));
    }
    super::shell::vgradient(
        ui.painter(),
        Rect::from_min_max(pos2(hero.left(), hero.center().y - 40.0), hero.max),
        Color32::TRANSPARENT,
        theme::shade(0.85),
    );
    let base = hero.left_bottom() + vec2(28.0, -30.0);
    // Apple letters some artists a wordmark image instead of setting their name in type.
    // Until it downloads (or when there's none) the typed name stands in.
    let drawn = apple
        .as_ref()
        .and_then(|x| x.logo.as_ref())
        .is_some_and(|logo| wordmark(ui, base, logo, w * 0.62));
    if !drawn {
        match &signature {
            // The artist's own hand, credited above it the way a page signs itself off.
            Some(tex) => {
                let tex_size = tex.size_vec2();
                let aspect = tex_size.x / tex_size.y.max(1.0);
                let mut size = vec2(96.0 * aspect, 96.0);
                let max_w = (w * 0.42).min(340.0);
                if size.x > max_w {
                    size = vec2(max_w, max_w / aspect);
                }
                let rect = Rect::from_min_size(pos2(base.x, base.y - 40.0 - size.y), size);
                ui.painter().text(
                    pos2(base.x, rect.top() - 8.0),
                    Align2::LEFT_BOTTOM,
                    format!("Signed by {name}"),
                    theme::regular(12.0),
                    Color32::from_white_alpha(199),
                );
                let tex = egui::load::SizedTexture::from_handle(tex);
                egui::Image::from_texture(tex)
                    .uv(Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)))
                    .paint_at(ui, rect);
            }
            None => {
                ui.painter().text(
                    base - vec2(0.0, 44.0),
                    Align2::LEFT_BOTTOM,
                    &name,
                    theme::bold(58.0),
                    Color32::WHITE,
                );
            }
        }
    }
    if let Some(s) = a.subscribers {
        ui.painter().text(
            base - vec2(0.0, 18.0),
            Align2::LEFT_BOTTOM,
            format!("{} subscribers", compact(s)),
            theme::regular(14.0),
            theme::text_dim(),
        );
    }
    ui.add_space(18.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        play_pill(ui, acts, &Target::Artist(id.to_owned()), top);
        if pill(ui, "Shuffle", Some(icon::SHUFFLE), false).clicked() && !top.is_empty() {
            let mut t = top.clone();
            super::shuffle(&mut t);
            acts.push(Action::Play(t, 0));
        }
        if let Some(first) = top.first()
            && pill(ui, "Radio", Some(icon::RADIO), false).clicked()
        {
            acts.push(Action::Radio(first.clone()));
        }
    });
    if !top.is_empty() {
        ui.add_space(26.0);
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("Top songs")
                    .font(theme::bold(24.0))
                    .color(theme::text()),
            );
            if let Some(pl) = &a.top_playlist {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if pill(ui, "Show all", None, false).clicked() {
                        acts.push(Action::Open(Target::Playlist(pl.clone())));
                    }
                });
            }
        });
        ui.add_space(12.0);
        for (i, t) in top.iter().enumerate().take(5) {
            let o = rows.opts(&app.liked, t, None, true);
            if track_row(ui, t, &o, acts).clicked() {
                acts.push(Action::Play(top.clone(), i));
            }
        }
    }
    card_row(ui, "a-albums", "Albums & singles", &a.albums, acts);
    card_row(ui, "a-pl", "Featured on", &a.playlists, acts);
    card_row(ui, "a-sim", "Fans might also like", &a.similar, acts);
    if let Some(d) = a.description.as_deref().filter(|d| !d.trim().is_empty()) {
        shelf_header(ui, "About", None, None, false);
        let full = ui.available_width().min(820.0);
        // A lettered artist's signature can't be the hero, so it signs the About
        // section beside the prose the way a page signs itself off.
        let beside = drawn.then_some(signature.as_ref()).flatten();
        if let Some(tex) = beside {
            ui.horizontal_top(|ui| {
                let sig_w = (full * 0.28).clamp(120.0, 210.0);
                let text_w = (full - sig_w - 32.0).max(220.0);
                let g = widgets::galley_wrapped(
                    ui,
                    d.trim(),
                    theme::regular(14.5),
                    theme::text_dim(),
                    text_w,
                    8,
                );
                ui.label(g);
                ui.add_space(32.0);
                let tex_size = tex.size_vec2();
                let aspect = tex_size.x / tex_size.y.max(1.0);
                let mut size = vec2(sig_w, sig_w / aspect);
                if size.y > 140.0 {
                    size = vec2(140.0 * aspect, 140.0);
                }
                let (rect, _) = ui.allocate_exact_size(vec2(size.x, size.y + 22.0), Sense::hover());
                ui.painter().text(
                    rect.left_top(),
                    Align2::LEFT_TOP,
                    format!("Signed by {name}"),
                    theme::regular(12.0),
                    Color32::from_white_alpha(199),
                );
                let img = Rect::from_min_size(pos2(rect.left(), rect.top() + 22.0), size);
                let tex = egui::load::SizedTexture::from_handle(tex);
                egui::Image::from_texture(tex)
                    .uv(Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)))
                    .paint_at(ui, img);
            });
        } else {
            let g = widgets::galley_wrapped(
                ui,
                d.trim(),
                theme::regular(14.5),
                theme::text_dim(),
                full,
                8,
            );
            ui.label(g);
        }
    }
}

// ------------------------------------------------------------------ library

fn library(app: &mut App, ui: &mut Ui, tab: LibTab, now: f64) {
    page_title(ui, "Library");
    if !app.logged_in {
        ui.add_space(10.0);
        ui.label(
            RichText::new("Sign in to see your playlists, liked songs and history.")
                .color(theme::text_dim()),
        );
        ui.add_space(12.0);
        if pill(ui, "Sign in with Google", Some(icon::CIRCLE_USER), true).clicked() {
            app.start_sign_in(ui.ctx());
        }
        return;
    }
    ui.add_space(12.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        for (t, label) in [
            (LibTab::Playlists, "Playlists"),
            (LibTab::Songs, "Liked songs"),
            (LibTab::Albums, "Albums"),
            (LibTab::History, "History"),
            (LibTab::Uploads, "Uploads"),
        ] {
            if chip(ui, label, tab == t).clicked() {
                // Tabs swap in place (no history entry); the current one refreshes.
                if tab == t {
                    app.refresh();
                } else {
                    app.page = Page::Library(t);
                    app.page_loaded();
                }
            }
        }
    });
    ui.add_space(12.0);
    let grid = |ui: &mut Ui, cards: &[Card], acts: &mut Vec<Action>| {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(20.0, 22.0);
            for c in cards {
                card(ui, c, CARD, false, acts);
            }
        });
    };
    let rows = Rows::new(app, now);
    let plain = |_: usize, _: &Track| None;
    match tab {
        LibTab::History => {
            if app.history.is_none() {
                placeholder(app, ui, now, 1, 48.0);
                return;
            }
            let Some(h) = &app.history else { return };
            list(
                ui,
                &rows,
                &app.liked,
                h,
                true,
                plain,
                ListCtx {
                    sel: &mut app.selection,
                    page: &app.page,
                    own: false,
                },
                &mut app.actions,
            );
        }
        LibTab::Uploads => {
            ui.horizontal(|ui| {
                if pill(ui, "Upload music", Some(icon::UPLOAD), true).clicked() {
                    app.pick_uploads(ui.ctx());
                }
                if app.uploads.as_ref().is_some_and(|u| !u.is_empty())
                    && pill(ui, "Shuffle", Some(icon::SHUFFLE), false).clicked()
                {
                    let mut t = app.uploads.clone().unwrap_or_default();
                    super::shuffle(&mut t);
                    app.actions.push(Action::Play(t, 0));
                }
            });
            ui.add_space(6.0);
            ui.label(
                RichText::new(
                    "Or drop audio files anywhere on the window. MP3, M4A, FLAC, OGG and WMA.",
                )
                .font(theme::regular(13.0))
                .color(theme::text_dim()),
            );
            ui.add_space(12.0);
            if app.uploads.is_none() {
                placeholder(app, ui, now, 1, 48.0);
                return;
            }
            let Some(up) = &app.uploads else { return };
            if up.is_empty() {
                ui.add_space(20.0);
                ui.label(RichText::new("No uploads yet").color(theme::text_dim()));
            }
            list(
                ui,
                &rows,
                &app.liked,
                up,
                true,
                plain,
                ListCtx {
                    sel: &mut app.selection,
                    page: &app.page,
                    own: false,
                },
                &mut app.actions,
            );
        }
        // Liked Music, kept like any playlist.
        LibTab::Songs => {
            if app.opened(super::LIKED_MUSIC).is_none() {
                placeholder(app, ui, now, 1, 48.0);
                return;
            }
            let more = app.receiving(super::LIKED_MUSIC);
            let Some(o) = app.lists.get(super::LIKED_MUSIC) else {
                return;
            };
            let liked = &o.c.tracks;
            if !liked.is_empty() {
                ui.horizontal(|ui| {
                    if pill(ui, "Play", Some(icon::PLAY), true).clicked() {
                        app.actions.push(Action::Play(liked.clone(), 0));
                    }
                    if pill(ui, "Shuffle", Some(icon::SHUFFLE), false).clicked() {
                        let mut t = liked.clone();
                        super::shuffle(&mut t);
                        app.actions.push(Action::Play(t, 0));
                    }
                    select_pill(ui, &mut app.selection, &app.page);
                });
                ui.add_space(10.0);
            }
            list(
                ui,
                &rows,
                &app.liked,
                liked,
                true,
                plain,
                ListCtx {
                    sel: &mut app.selection,
                    page: &app.page,
                    own: false,
                },
                &mut app.actions,
            );
            if more {
                widgets::skeleton_rows(ui, now, 3);
            }
        }
        _ => {
            if app.library.is_none() {
                placeholder(app, ui, now, 2, CARD);
                return;
            }
            let Some(lib) = &app.library else { return };
            match tab {
                LibTab::Playlists => {
                    if pill(ui, "New playlist", None, false).clicked() {
                        app.actions.push(Action::NewPlaylist(Vec::new()));
                    }
                    ui.add_space(14.0);
                    grid(ui, &lib.playlists, &mut app.actions)
                }
                LibTab::Albums => grid(ui, &lib.albums, &mut app.actions),
                LibTab::Songs | LibTab::History | LibTab::Uploads => unreachable!(),
            }
        }
    }
}

// ------------------------------------------------------------------ downloads

fn local_music(app: &mut App, ui: &mut Ui) {
    page_title(ui, "Local Music");
    let mut choose = false;
    let mut rescan = false;
    ui.horizontal(|ui| {
        choose = pill(ui, "Choose folder", Some(icon::FOLDER_OPEN), true).clicked();
        rescan = pill(ui, "Rescan", None, false).clicked();
        if !app.local_tracks.is_empty() && pill(ui, "Play all", Some(icon::PLAY), false).clicked() {
            app.actions.push(Action::Play(app.local_tracks.clone(), 0));
        }
    });
    if choose {
        if let Some(folder) = rfd::FileDialog::new()
            .set_title("Choose your music folder")
            .pick_folder()
        {
            app.local_folder = Some(folder);
            rescan = true;
        }
    }
    if rescan {
        if let Some(folder) = app.local_folder.clone() {
            app.local_art_missed.clear();
            app.local_tracks = crate::local_music::scan(&folder, app.backend.data_dir());
            app.toast(format!("Found {} local tracks", app.local_tracks.len()));
        }
    }
    ui.add_space(8.0);
    ui.label(
        RichText::new(app.local_folder.as_ref().map_or_else(
            || "Choose a folder to scan for MP3 and FLAC files".to_owned(),
            |p| p.display().to_string(),
        ))
        .font(theme::regular(12.5))
        .color(theme::text_dim()),
    );
    ui.add_space(12.0);
    if app.local_folder.is_none() {
        ui.label(RichText::new("Your selected folder and its subfolders are scanned again each time Marshall starts.").font(theme::regular(14.0)).color(theme::text_dim()));
        return;
    }
    if app.local_tracks.is_empty() {
        ui.label(
            RichText::new("No MP3 or FLAC files found")
                .font(theme::semibold(16.0))
                .color(theme::text()),
        );
        return;
    }
    let tracks = app.local_tracks.clone();
    let current = app.current_id().map(str::to_owned);
    let playing = app.playing();
    let mut play = None;
    for (i, track) in tracks.iter().enumerate() {
        let (rect, response) =
            ui.allocate_exact_size(vec2(ui.available_width(), 64.0), Sense::click());
        if ui.is_rect_visible(rect) {
            request_local_artwork(app, track);
        }
        let response =
            named(response, &track.title).on_hover_cursor(egui::CursorIcon::PointingHand);
        ui.painter().rect_filled(
            rect,
            8.0,
            theme::glass(if response.hovered() { 0.11 } else { 0.045 }),
        );
        let cover = Rect::from_min_size(rect.left_top() + vec2(8.0, 8.0), Vec2::splat(48.0));
        paint_cover(ui, cover, &track.thumbs, 6.0, 1.0);
        let x = cover.right() + 14.0;
        widgets::text_at(
            ui,
            pos2(x, rect.center().y - 9.0),
            &track.title,
            theme::semibold(14.0),
            theme::text(),
            rect.right() - x - 64.0,
        );
        let detail = [
            track.artist_line(),
            track
                .album
                .as_ref()
                .map(|a| a.name.clone())
                .unwrap_or_default(),
        ]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" • ");
        widgets::text_at(
            ui,
            pos2(x, rect.center().y + 12.0),
            &detail,
            theme::regular(12.0),
            theme::text_dim(),
            rect.right() - x - 64.0,
        );
        let selected = current.as_deref() == Some(track.id.as_str());
        let center = pos2(rect.right() - 26.0, rect.center().y);
        ui.painter().circle_filled(
            center,
            18.0,
            theme::with_alpha(Color32::WHITE, if response.hovered() { 0.85 } else { 0.55 }),
        );
        widgets::play_pause_glyph(ui, center, 7.0, Color32::from_gray(18), selected && playing);
        if response.clicked() {
            play = Some(i);
        }
    }
    if let Some(i) = play {
        app.actions.push(Action::Play(tracks, i));
    }
}

fn request_local_artwork(app: &mut App, track: &Track) {
    if !crate::local_music::needs_better_art(track) {
        return;
    }
    let Some(key) = crate::local_music::artwork_key(track) else {
        return;
    };
    if app.local_art_waiting.contains(&key)
        || app
            .local_art_missed
            .get(&key)
            .is_some_and(|at| at.elapsed() < std::time::Duration::from_secs(30 * 60))
    {
        return;
    }
    app.local_art_missed.remove(&key);
    app.local_art_waiting.insert(key.clone());
    app.backend.request(crate::backend::Req::LocalArtwork {
        key,
        title: track.title.clone(),
        artist: track.artist_line(),
        album: track.album.as_ref().map(|a| a.name.clone()),
    });
}

fn statistics(app: &mut App, ui: &mut Ui) {
    page_title(ui, "Statistics");
    if !app.logged_in {
        ui.add_space(8.0);
        ui.label(
            RichText::new("Sign in to see your recent listening history.")
                .font(theme::regular(14.0))
                .color(theme::text_dim()),
        );
        ui.add_space(12.0);
        if pill(ui, "Sign in", None, true).clicked() {
            app.start_sign_in(ui.ctx());
        }
        return;
    }
    let Some(history) = app.history.as_deref() else {
        ui.add_space(18.0);
        if app.loading {
            ui.spinner();
            ui.label(
                RichText::new("Loading listening history…")
                    .font(theme::regular(13.0))
                    .color(theme::text_dim()),
            );
        } else if app.load_failed {
            ui.label(
                RichText::new("Couldn't load listening history.")
                    .font(theme::regular(13.0))
                    .color(theme::text_dim()),
            );
            ui.add_space(10.0);
            if pill(ui, "Try again", None, false).clicked() {
                app.loading = true;
                app.load_failed = false;
                app.backend.request(crate::backend::Req::History);
            }
        }
        return;
    };
    let stats = history_stats(history);
    ui.add_space(4.0);
    ui.label(
        RichText::new(format!(
            "Based on {} recent entries in your YouTube Music history.",
            stats.entries
        ))
        .font(theme::regular(13.0))
        .color(theme::text_dim()),
    );
    ui.add_space(12.0);
    settings_card(ui, |ui| {
        let width = ((ui.available_width() - 24.0) / 3.0).max(100.0);
        ui.horizontal(|ui| {
            stat_metric(ui, width, &stats.entries.to_string(), "History entries");
            stat_metric(ui, width, &stats.artists.to_string(), "Artists");
            stat_metric(
                ui,
                width,
                &history_runtime(stats.runtime_seconds),
                "Runtime represented",
            );
        });
    });
    ui.add_space(20.0);
    section_label(ui, "Most repeated in history", None);
    if stats.repeat_tracks.is_empty() {
        ui.label(
            RichText::new("No listening history yet.")
                .font(theme::regular(14.0))
                .color(theme::text_dim()),
        );
        return;
    }
    let mut play = None;
    settings_card(ui, |ui| {
        for (track, count) in stats.repeat_tracks.iter().take(8) {
            let (rect, resp) =
                ui.allocate_exact_size(vec2(ui.available_width(), 62.0), Sense::click());
            let hover = theme::anim_bool(
                ui.ctx(),
                resp.id.with("history-stat-hover"),
                resp.hovered(),
                0.14,
            );
            if hover > 0.0 {
                ui.painter()
                    .rect_filled(rect, 8.0, theme::glass(0.08 * hover));
            }
            let cover = Rect::from_min_size(rect.min + vec2(7.0, 7.0), Vec2::splat(48.0));
            paint_cover(ui, cover, &track.thumbs, theme::thumb_radius(), 1.0);
            let title = if track.title.is_empty() {
                "Unknown title"
            } else {
                &track.title
            };
            widgets::text_at(
                ui,
                pos2(cover.right() + 12.0, rect.top() + 13.0),
                title,
                theme::semibold(14.0),
                theme::text(),
                (rect.width() - 170.0).max(80.0),
            );
            widgets::text_at(
                ui,
                pos2(cover.right() + 12.0, rect.top() + 36.0),
                &track.artist_line(),
                theme::regular(12.5),
                theme::text_dim(),
                (rect.width() - 170.0).max(80.0),
            );
            let count_label = if *count == 1 {
                "1 entry".to_owned()
            } else {
                format!("{count} entries")
            };
            ui.painter().text(
                pos2(rect.right() - 12.0, rect.center().y),
                Align2::RIGHT_CENTER,
                count_label,
                theme::regular(12.5),
                theme::text_dim(),
            );
            if resp.clicked() {
                play = Some(track.clone());
            }
        }
    });
    if let Some(track) = play {
        app.actions.push(Action::Play(vec![track], 0));
    }
}

#[derive(Default)]
struct HistoryStats {
    entries: usize,
    artists: usize,
    runtime_seconds: u64,
    repeat_tracks: Vec<(Track, usize)>,
}

fn history_stats(history: &[Track]) -> HistoryStats {
    let mut artists = HashSet::new();
    let mut counts: std::collections::HashMap<String, (Track, usize)> =
        std::collections::HashMap::new();
    let mut runtime_seconds = 0u64;
    for track in history {
        runtime_seconds = runtime_seconds.saturating_add(track.duration.unwrap_or(0) as u64);
        for artist in &track.artists {
            let key = artist.name.trim().to_lowercase();
            if !key.is_empty() {
                artists.insert(key);
            }
        }
        let count = counts
            .entry(track.id.clone())
            .or_insert_with(|| (track.clone(), 0));
        count.1 += 1;
    }
    let mut repeat_tracks: Vec<_> = counts.into_values().collect();
    repeat_tracks.sort_by(|a, b| {
        b.1.cmp(&a.1)
            .then_with(|| a.0.title.to_lowercase().cmp(&b.0.title.to_lowercase()))
    });
    HistoryStats {
        entries: history.len(),
        artists: artists.len(),
        runtime_seconds,
        repeat_tracks,
    }
}

fn stat_metric(ui: &mut Ui, width: f32, value: &str, label: &str) {
    ui.allocate_ui_with_layout(
        vec2(width, 70.0),
        egui::Layout::top_down(egui::Align::Center),
        |ui| {
            ui.add_space(5.0);
            ui.label(
                RichText::new(value)
                    .font(theme::bold(22.0))
                    .color(theme::text()),
            );
            ui.label(
                RichText::new(label)
                    .font(theme::regular(12.0))
                    .color(theme::text_dim()),
            );
        },
    );
}

fn history_runtime(seconds: u64) -> String {
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    if hours > 0 {
        format!("{hours} h {minutes} m")
    } else {
        format!("{minutes} min")
    }
}

#[cfg(test)]
mod history_stats_tests {
    use super::{history_runtime, history_stats, smart_playlists};
    use crate::listening_stats::Entry;
    use crate::model::{Link, Track};

    fn track(id: &str, title: &str, artist: &str, duration: Option<u32>) -> Track {
        Track {
            id: id.into(),
            title: title.into(),
            artists: vec![Link {
                name: artist.into(),
                id: None,
            }],
            album: None,
            duration,
            thumbs: Vec::new(),
            track_nr: None,
            plays: None,
            music_video: false,
        }
    }

    #[test]
    fn summary_counts_repeated_tracks_and_unique_artists() {
        let history = vec![
            track("a", "First", "Band", Some(180)),
            track("b", "Second", "band", Some(120)),
            track("a", "First", "Band", Some(180)),
        ];
        let summary = history_stats(&history);
        assert_eq!(summary.entries, 3);
        assert_eq!(summary.artists, 1);
        assert_eq!(summary.runtime_seconds, 480);
        assert_eq!(summary.repeat_tracks[0].0.id, "a");
        assert_eq!(summary.repeat_tracks[0].1, 2);
    }

    #[test]
    fn smart_playlists_are_derived_from_local_history() {
        let now = crate::listening_stats::unix_now();
        let a = track("a", "A", "Artist", Some(180));
        let b = track("b", "B", "Artist", Some(180));
        let entries = vec![
            Entry {
                started_at: now - 2 * 86_400,
                track: a.clone(),
                listened_seconds: 100.0,
            },
            Entry {
                started_at: now - 600,
                track: b.clone(),
                listened_seconds: 100.0,
            },
            Entry {
                started_at: now - 300,
                track: a.clone(),
                listened_seconds: 100.0,
            },
            Entry {
                started_at: now - 20,
                track: a.clone(),
                listened_seconds: 20.0,
            },
        ];
        let lists = smart_playlists(&entries);
        assert_eq!(
            lists[0].2.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(),
            ["a", "b"]
        );
        assert_eq!(
            lists[1].2.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(),
            ["a", "b"]
        );
        assert_eq!(
            lists[2].2.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(),
            ["a", "b"]
        );
    }

    #[test]
    fn runtime_uses_hours_only_when_present() {
        assert_eq!(history_runtime(1800), "30 min");
        assert_eq!(history_runtime(3720), "1 h 2 m");
    }
}

fn mix_lab(app: &mut App, ui: &mut Ui) {
    page_title(ui, "Mix Lab");
    ui.add_space(4.0);
    ui.label(RichText::new("Shape transitions with Marshall's playback engine. These controls also stay available in Settings.").font(theme::regular(13.0)).color(theme::text_dim()));
    ui.add_space(14.0);
    section_label(ui, "Transitions", None);
    settings_card(ui, |ui| {
        setting_row(
            ui,
            "AutoMix",
            Some(
                "Blend tracks across musical phrases. The player shows when a transition is in progress.",
            ),
            |ui| {
                if toggle(ui, &mut app.automix, "AutoMix").changed() {
                    app.set_crossfade();
                }
            },
        );
        if !app.automix {
            divider(ui);
            setting_row(
                ui,
                "Crossfade songs",
                Some("Blend the end of one track into the start of the next."),
                |ui| {
                    if toggle(ui, &mut app.crossfade, "Crossfade songs").changed() {
                        app.set_crossfade();
                    }
                },
            );
            if app.crossfade {
                divider(ui);
                setting_row(ui, "Crossfade length", None, |ui| {
                    if seconds_slider(
                        ui,
                        &mut app.crossfade_secs,
                        1..=super::CROSSFADE_MAX,
                        "Crossfade length",
                    )
                    .changed()
                    {
                        app.set_crossfade();
                    }
                });
            }
        }
        divider(ui);
        setting_row(
            ui,
            "Volume normalization",
            Some("Keep loud tracks from jumping out during a mix."),
            |ui| {
                if toggle(ui, &mut app.normalize, "Volume normalization").changed() {
                    app.audio.set_normalize(app.normalize);
                }
            },
        );
    });
    if app.mixing() {
        ui.add_space(16.0);
        ui.label(
            RichText::new("A transition is active. The current player controls remain in charge.")
                .font(theme::regular(13.0))
                .color(theme::text_dim()),
        );
    }
}

fn downloads(app: &mut App, ui: &mut Ui, now: f64) {
    page_title(ui, "Downloads");
    let songs = crate::downloads::list();
    ui.add_space(4.0);
    ui.label(
        RichText::new(format!(
            "{} songs \u{2022} {}",
            songs.len(),
            crate::downloads::folder().display()
        ))
        .font(theme::regular(13.0))
        .color(theme::text_dim()),
    );
    ui.add_space(12.0);
    if songs.is_empty() {
        ui.label(
            RichText::new("No downloads yet")
                .font(theme::semibold(16.0))
                .color(theme::text()),
        );
        ui.add_space(4.0);
        ui.label(
            RichText::new("Right-click a song and choose Download to keep it on this computer, playable offline.")
                .font(theme::regular(13.0))
                .color(theme::text_dim()),
        );
        ui.add_space(10.0);
        if pill(ui, "Open folder", Some(icon::FOLDER_OPEN), false).clicked() {
            crate::downloads::open_folder();
        }
        return;
    }
    ui.horizontal(|ui| {
        if pill(ui, "Play", Some(icon::PLAY), true).clicked() {
            app.actions.push(Action::Play(songs.clone(), 0));
        }
        if pill(ui, "Shuffle", Some(icon::SHUFFLE), false).clicked() {
            let mut t = songs.clone();
            super::shuffle(&mut t);
            app.actions.push(Action::Play(t, 0));
        }
        if pill(ui, "Open folder", Some(icon::FOLDER_OPEN), false).clicked() {
            crate::downloads::open_folder();
        }
    });
    ui.add_space(12.0);
    let rows = Rows::new(app, now);
    list(
        ui,
        &rows,
        &app.liked,
        &songs,
        true,
        |_, _| None,
        ListCtx {
            sel: &mut app.selection,
            page: &app.page,
            own: false,
        },
        &mut app.actions,
    );
}

// ------------------------------------------------------------------ settings

fn settings(app: &mut App, ui: &mut Ui) {
    const SECTIONS: &[&str] = &[
        "General",
        "Playback",
        "Audio",
        "Downloads",
        "Library",
        "Lyrics",
        "Appearance",
        "Last.fm",
        "Integrations",
        "Sources",
        "Experimental",
        "About",
    ];
    let width = ui.available_width();
    let sidebar_width = if width < 760.0 { 138.0 } else { 190.0 };
    ui.horizontal_top(|ui| {
        ui.allocate_ui_with_layout(
            vec2(sidebar_width, ui.available_height().max(700.0)),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                ui.add_space(14.0);
                ui.label(
                    RichText::new("SYSTEM")
                        .font(theme::semibold(11.0))
                        .color(theme::text_dim()),
                );
                ui.label(
                    RichText::new("Settings")
                        .font(theme::bold(26.0))
                        .color(theme::text()),
                );
                ui.add_space(16.0);
                for (i, label) in SECTIONS.iter().enumerate() {
                    let (rect, response) =
                        ui.allocate_exact_size(vec2(sidebar_width, 40.0), Sense::click());
                    let response = named(response, label);
                    let selected = app.settings_section as usize == i;
                    let hover = theme::anim_bool(
                        ui.ctx(),
                        response.id.with("settings-section-hover"),
                        response.hovered(),
                        0.12,
                    );
                    if selected || hover > 0.0 {
                        ui.painter().rect_filled(
                            rect,
                            9.0,
                            theme::glass(if selected { 0.12 } else { 0.06 * hover }),
                        );
                    }
                    if selected {
                        ui.painter().rect_filled(
                            Rect::from_min_size(
                                rect.min + vec2(2.0, 9.0),
                                vec2(3.0, rect.height() - 18.0),
                            ),
                            2.0,
                            app.look().accent(app.accent),
                        );
                    }
                    ui.painter().text(
                        pos2(rect.left() + 18.0, rect.center().y),
                        Align2::LEFT_CENTER,
                        *label,
                        theme::regular(14.0),
                        if selected {
                            theme::text()
                        } else {
                            theme::text_dim()
                        },
                    );
                    if response.clicked() {
                        app.settings_section = i as u8;
                    }
                }
            },
        );
        ui.add_space(12.0);
        ui.separator();
        ui.add_space(16.0);
        ui.vertical(|ui| {
            ui.set_min_width((width - sidebar_width - 36.0).max(300.0));
            let search =
                super::text_field(ui, "", &mut app.settings_search, false, "Search settings");
            if search.changed()
                && let Some(section) = settings_search_section(&app.settings_search)
            {
                app.settings_section = section;
            }
            ui.add_space(14.0);
            settings_body(app, ui);
            ui.add_space(40.0);
        });
    });
}

fn settings_body(app: &mut App, ui: &mut Ui) {
    match app.settings_section {
        0 => {
            settings_account(app, ui);
            settings_window(app, ui);
            settings_accessibility(app, ui);
            settings_shortcuts(ui);
            section_label(ui, "Network", None);
            settings_card(ui, |ui| network(app, ui));
        }
        1 => settings_playback(app, ui),
        2 => settings_audio(app, ui),
        3 => settings_downloads(ui),
        4 => settings_library(app, ui),
        5 => settings_lyrics(app, ui),
        6 => {
            settings_appearance(app, ui);
            settings_artists(app, ui);
        }
        7 => settings_lastfm(app, ui),
        8 => settings_integrations(app, ui),
        9 => settings_sources(app, ui),
        10 => settings_experimental(app, ui),
        _ => settings_about(ui),
    }
}

fn settings_search_section(query: &str) -> Option<u8> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return None;
    }
    let groups: &[(&[&str], u8)] = &[
        (
            &[
                "general",
                "account",
                "window",
                "accessibility",
                "touchpad",
                "keyboard",
                "shortcut",
                "network",
                "proxy",
                "motion",
            ],
            0,
        ),
        (
            &[
                "playback",
                "autoplay",
                "crossfade",
                "automix",
                "video",
                "sponsor",
            ],
            1,
        ),
        (
            &["audio", "volume", "device", "normalize", "equalizer", "eq"],
            2,
        ),
        (&["download", "offline", "folder"], 3),
        (
            &[
                "library", "playlist", "liked", "history", "upload", "backup", "restore", "smart",
            ],
            4,
        ),
        (&["lyric", "roman", "translation"], 5),
        (
            &[
                "appearance",
                "theme",
                "font",
                "colour",
                "color",
                "animation",
                "artwork",
            ],
            6,
        ),
        (&["last.fm", "lastfm", "scrobble"], 7),
        (&["integration", "discord", "spotify"], 8),
        (&["source", "hi-res", "atmos", "lossless"], 9),
        (&["experimental", "renderer", "gpu"], 10),
        (&["about", "version", "license"], 11),
    ];
    groups.iter().find_map(|(words, section)| {
        words
            .iter()
            .any(|word| q.contains(word))
            .then_some(*section)
    })
}

/// Settings › Account: signing in and out, and which channel to use.
fn settings_account(app: &mut App, ui: &mut Ui) {
    section_label(ui, "Account", None);
    let busy = app.signing_in.load(Ordering::Relaxed);
    let (mut sign_in, mut sign_out) = (false, false);
    // Asked again now and then while Settings shows: a failed ask, or a channel made since.
    let now = ui.input(|i| i.time);
    if app.logged_in && app.channels_asked.is_none_or(|at| now - at > 60.0) {
        app.channels_asked = Some(now);
        app.backend.request(crate::backend::Req::Channels);
    }
    settings_card(ui, |ui| {
        if app.logged_in {
            let name = app
                .account_name
                .clone()
                .unwrap_or_else(|| "YouTube Music".to_owned());
            setting_row(
                ui,
                &name,
                Some(
                    "Signed in with Google. Your home feed, library and likes come from this account. Saved encrypted in Windows Credential Manager.",
                ),
                |ui| {
                    sign_out = pill(ui, "Sign out", Some(icon::LOG_OUT), false).clicked();
                },
            );
            // Also with one, when the one in use isn't it (a brand channel since deleted).
            if app
                .channels
                .as_ref()
                .is_some_and(|c| c.list.len() > 1 || !c.list.iter().any(|l| l.page_id == c.current))
            {
                divider(ui);
                setting_row(
                    ui,
                    "Channel",
                    Some(
                        "Your Google account and each YouTube channel it manages have their own library, likes, history and recommendations.",
                    ),
                    |ui| {
                        channel_picker(app, ui);
                    },
                );
            }
        } else {
            setting_row(
                ui,
                "Not signed in",
                Some("Sign in to get your home feed, library and likes."),
                |ui| {
                    if busy {
                        ui.spinner();
                    } else {
                        sign_in = pill(ui, "Sign in with Google", Some(icon::CIRCLE_USER), true)
                            .clicked();
                    }
                },
            );
        }
    });
    if busy {
        ui.add_space(6.0);
        ui.label(
            RichText::new("Finish signing in in the window that opened.")
                .font(theme::regular(12.5))
                .color(theme::text_dim()),
        );
    }
    if sign_in {
        app.start_sign_in(ui.ctx());
    }
    if sign_out {
        app.backend.request(crate::backend::Req::Logout);
    }
}

fn channel_picker(app: &mut App, ui: &mut Ui) {
    let Some(crate::backend::Channels {
        list,
        current,
        epoch,
    }) = &app.channels
    else {
        return;
    };
    // A brand channel often has the account's name: the handle, or "No channel" (as
    // YouTube's switcher says), tells them apart.
    let label = |c: &crate::ytm::Channel| match (&c.handle, c.has_channel) {
        (Some(h), _) => format!("{} ({h})", c.name),
        (None, false) => format!("{} (No channel)", c.name),
        (None, true) => c.name.clone(),
    };
    // The button is narrower: a handle alone is enough there.
    let shown = match list.iter().find(|c| c.page_id == *current) {
        Some(c) => c.handle.clone().unwrap_or_else(|| label(c)),
        None => "Another channel".to_owned(),
    };
    // A transfer would go on as the new channel.
    if app.transfer.running() {
        ui.label(
            RichText::new("Not while a transfer runs")
                .font(theme::regular(13.0))
                .color(theme::text_dim()),
        );
        return;
    }
    let switching = app.switching_to.is_some();
    let btn = dropdown_button(
        ui,
        if switching { "Switching…" } else { &shown },
        220.0,
        "Channel",
    );
    let mut pick = None;
    egui::Popup::menu(&btn).show(|ui| {
        ui.set_width(300.0);
        ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
        egui::ScrollArea::vertical()
            .max_height(320.0)
            .show(ui, |ui| {
                ui.add_space(4.0);
                for c in list {
                    if option_row(ui, &label(c), c.page_id == *current).clicked() {
                        pick = Some(c.clone());
                        ui.close();
                    }
                }
                ui.add_space(4.0);
            });
    });
    if let Some(c) = pick.filter(|c| c.page_id != *current && !switching) {
        app.switching_to = Some(label(&c));
        app.backend.request(crate::backend::Req::SwitchChannel {
            page_id: c.page_id,
            epoch: *epoch,
        });
    }
}

/// Settings › Playback.
fn settings_playback(app: &mut App, ui: &mut Ui) {
    section_label(ui, "Playback", None);
    settings_card(ui, |ui| {
        setting_row(
            ui,
            "Autoplay",
            Some("Keep playing similar songs when the queue ends."),
            |ui| {
                toggle(ui, &mut app.autoplay, "Autoplay");
            },
        );
        divider(ui);
        setting_row(
            ui,
            "Play songs, not music videos",
            Some(
                "An official music video plays as its song, the album recording, when YouTube Music has one: no intros, skits or video-only edits. Lists still show what they hold.",
            ),
            |ui| {
                toggle(ui, &mut app.play_songs, "Play songs, not music videos");
            },
        );
        divider(ui);
        setting_row(
            ui,
            "AutoMix",
            Some("Blend songs using Marshall's phrase-aware handoff."),
            |ui| {
                if toggle(ui, &mut app.automix, "AutoMix").changed() {
                    app.set_crossfade();
                }
            },
        );
        divider(ui);
        let now = ui.input(|i| i.time);
        let timer_label = app.sleep_timer_deadline.map_or_else(
            || "Off".to_owned(),
            |deadline| {
                let seconds = (deadline - now).ceil().max(0.0) as u64;
                format!("{}:{:02} remaining", seconds / 60, seconds % 60)
            },
        );
        setting_row(
            ui,
            "Sleep timer",
            Some("Pause playback after a set time or at the end of this song."),
            |ui| {
                let button = dropdown_button(ui, &timer_label, 190.0, "Sleep timer");
                egui::Popup::menu(&button).show(|ui| {
                    ui.set_width(210.0);
                    for (name, minutes) in [
                        ("Off", 0u16),
                        ("15 minutes", 15),
                        ("30 minutes", 30),
                        ("45 minutes", 45),
                        ("60 minutes", 60),
                    ] {
                        if option_row(
                            ui,
                            name,
                            (minutes == 0 && app.sleep_timer_deadline.is_none())
                                || (minutes > 0
                                    && app.sleep_timer_deadline.is_some_and(|d| {
                                        (d - now - f64::from(minutes) * 60.0).abs() < 3.0
                                    })),
                        )
                        .clicked()
                        {
                            app.sleep_timer_deadline =
                                (minutes > 0).then_some(now + f64::from(minutes) * 60.0);
                            ui.close();
                        }
                    }
                    divider(ui);
                    if option_row(ui, "At end of track", false).clicked() {
                        let duration = app
                            .audio
                            .status
                            .duration_ms
                            .load(std::sync::atomic::Ordering::Relaxed);
                        let position = app.audio.status.position_smooth().as_millis() as u64;
                        if duration > position {
                            app.sleep_timer_deadline =
                                Some(now + (duration - position) as f64 / 1000.0);
                        } else {
                            app.toast("Play a track first");
                        }
                        ui.close();
                    }
                });
            },
        );
        if !app.automix {
            divider(ui);
            setting_row(
                ui,
                "Crossfade songs",
                Some("Blend the end of one song into the start of the next."),
                |ui| {
                    if toggle(ui, &mut app.crossfade, "Crossfade songs").changed() {
                        app.set_crossfade();
                    }
                },
            );
            if app.crossfade {
                divider(ui);
                setting_row(ui, "Crossfade length", None, |ui| {
                    if seconds_slider(
                        ui,
                        &mut app.crossfade_secs,
                        1..=super::CROSSFADE_MAX,
                        "Crossfade length",
                    )
                    .changed()
                    {
                        app.set_crossfade();
                    }
                });
            }
        }
        divider(ui);
        setting_row(
            ui,
            "Skip non-music parts",
            Some("Jump over talking, skits and sponsor breaks marked by the community."),
            |ui| {
                if toggle(ui, &mut app.sponsorblock, "Skip non-music parts").changed() {
                    app.request_segments();
                }
            },
        );
    });
}

fn settings_audio(app: &mut App, ui: &mut Ui) {
    section_label(ui, "Audio", None);
    settings_card(ui, |ui| {
        setting_row(
            ui,
            "Output device",
            Some("Choose where Marshall plays audio."),
            |ui| output_device_picker(app, ui),
        );
        divider(ui);
        setting_row(
            ui,
            "Volume normalization",
            Some("Turn loud songs down to the same level."),
            |ui| {
                if toggle(ui, &mut app.normalize, "Volume normalization").changed() {
                    app.audio.set_normalize(app.normalize);
                }
            },
        );
        divider(ui);
        setting_row(
            ui,
            "Logarithmic volume",
            Some("Give quiet listening levels more room on the volume slider."),
            |ui| {
                if toggle(ui, &mut app.log_volume, "Logarithmic volume").changed() {
                    app.audio.set_log_volume(app.log_volume);
                }
            },
        );
        ui.add_space(8.0);
        ui.label(
            RichText::new("Audio output stays on Marshall's playback engine.")
                .font(theme::regular(12.5))
                .color(theme::text_dim()),
        );
    });
    settings_equalizer(app, ui);
}

/// The ten-band EQ and processing controls, arranged to match the supplied panel.
fn settings_equalizer(app: &mut App, ui: &mut Ui) {
    const PRESETS: [(&str, [f32; 10]); 13] = [
        ("Default", [0.0; 10]),
        (
            "Studio Master",
            [2.7, 2.2, 0.6, -2.6, -0.6, 1.2, 3.0, 4.0, 4.3, 4.8],
        ),
        (
            "Vocal Clarity",
            [-1.0, -1.0, 0.0, 1.0, 3.0, 3.0, 2.0, 0.0, 1.0, 1.0],
        ),
        (
            "Deep Bass Clean",
            [2.0, 1.0, 0.0, -1.0, -1.0, 0.0, 1.0, 1.0, 0.0, 0.0],
        ),
        (
            "Bass Boost",
            [4.0, 3.0, 2.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        ),
        ("Warm", [2.0, 1.0, 0.0, -1.0, -2.0, 0.0, 1.0, 2.0, 1.0, 0.0]),
        (
            "Bright",
            [-1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 2.0, 3.0, 3.0, 2.0],
        ),
        (
            "Late Night",
            [-2.0, -1.0, 1.0, 2.0, 1.0, 0.0, 1.0, 1.0, 0.0, -1.0],
        ),
        ("Rock", [3.0, 2.0, 0.0, -1.0, -2.0, 1.0, 2.0, 3.0, 3.0, 2.0]),
        ("Pop", [1.0, 2.0, 3.0, 1.0, 0.0, 1.0, 2.0, 1.0, 0.0, 1.0]),
        (
            "Electronic",
            [3.0, 2.0, 0.0, 0.0, -1.0, 1.0, 2.0, 3.0, 2.0, 2.0],
        ),
        ("R&B", [3.0, 2.0, 1.0, -1.0, 0.0, 1.0, 2.0, 1.0, 0.0, 1.0]),
        (
            "Acoustic",
            [-1.0, 0.0, 1.0, 2.0, 2.0, 1.0, 0.0, 1.0, 2.0, 1.0],
        ),
    ];
    const PROFILES: [(&str, &str); 4] = [
        ("Reference", "Unmodified reference curve; no stage trims."),
        (
            "Speaker",
            "Softer low bass and top end for small desktop speakers.",
        ),
        (
            "Headphone",
            "Gentler bass and presence for close-coupled drivers.",
        ),
        ("DAC", "Smoother upper shelf for external DAC chains."),
    ];
    const FREQS: [&str; 10] = [
        "31", "63", "125", "250", "500", "1k", "2k", "4k", "8k", "16k",
    ];

    section_label(
        ui,
        "Equalizer & Sound FX",
        Some("10-band ISO curves with acoustic & stage compensation"),
    );
    settings_card(ui, |ui| {
        let eq_desc = if app.eq_enabled {
            format!("{} active", app.eq_preset)
        } else {
            "Saved and ready when enabled".to_owned()
        };
        setting_row(ui, "Equalizer & Sound FX", Some(&eq_desc), |ui| {
            let mut enabled = app.eq_enabled;
            if toggle(ui, &mut enabled, "Equalizer & Sound FX").changed() {
                app.eq_enabled = enabled;
                apply_eq(app);
            }
        });
        divider(ui);
        ui.horizontal(|ui| {
            let tab_w = (ui.available_width() - 12.0) / 2.0;
            ui.allocate_ui_with_layout(
                vec2(tab_w, 38.0),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    if chip(ui, "10-Band EQ", app.eq_panel_tab == 0).clicked() {
                        app.eq_panel_tab = 0;
                    }
                },
            );
            ui.allocate_ui_with_layout(
                vec2(tab_w, 38.0),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    if chip(ui, "★  Studio Clarity", app.eq_panel_tab == 1).clicked() {
                        app.eq_panel_tab = 1;
                    }
                },
            );
        });
        ui.add_space(10.0);
        eq_curve_preview(
            ui,
            &active_eq(&app.eq_gains, &app.eq_profile, app.eq_sfx_enabled),
            Color32::from_rgb(230, 181, 79),
        );
        ui.add_space(10.0);
        if app.eq_panel_tab == 0 {
            egui::ScrollArea::horizontal()
                .id_salt("eq-presets")
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        for (name, curve) in PRESETS {
                            if chip(ui, name, app.eq_preset == name).clicked() {
                                app.eq_preset = name.to_owned();
                                app.eq_gains = curve;
                                apply_eq(app);
                            }
                        }
                    });
                });
            ui.add_space(10.0);
            egui::Frame::new()
                .fill(theme::panel(0.26))
                .corner_radius(14)
                .inner_margin(egui::Margin::symmetric(12, 10))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        for (i, label) in FREQS.into_iter().enumerate() {
                            let width = (ui.available_width() / (10 - i).max(1) as f32).min(78.0);
                            ui.allocate_ui_with_layout(
                                vec2(width, 190.0),
                                egui::Layout::top_down(egui::Align::Center),
                                |ui| {
                                    ui.label(
                                        RichText::new(format!("{:+.1}", app.eq_gains[i]))
                                            .font(theme::semibold(11.0))
                                            .color(if app.eq_gains[i] < 0.0 {
                                                Color32::from_rgb(70, 171, 245)
                                            } else {
                                                theme::text()
                                            }),
                                    );
                                    let response = ui.add_sized(
                                        vec2(30.0, 142.0),
                                        egui::Slider::new(&mut app.eq_gains[i], -12.0..=12.0)
                                            .vertical()
                                            .show_value(false),
                                    );
                                    ui.label(
                                        RichText::new(label)
                                            .font(theme::semibold(11.0))
                                            .color(theme::text_dim()),
                                    );
                                    if response.double_clicked() {
                                        app.eq_gains[i] = 0.0;
                                        app.eq_preset = "Custom".to_owned();
                                        apply_eq(app);
                                    } else if response.changed() {
                                        app.eq_preset = "Custom".to_owned();
                                        apply_eq(app);
                                    }
                                },
                            );
                        }
                    });
                });
            ui.add_space(8.0);
            ui.horizontal_wrapped(|ui| {
                if chip(ui, "Flat (0 dB)", app.eq_gains == [0.0; 10]).clicked() {
                    app.eq_gains = [0.0; 10];
                    app.eq_preset = "Default".to_owned();
                    apply_eq(app);
                }
                if chip(ui, "+2 dB Bass", false).clicked() {
                    app.eq_gains = [2.0, 2.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
                    app.eq_preset = "Custom".to_owned();
                    apply_eq(app);
                }
                if chip(ui, "+2 dB Treble", false).clicked() {
                    app.eq_gains = [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 2.0, 2.0];
                    app.eq_preset = "Custom".to_owned();
                    apply_eq(app);
                }
                ui.label(
                    RichText::new("Double-click a band to zero it")
                        .font(theme::regular(11.5))
                        .color(theme::text_dim()),
                );
            });
        } else {
            let mut clarity = app.eq_sfx_enabled;
            setting_row(
                ui,
                "Studio Master Clarity",
                Some(
                    "Reduces low-mid boxiness, steadies sub-bass and adds a soft, clear air shelf.",
                ),
                |ui| {
                    if toggle(ui, &mut clarity, "Studio Master Clarity").changed() {
                        app.eq_sfx_enabled = clarity;
                        apply_eq(app);
                    }
                },
            );
            ui.add_space(12.0);
            ui.label(
                RichText::new("ACOUSTIC TARGET PROFILE")
                    .font(theme::semibold(11.0))
                    .color(theme::text_dim()),
            );
            for (name, description) in PROFILES {
                let (rect, response) =
                    ui.allocate_exact_size(vec2(ui.available_width(), 62.0), Sense::click());
                let selected = app.eq_profile == name;
                let fill = if selected {
                    Color32::from_rgb(47, 39, 25)
                } else {
                    theme::panel(0.28)
                };
                let stroke = if selected {
                    Color32::from_rgb(238, 166, 31)
                } else {
                    theme::glass(0.03)
                };
                ui.painter().rect(
                    rect,
                    14.0,
                    fill,
                    egui::Stroke::new(if selected { 1.6 } else { 1.0 }, stroke),
                    egui::StrokeKind::Inside,
                );
                let dot = pos2(rect.left() + 22.0, rect.center().y);
                ui.painter().circle_stroke(
                    dot,
                    8.0,
                    egui::Stroke::new(
                        2.0,
                        if selected {
                            Color32::from_rgb(238, 166, 31)
                        } else {
                            theme::text_faint()
                        },
                    ),
                );
                widgets::text_at(
                    ui,
                    pos2(rect.left() + 42.0, rect.top() + 18.0),
                    name,
                    theme::semibold(13.5),
                    theme::text(),
                    rect.width() - 56.0,
                );
                widgets::text_at(
                    ui,
                    pos2(rect.left() + 42.0, rect.top() + 39.0),
                    description,
                    theme::regular(11.5),
                    theme::text_dim(),
                    rect.width() - 56.0,
                );
                if response.clicked() {
                    app.eq_profile = name.to_owned();
                    apply_eq(app);
                }
                ui.add_space(6.0);
            }
            eq_curve_preview(
                ui,
                &active_eq(&app.eq_gains, &app.eq_profile, app.eq_sfx_enabled),
                Color32::from_rgb(190, 190, 190),
            );
        }
        ui.add_space(8.0);
        ui.label(RichText::new("Floating-point biquad filters run in Marshall’s playback pipeline with automatic peak headroom.").font(theme::regular(11.5)).color(theme::text_dim()));
    });
}

fn apply_eq(app: &mut App) {
    app.audio.set_eq_gains(if app.eq_enabled {
        active_eq(&app.eq_gains, &app.eq_profile, app.eq_sfx_enabled)
    } else {
        [0.0; 10]
    });
}

pub(super) fn active_eq(gains: &[f32; 10], profile: &str, clarity: bool) -> [f32; 10] {
    let clarity_curve = if clarity {
        [-0.5, -0.5, -0.2, -1.2, -1.4, 0.2, 0.7, 1.0, 1.3, 1.1]
    } else {
        [0.0; 10]
    };
    let combined = std::array::from_fn(|i| (gains[i] + clarity_curve[i]).clamp(-12.0, 12.0));
    profile_eq(&combined, profile)
}

fn eq_curve_preview(ui: &mut Ui, gains: &[f32; 10], accent: Color32) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 78.0), Sense::hover());
    ui.painter().rect_filled(rect, 12.0, theme::panel(0.36));
    let baseline = rect.center().y;
    ui.painter().line_segment(
        [
            pos2(rect.left() + 14.0, baseline),
            pos2(rect.right() - 14.0, baseline),
        ],
        egui::Stroke::new(1.0, theme::glass(0.12)),
    );
    let points: Vec<_> = gains
        .iter()
        .enumerate()
        .map(|(i, gain)| {
            let x = rect.left() + 14.0 + (rect.width() - 28.0) * i as f32 / 9.0;
            let y = baseline - gain.clamp(-12.0, 12.0) * 2.05;
            pos2(x, y)
        })
        .collect();
    for pair in points.windows(2) {
        ui.painter().add(egui::Shape::convex_polygon(
            vec![
                pair[0],
                pair[1],
                pos2(pair[1].x, baseline),
                pos2(pair[0].x, baseline),
            ],
            Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 36),
            egui::Stroke::NONE,
        ));
        ui.painter()
            .line_segment([pair[0], pair[1]], egui::Stroke::new(2.2, accent));
    }
    for point in points {
        ui.painter().circle_filled(point, 3.0, accent);
    }
}

fn profile_eq(gains: &[f32; 10], profile: &str) -> [f32; 10] {
    let offsets = match profile {
        "Speaker" => [-1.5, -0.5, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, -0.5, -0.5],
        "Headphone" => [-0.5, -0.5, 0.0, 0.0, 0.0, -0.5, -1.0, 0.0, -0.5, -0.5],
        "DAC" => [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, -1.0, -1.5, -1.5],
        _ => [0.0; 10],
    };
    std::array::from_fn(|i| (gains[i] + offsets[i]).clamp(-12.0, 12.0))
}

fn settings_sources(app: &mut App, ui: &mut Ui) {
    section_label(ui, "Sources", None);
    settings_card(ui, |ui| {
        setting_row(
            ui,
            "Hi-Res audio",
            Some(
                "Addon servers can provide hi-res copies when available; Marshall shows the active quality in the player.",
            ),
            |_| {},
        );
        divider(ui);
        ui.add_space(12.0);
        hires_sources_editor(app, ui);
        ui.add_space(10.0);
        divider(ui);
        setting_row(
            ui,
            "Prefer Dolby Atmos",
            Some(
                "Ask configured hi-res sources for Dolby Atmos, then fall back to stereo when needed.",
            ),
            |ui| {
                if toggle(ui, &mut app.prefer_atmos, "Prefer Dolby Atmos").changed() {
                    app.backend.set_prefer_atmos(app.prefer_atmos);
                }
            },
        );
    });
}

/// The hi-res source list: one line per addon, and "+ Add a source" below it that
/// opens a fresh line for the next one.
fn hires_sources_editor(app: &mut App, ui: &mut Ui) {
    let mut edited = false;
    let mut remove = None;
    for i in 0..app.hires_sources.len() {
        ui.horizontal(|ui| {
            let field_w = (ui.available_width() - 34.0).max(120.0);
            ui.allocate_ui_with_layout(
                vec2(field_w, 42.0),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.set_width(field_w);
                    if super::text_field(
                        ui,
                        "",
                        &mut app.hires_sources[i],
                        false,
                        "https://your-addon.example",
                    )
                    .changed()
                    {
                        edited = true;
                    }
                },
            );
            if widgets::icon_button(ui, icon::X, 14.0, "Remove source", false).clicked() {
                remove = Some(i);
            }
        });
        ui.add_space(6.0);
    }
    if let Some(i) = remove {
        app.hires_sources.remove(i);
        edited = true;
    }
    if edited {
        app.backend.set_hires_source(&live_sources(app));
    }
    if app.hires_adding {
        let resp = super::text_field(
            ui,
            "",
            &mut app.hires_draft,
            false,
            "https://your-addon.example",
        );
        if app.hires_focus {
            resp.request_focus();
            app.hires_focus = false;
        }
        if resp.lost_focus() {
            let link = app.hires_draft.trim().to_owned();
            if !link.is_empty() && !app.hires_sources.iter().any(|s| s.trim() == link) {
                app.hires_sources.push(link);
                app.backend.set_hires_source(&live_sources(app));
            }
            app.hires_draft.clear();
            app.hires_adding = false;
        }
        ui.add_space(6.0);
    } else if pill(ui, "+  Add a source", None, false).clicked() {
        app.hires_adding = true;
        app.hires_focus = true;
        app.hires_draft.clear();
    }
}

/// The non-empty links as one newline-separated list, the way the backend reads them.
fn live_sources(app: &App) -> String {
    app.hires_sources
        .iter()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Settings › Downloads.
fn settings_downloads(ui: &mut Ui) {
    section_label(ui, "Downloads", None);
    ui.label(
        RichText::new("Download songs from their menu. They're saved at the best quality your account gets, tagged with their cover art, and play from the file, even offline.")
            .font(theme::regular(13.0))
            .color(theme::text_dim()),
    );
    ui.add_space(12.0);
    settings_card(ui, |ui| {
        let folder = crate::downloads::folder().display().to_string();
        setting_row(ui, "Download folder", Some(&folder), |ui| {
            if pill(ui, "Change", None, false).clicked() {
                crate::downloads::pick_folder(ui.ctx());
            }
            ui.add_space(6.0);
            if pill(ui, "Open", Some(icon::FOLDER_OPEN), false).clicked() {
                crate::downloads::open_folder();
            }
        });
    });
}

/// Settings › Lyrics: the better-lyrics extension's options.
fn settings_lyrics(app: &mut App, ui: &mut Ui) {
    section_label(ui, "Lyrics", None);
    settings_card(ui, |ui| {
        setting_row(
            ui,
            "Romanization",
            Some(
                "Japanese, Korean, Chinese, Cyrillic and other scripts in Latin letters. Timed with the lyrics when the source has it.",
            ),
            |ui| {
                toggle(ui, &mut app.show_roman, "Romanization");
            },
        );
        divider(ui);
        setting_row(
            ui,
            "Translation",
            Some(
                "Translated by Unison, or Google when it can't. Lyrics that ship their own translation use it.",
            ),
            |ui| {
                toggle(ui, &mut app.show_trans, "Translation");
            },
        );
        divider(ui);
        setting_row(ui, "Translate into", None, |ui| {
            let current = super::lyric_prefs::LANGUAGES
                .iter()
                .find(|(c, _)| *c == app.lyric_prefs.target)
                .map_or(app.lyric_prefs.target.as_str(), |(_, n)| n);
            let btn = dropdown_button(ui, current, 200.0, "Translate into");
            let just_opened = btn.clicked();
            egui::Popup::menu(&btn).show(|ui| {
                ui.set_width(200.0);
                ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                egui::ScrollArea::vertical()
                    .max_height(320.0)
                    .show(ui, |ui| {
                        ui.add_space(4.0);
                        for (code, name) in super::lyric_prefs::LANGUAGES {
                            let on = app.lyric_prefs.target == *code;
                            let row = option_row(ui, name, on);
                            if on && just_opened {
                                row.scroll_to_me(Some(egui::Align::Center));
                            }
                            if row.clicked() {
                                app.lyric_prefs.target = (*code).to_owned();
                                ui.close();
                            }
                        }
                        ui.add_space(4.0);
                    });
            });
        });
        divider(ui);
        setting_row(
            ui,
            "Text size",
            Some("Make the lyrics larger or smaller. Works on top of the theme's own size."),
            |ui| {
                let (min, max, step) = super::lyric_prefs::TEXT_SCALE;
                let v = &mut app.lyric_prefs.text_scale;
                if ui
                    .add_enabled(
                        *v < max - 0.01,
                        egui::Button::new("+").min_size(vec2(28.0, 26.0)),
                    )
                    .on_hover_text("Larger")
                    .clicked()
                {
                    *v = ((*v + step) * 10.0).round() / 10.0;
                }
                ui.label(
                    egui::RichText::new(format!("{:.0}%", *v * 100.0)).font(theme::semibold(13.0)),
                );
                if ui
                    .add_enabled(
                        *v > min + 0.01,
                        egui::Button::new("−").min_size(vec2(28.0, 26.0)),
                    )
                    .on_hover_text("Smaller")
                    .clicked()
                {
                    *v = ((*v - step) * 10.0).round() / 10.0;
                }
            },
        );
        divider(ui);
        // Offsets: seconds taken off the lyrics clock, as the extension's dock sets them.
        for (title, desc, which) in [
            (
                "Timing offset",
                "Shift all lyrics later (+) or earlier (−).",
                0,
            ),
            (
                "Word-synced trim",
                "Extra shift for syllable and word-synced lyrics.",
                1,
            ),
            ("Line-synced trim", "Extra shift for line-synced lyrics.", 2),
        ] {
            setting_row(ui, title, Some(desc), |ui| {
                let v = match which {
                    0 => &mut app.lyric_prefs.global_offset,
                    1 => &mut app.lyric_prefs.word_trim,
                    _ => &mut app.lyric_prefs.line_trim,
                };
                if ui
                    .add(egui::Button::new("+").min_size(vec2(28.0, 26.0)))
                    .on_hover_text("Later")
                    .clicked()
                {
                    *v = ((*v + 0.1) * 10.0).round() / 10.0;
                }
                ui.label(
                    egui::RichText::new(format!("{:+.1} s", *v).replace("+0.0", "0.0"))
                        .font(theme::semibold(13.0)),
                );
                if ui
                    .add(egui::Button::new("−").min_size(vec2(28.0, 26.0)))
                    .on_hover_text("Earlier")
                    .clicked()
                {
                    *v = ((*v - 0.1) * 10.0).round() / 10.0;
                }
            });
            divider(ui);
        }
        // Source order: the first enabled source with lyrics for a song is shown; the
        // lyrics toolbar can pick another for that song.
        setting_row(
            ui,
            "Lyrics sources",
            Some(
                "Tried in this order. Switch one off to skip it; the toolbar under the lyrics picks a different one for a single song.",
            ),
            |_| {},
        );
        let n = app.lyric_prefs.order.len();
        let mut swap: Option<(usize, usize)> = None;
        for i in 0..n {
            let (p, on) = app.lyric_prefs.order[i];
            let (rect, _) =
                ui.allocate_exact_size(vec2(ui.available_width(), 34.0), Sense::hover());
            let mut row = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(rect)
                    .layout(egui::Layout::left_to_right(egui::Align::Center)),
            );
            let mut enabled = on;
            if toggle(&mut row, &mut enabled, p.name()).changed() {
                app.lyric_prefs.order[i].1 = enabled;
            }
            row.add_space(10.0);
            let kind = match p.sync() {
                crate::lyrics::SyncKind::Syllable => "syllable",
                crate::lyrics::SyncKind::Word => "word",
                crate::lyrics::SyncKind::Line => "line",
                crate::lyrics::SyncKind::Unsynced => "plain",
            };
            row.label(
                egui::RichText::new(p.name())
                    .font(theme::semibold(13.5))
                    .color(if on { theme::text() } else { theme::text_dim() }),
            );
            row.label(
                egui::RichText::new(kind)
                    .font(theme::regular(12.0))
                    .color(theme::text_dim()),
            );
            row.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add_enabled(i + 1 < n, egui::Button::new("↓").min_size(vec2(28.0, 24.0)))
                    .on_hover_text("Lower")
                    .clicked()
                {
                    swap = Some((i, i + 1));
                }
                if ui
                    .add_enabled(i > 0, egui::Button::new("↑").min_size(vec2(28.0, 24.0)))
                    .on_hover_text("Higher")
                    .clicked()
                {
                    swap = Some((i, i - 1));
                }
            });
        }
        if let Some((a, b)) = swap {
            app.lyric_prefs.order.swap(a, b);
        }
        ui.add_space(6.0);
    });
}

/// Settings › Artist pages: the listener's own signature service.
fn settings_artists(app: &mut App, ui: &mut Ui) {
    section_label(ui, "Artist pages", None);
    settings_card(ui, |ui| {
        setting_row(
            ui,
            "Signature service",
            Some(
                "A link to your own signature API. Artists get their handwritten signature, with “Signed by …” above it — beside About when Apple also letters them a wordmark; without a signature, their name is set in type.",
            ),
            |_| {},
        );
        divider(ui);
        ui.add_space(12.0);
        if super::text_field(
            ui,
            "Link",
            &mut app.signature_api,
            false,
            "https://your-signature-service.example",
        )
        .changed()
        {
            app.signature_reset();
        }
        ui.add_space(12.0);
    });
}

/// Settings › Appearance.
fn settings_appearance(app: &mut App, ui: &mut Ui) {
    section_label(ui, "Appearance", None);
    settings_card(ui, |ui| {
        let current_theme = crate::preset::get(&app.theme_id).name.clone();
        let mut chosen_theme = None;
        setting_row(
            ui,
            "Theme",
            Some("Choose a Marshall theme preset for color, surfaces, and player styling."),
            |ui| {
                let button = dropdown_button(ui, &current_theme, 210.0, "Theme");
                egui::Popup::menu(&button).show(|ui| {
                    ui.set_width(240.0);
                    ui.spacing_mut().item_spacing.y = 2.0;
                    for preset in crate::preset::all() {
                        if option_row(ui, &preset.name, preset.id == app.theme_id).clicked() {
                            chosen_theme = Some(preset.id.clone());
                            ui.close();
                        }
                    }
                });
            },
        );
        if let Some(id) = chosen_theme {
            app.theme_id = id;
            crate::preset::set_active(&app.theme_id);
        }
        divider(ui);
        let desc = if cfg!(windows) {
            "Light or dark pages. System follows Windows' app mode. Now playing stays dark, over the album art."
        } else {
            "Light or dark pages. System follows your system's setting. Now playing stays dark, over the album art."
        };
        let mut pick = None;
        setting_row(ui, "Mode", Some(desc), |ui| {
            let btn = dropdown_button(ui, app.appearance.label(), 160.0, "Mode");
            egui::Popup::menu(&btn).show(|ui| {
                ui.set_width(160.0);
                ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                ui.add_space(4.0);
                for a in theme::Appearance::ALL {
                    if option_row(ui, a.label(), a == app.appearance).clicked() {
                        pick = Some(a);
                        ui.close();
                    }
                }
                ui.add_space(4.0);
            });
        });
        if let Some(a) = pick {
            app.appearance = a;
        }
        divider(ui);
        setting_row(
            ui,
            "Accent source",
            Some("Choose the colour used by active controls and highlights."),
            |ui| {
                let labels = [
                    "Theme default",
                    "Album art",
                    "System colour",
                    "Custom colour",
                ];
                let current = labels[app.accent_source.min(3) as usize];
                let button = dropdown_button(ui, current, 190.0, "Accent source");
                egui::Popup::menu(&button).show(|ui| {
                    ui.set_width(190.0);
                    for (i, label) in labels.iter().enumerate() {
                        if option_row(ui, label, app.accent_source as usize == i).clicked() {
                            app.accent_source = i as u8;
                            ui.close();
                        }
                    }
                });
            },
        );
        if app.accent_source == 3 {
            divider(ui);
            setting_row(
                ui,
                "Accent colour",
                Some("Pick a preset swatch or choose any colour."),
                |ui| {
                    const SWATCHES: [Color32; 8] = [
                        Color32::from_rgb(244, 243, 240),
                        Color32::from_rgb(76, 194, 255),
                        Color32::from_rgb(232, 51, 58),
                        Color32::from_rgb(126, 75, 255),
                        Color32::from_rgb(31, 154, 196),
                        Color32::from_rgb(101, 159, 108),
                        Color32::from_rgb(228, 158, 43),
                        Color32::from_rgb(225, 66, 112),
                    ];
                    for swatch in SWATCHES {
                        let (rect, response) =
                            ui.allocate_exact_size(vec2(28.0, 28.0), Sense::click());
                        ui.painter().circle_filled(rect.center(), 11.0, swatch);
                        if app.custom_accent == swatch {
                            ui.painter().text(
                                rect.center(),
                                Align2::CENTER_CENTER,
                                "✓",
                                theme::semibold(14.0),
                                Color32::WHITE,
                            );
                        }
                        if response.clicked() {
                            app.custom_accent = swatch;
                        }
                    }
                    ui.add_space(4.0);
                    ui.color_edit_button_srgba(&mut app.custom_accent);
                },
            );
        }
        divider(ui);
        setting_row(
            ui,
            "Haze material",
            Some("Adjust the translucency of Marshall's cards and controls."),
            |ui| {
                let label = if app.haze_mode == 1 {
                    "Solid"
                } else {
                    "Automatic"
                };
                let button = dropdown_button(ui, label, 160.0, "Haze material");
                egui::Popup::menu(&button).show(|ui| {
                    ui.set_width(160.0);
                    for (value, name) in [(0, "Automatic"), (1, "Solid")] {
                        if option_row(ui, name, app.haze_mode == value).clicked() {
                            app.haze_mode = value;
                            ui.close();
                        }
                    }
                });
            },
        );
        if app.haze_mode == 0 {
            setting_row(ui, "Haze intensity", None, |ui| {
                for (i, label) in ["Low", "Medium", "High"].iter().enumerate() {
                    if widgets::chip(ui, label, app.haze_intensity as usize == i).clicked() {
                        app.haze_intensity = i as u8;
                    }
                }
            });
        }
        divider(ui);
        setting_row(
            ui,
            "Now Playing ambience",
            Some("Choose the background behind the cover and the unchanged lyrics panel."),
            |ui| {
                if widgets::chip(ui, "Album art", app.np_artwork_ambient).clicked() {
                    app.np_artwork_ambient = true;
                }
                if widgets::chip(ui, "Marshall", !app.np_artwork_ambient).clicked() {
                    app.np_artwork_ambient = false;
                }
            },
        );
        divider(ui);
        setting_row(
            ui,
            "AMOLED black",
            Some("Use pure-black dark surfaces. Light mode is unaffected."),
            |ui| {
                toggle(ui, &mut app.amoled_black, "AMOLED black");
            },
        );
        divider(ui);
        setting_row(
            ui,
            "Animated background",
            Some("The album art slowly drifts behind everything."),
            |ui| {
                toggle(ui, &mut app.anim_bg, "Animated background");
            },
        );
        divider(ui);
        setting_row(
            ui,
            "Pulse with the music",
            Some("The background swells with the beat."),
            |ui| {
                toggle(ui, &mut app.reactive_bg, "Pulse with the music");
            },
        );
        divider(ui);
        setting_row(
            ui,
            "Font",
            Some(
                "For the whole app and the lyrics. Any font installed on this PC works, e.g. one made for your language.",
            ),
            |ui| {
                if let Some(name) = font_picker(ui) {
                    theme::set_font_choice(&name);
                    app.chrome_applied = None;
                }
            },
        );
    });
}

/// Settings › Window.
fn settings_window(app: &mut App, ui: &mut Ui) {
    section_label(ui, "Window", None);
    settings_card(ui, |ui| {
        setting_row(
            ui,
            "Close to tray",
            Some(
                "Clicking × hides the window and keeps the music playing. Click the tray icon to bring it back, or right-click it to quit.",
            ),
            |ui| {
                toggle(ui, &mut app.close_to_tray, "Close to tray");
            },
        );
        divider(ui);
        setting_row(
            ui,
            "Collapse sidebar",
            Some(
                "Show only icons in the sidebar, for narrow windows. The menu button at its top does the same.",
            ),
            |ui| {
                toggle(ui, &mut app.sidebar_collapsed, "Collapse sidebar");
            },
        );
    });
}

fn settings_lastfm(app: &mut App, ui: &mut Ui) {
    section_label(ui, "Last.fm", None);
    settings_card(ui, |ui| {
        let busy = app.lastfm_busy.load(Ordering::Relaxed);
        let desc = match &app.lastfm_user {
            Some(name) => format!("Scrobbling what you play to {name}'s profile."),
            None if busy => {
                "Allow Marshall on the Last.fm page that opened in your browser.".to_owned()
            }
            None => "Scrobble what you play to your Last.fm profile.".to_owned(),
        };
        let (mut connect, mut disconnect) = (false, false);
        setting_row(ui, "Account", Some(&desc), |ui| {
            if app.lastfm_user.is_some() {
                disconnect = pill(ui, "Disconnect", None, false).clicked();
            } else if busy {
                ui.spinner();
            } else {
                connect = pill(ui, "Connect", None, false).clicked();
            }
        });
        if connect {
            app.lastfm_sign_in();
        }
        if disconnect {
            app.lastfm_sign_out();
        }
    });
}

fn settings_integrations(app: &mut App, ui: &mut Ui) {
    let mut discord_enabled = app.discord_enabled;
    section_label(ui, "Integrations", None);
    settings_card(ui, |ui| {
        setting_row(
            ui,
            "Discord",
            Some("Show what you're playing on your Discord profile."),
            |ui| {
                toggle(ui, &mut discord_enabled, "Discord");
            },
        );
    });
    if discord_enabled != app.discord_enabled {
        app.set_discord_enabled(discord_enabled);
    }
    super::transfer::settings_section(app, ui);
}

fn settings_library(app: &mut App, ui: &mut Ui) {
    section_label(ui, "Library", None);
    settings_card(ui, |ui| {
        ui.label(RichText::new("Your Marshall library brings playlists, liked songs, albums, listening history, uploads, and local music together.").font(theme::regular(13.0)).color(theme::text_dim()));
        ui.add_space(12.0);
        ui.horizontal_wrapped(|ui| {
            for (label, tab) in [
                ("Playlists", LibTab::Playlists),
                ("Liked songs", LibTab::Songs),
                ("Albums", LibTab::Albums),
                ("History", LibTab::History),
                ("Uploads", LibTab::Uploads),
            ] {
                if pill(ui, label, None, false).clicked() {
                    app.actions.push(Action::Go(Page::Library(tab)));
                }
            }
            if pill(ui, "Local music", Some(icon::FOLDER_OPEN), false).clicked() {
                app.actions.push(Action::Go(Page::LocalMusic));
            }
        });
        divider(ui);
        let hidden = app.hidden_playlists.len();
        let desc = match hidden {
            0 => "Nothing hidden. Right-click a playlist in the sidebar to hide it.".to_owned(),
            1 => "1 playlist is hidden from the sidebar.".to_owned(),
            n => format!(
                "{} playlists are hidden from the sidebar.",
                thousands(n as u64)
            ),
        };
        setting_row(ui, "Hidden playlists", Some(&desc), |ui| {
            if hidden > 0 && pill(ui, "Show all", None, false).clicked() {
                app.hidden_playlists.clear();
            }
        });
    });

    section_label(
        ui,
        "Smart playlists",
        Some("Update from your listening history"),
    );
    let smart = smart_playlists(app.listening_stats.entries());
    settings_card(ui, |ui| {
        for (index, (title, desc, tracks)) in smart.iter().enumerate() {
            if index > 0 {
                divider(ui);
            }
            let count = tracks.len();
            setting_row(ui, title, Some(&format!("{desc} · {count} songs")), |ui| {
                if count > 0 && pill(ui, "Play", Some(icon::PLAY), true).clicked() {
                    app.actions.push(Action::Play(tracks.clone(), 0));
                }
            });
        }
        if smart.iter().all(|(_, _, tracks)| tracks.is_empty()) {
            ui.label(
                RichText::new("Your listening history will fill these automatically.")
                    .font(theme::regular(13.0))
                    .color(theme::text_dim()),
            );
        }
        if !app.discovery_feedback.is_empty() {
            divider(ui);
            setting_row(
                ui,
                "Personalized discovery feedback",
                Some(
                    "Reset songs you marked ‘Not for me’ so they can be used as a discovery starting point again.",
                ),
                |ui| {
                    if pill(ui, "Reset feedback", None, false).clicked() {
                        app.discovery_feedback.clear();
                    }
                },
            );
        }
    });

    section_label(ui, "Backup", Some("Playlists and preferences"));
    settings_card(ui, |ui| {
        setting_row(
            ui,
            "Export or restore",
            Some(
                "Back up playlist order, hidden playlists, recent searches, discovery feedback and app preferences. Sign-in, cookies and account keys are never included.",
            ),
            |ui| {
                if pill(ui, "Export backup", None, true).clicked() {
                    export_backup(app);
                }
                if pill(ui, "Import backup", None, false).clicked() {
                    import_backup(app);
                }
            },
        );
    });
}

fn smart_playlists(entries: &[crate::listening_stats::Entry]) -> Vec<(String, String, Vec<Track>)> {
    let now = crate::listening_stats::unix_now();
    let recent = unique_recent(entries, now - 30 * 86_400);
    let week = unique_recent(entries, now - 7 * 86_400);
    let mut counts: std::collections::HashMap<String, (Track, usize)> =
        std::collections::HashMap::new();
    for entry in entries.iter().filter(|entry| {
        entry.listened_seconds
            >= entry
                .track
                .duration
                .map_or(30.0, |d| (d as f64 / 2.0).clamp(30.0, 240.0))
    }) {
        let slot = counts
            .entry(entry.track.id.clone())
            .or_insert_with(|| (entry.track.clone(), 0));
        slot.1 += 1;
    }
    let mut most: Vec<_> = counts.into_values().collect();
    most.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.title.cmp(&b.0.title)));
    let most = most.into_iter().take(50).map(|(track, _)| track).collect();
    vec![
        (
            "Recently played".to_owned(),
            "Tracks heard in the last 30 days, newest first".to_owned(),
            recent,
        ),
        (
            "Played this week".to_owned(),
            "Tracks heard in the last 7 days".to_owned(),
            week,
        ),
        (
            "Most played".to_owned(),
            "Tracks you listened to for at least half their length".to_owned(),
            most,
        ),
    ]
}

fn unique_recent(entries: &[crate::listening_stats::Entry], since: i64) -> Vec<Track> {
    let mut seen = HashSet::new();
    entries
        .iter()
        .rev()
        .filter(|entry| entry.started_at >= since && entry.listened_seconds > 0.0)
        .filter(|entry| seen.insert(entry.track.id.clone()))
        .take(100)
        .map(|entry| entry.track.clone())
        .collect()
}

#[derive(serde::Serialize, serde::Deserialize)]
struct MarshallBackup {
    format: u8,
    theme: String,
    appearance: String,
    accent_source: u8,
    custom_accent: String,
    font: String,
    haze_mode: u8,
    haze_intensity: u8,
    amoled_black: bool,
    anim_bg: bool,
    reactive_bg: bool,
    np_artwork_ambient: bool,
    close_to_tray: bool,
    autoplay: bool,
    normalize: bool,
    log_volume: bool,
    high_quality: bool,
    play_songs: bool,
    prefer_atmos: bool,
    automix: bool,
    crossfade: bool,
    crossfade_secs: u32,
    sponsorblock: bool,
    touchpad_gestures: bool,
    discord_enabled: bool,
    eq_enabled: bool,
    eq_sfx_enabled: bool,
    eq_preset: String,
    eq_profile: String,
    eq_gains: [f32; 10],
    playlist_order: Vec<String>,
    hidden_playlists: Vec<String>,
    recent_searches: Vec<super::RecentSearch>,
    discovery_feedback: Vec<String>,
    cached_playlists: Vec<Card>,
}

fn export_backup(app: &mut App) {
    let Some(path) = rfd::FileDialog::new()
        .add_filter("Marshall backup", &["json"])
        .set_file_name("marshall-backup.json")
        .save_file()
    else {
        return;
    };
    let backup = MarshallBackup {
        format: 1,
        theme: app.theme_id.clone(),
        appearance: app.appearance.key().to_owned(),
        accent_source: app.accent_source,
        custom_accent: format!(
            "#{:02X}{:02X}{:02X}",
            app.custom_accent.r(),
            app.custom_accent.g(),
            app.custom_accent.b()
        ),
        font: theme::font_choice(),
        haze_mode: app.haze_mode,
        haze_intensity: app.haze_intensity,
        amoled_black: app.amoled_black,
        anim_bg: app.anim_bg,
        reactive_bg: app.reactive_bg,
        np_artwork_ambient: app.np_artwork_ambient,
        close_to_tray: app.close_to_tray,
        autoplay: app.autoplay,
        normalize: app.normalize,
        log_volume: app.log_volume,
        high_quality: app.high_quality,
        play_songs: app.play_songs,
        prefer_atmos: app.prefer_atmos,
        automix: app.automix,
        crossfade: app.crossfade,
        crossfade_secs: app.crossfade_secs,
        sponsorblock: app.sponsorblock,
        touchpad_gestures: app.touchpad_gestures,
        discord_enabled: app.discord_enabled,
        eq_enabled: app.eq_enabled,
        eq_sfx_enabled: app.eq_sfx_enabled,
        eq_preset: app.eq_preset.clone(),
        eq_profile: app.eq_profile.clone(),
        eq_gains: app.eq_gains,
        playlist_order: app.playlist_order.clone(),
        hidden_playlists: app.hidden_playlists.iter().cloned().collect(),
        recent_searches: app.recent_searches.clone(),
        discovery_feedback: app.discovery_feedback.iter().cloned().collect(),
        cached_playlists: app
            .library
            .as_ref()
            .map(|library| library.playlists.clone())
            .or_else(|| app.sidebar_cache.clone())
            .unwrap_or_default(),
    };
    let result = serde_json::to_vec_pretty(&backup)
        .map_err(anyhow::Error::from)
        .and_then(|bytes| std::fs::write(&path, bytes).map_err(anyhow::Error::from));
    match result {
        Ok(()) => app.toast("Backup exported"),
        Err(error) => app.toast(format!("Couldn't export backup: {error}")),
    }
}

fn import_backup(app: &mut App) {
    let Some(path) = rfd::FileDialog::new()
        .add_filter("Marshall backup", &["json"])
        .pick_file()
    else {
        return;
    };
    let result = std::fs::read(&path)
        .map_err(anyhow::Error::from)
        .and_then(|bytes| {
            serde_json::from_slice::<MarshallBackup>(&bytes).map_err(anyhow::Error::from)
        });
    let backup = match result {
        Ok(backup) if backup.format == 1 => backup,
        Ok(_) => {
            app.toast("This backup version isn’t supported");
            return;
        }
        Err(error) => {
            app.toast(format!("Couldn't read backup: {error}"));
            return;
        }
    };
    app.theme_id = backup.theme;
    app.appearance = theme::Appearance::from_key(&backup.appearance);
    app.accent_source = backup.accent_source.min(3);
    app.custom_accent = crate::preset::hex(&backup.custom_accent).unwrap_or(app.custom_accent);
    theme::set_font_choice(&backup.font);
    app.haze_mode = backup.haze_mode.min(1);
    app.haze_intensity = backup.haze_intensity.min(2);
    app.amoled_black = backup.amoled_black;
    app.anim_bg = backup.anim_bg;
    app.reactive_bg = backup.reactive_bg;
    app.np_artwork_ambient = backup.np_artwork_ambient;
    app.close_to_tray = backup.close_to_tray;
    app.autoplay = backup.autoplay;
    app.normalize = backup.normalize;
    app.audio.set_normalize(app.normalize);
    app.log_volume = backup.log_volume;
    app.audio.set_log_volume(app.log_volume);
    app.high_quality = backup.high_quality;
    app.backend.set_high_quality(app.high_quality);
    app.play_songs = backup.play_songs;
    app.prefer_atmos = backup.prefer_atmos;
    app.backend.set_prefer_atmos(app.prefer_atmos);
    app.set_discord_enabled(backup.discord_enabled);
    app.automix = backup.automix;
    app.crossfade = backup.crossfade;
    app.crossfade_secs = backup.crossfade_secs.clamp(1, super::CROSSFADE_MAX);
    app.set_crossfade();
    app.sponsorblock = backup.sponsorblock;
    app.touchpad_gestures = backup.touchpad_gestures;
    app.eq_enabled = backup.eq_enabled;
    app.eq_sfx_enabled = backup.eq_sfx_enabled;
    app.eq_preset = backup.eq_preset;
    app.eq_profile = backup.eq_profile;
    app.eq_gains = backup.eq_gains.map(|gain| {
        if gain.is_finite() {
            gain.clamp(-12.0, 12.0)
        } else {
            0.0
        }
    });
    app.audio.set_eq_gains(if app.eq_enabled {
        active_eq(&app.eq_gains, &app.eq_profile, app.eq_sfx_enabled)
    } else {
        [0.0; 10]
    });
    app.playlist_order = backup.playlist_order;
    app.hidden_playlists = backup.hidden_playlists.into_iter().collect();
    app.recent_searches = backup.recent_searches;
    app.discovery_feedback = backup.discovery_feedback.into_iter().collect();
    app.sidebar_cache = (!backup.cached_playlists.is_empty()).then_some(backup.cached_playlists);
    app.toast("Backup restored");
}

fn settings_experimental(app: &mut App, ui: &mut Ui) {
    section_label(ui, "Experimental", None);
    settings_card(ui, |ui| {
        let api = crate::renderer::api_name();
        let title = format!("Use {api} renderer");
        let desc = format!(
            "Try the alternate {api} graphics renderer. If it fails to start, Marshall falls back to OpenGL."
        );
        setting_row(ui, &title, Some(&desc), |ui| {
            if toggle(ui, &mut app.wgpu_renderer, &title).changed() {
                use crate::renderer::Renderer;
                let renderer = if app.wgpu_renderer {
                    Renderer::Wgpu
                } else {
                    Renderer::OpenGl
                };
                if let Err(e) = crate::renderer::save(app.backend.data_dir(), renderer) {
                    log::warn!("renderer: couldn't save the setting: {e}");
                }
            }
        });
        if app.wgpu_renderer != (crate::renderer::active() == crate::renderer::Renderer::Wgpu) {
            divider(ui);
            let mut restart = false;
            setting_row(
                ui,
                "Restart to apply",
                Some("Marshall uses the selected renderer the next time it starts."),
                |ui| {
                    restart = pill(ui, "Restart now", None, true).clicked();
                },
            );
            if restart {
                app.restart(ui.ctx());
            }
        }
    });
}

fn settings_about(ui: &mut Ui) {
    section_label(ui, "About Marshall", None);
    settings_card(ui, |ui| {
        setting_row(
            ui,
            "Version",
            Some(concat!("Marshall ", env!("CARGO_PKG_VERSION"))),
            |_| {},
        );
        divider(ui);
        setting_row(
            ui,
            "License",
            Some(
                "GNU General Public License, version 3. The project license and notices are included with the source.",
            ),
            |_| {},
        );
        divider(ui);
        ui.label(RichText::new("Marshall is a native music player. Playback, library, lyrics, and settings continue to use Marshall's existing services and engine.").font(theme::regular(13.0)).color(theme::text_dim()));
    });
}

/// Settings › Keyboard shortcuts.
fn settings_accessibility(app: &mut App, ui: &mut Ui) {
    section_label(ui, "Accessibility", None);
    settings_card(ui, |ui| {
        setting_row(
            ui,
            "Two-finger touchpad gestures",
            Some(
                "On the Now Playing cover: swipe left for next, right for previous, up to open lyrics, and down to close lyrics. Gestures work only while the pointer is over the cover.",
            ),
            |ui| {
                toggle(
                    ui,
                    &mut app.touchpad_gestures,
                    "Two-finger touchpad gestures",
                );
            },
        );
        ui.label(RichText::new("Two-finger trackpad swipes are used; mouse-wheel scrolling is ignored. The cover stays still and the lyrics layout remains unchanged.").font(theme::regular(12.5)).color(theme::text_dim()));
    });
}

/// Settings › Keyboard shortcuts.
fn settings_shortcuts(ui: &mut Ui) {
    section_label(ui, "Keyboard shortcuts", None);
    settings_card(ui, |ui| {
        let keys: &[(&str, &[&str])] = &[
            ("Play / pause", &["Space"]),
            ("Next / previous song", &["Ctrl", "→ / ←"]),
            ("Volume up / down", &["Ctrl", "↑ / ↓"]),
            ("Search", &["Ctrl", "F"]),
            ("Lyrics", &["L"]),
            ("Queue", &["Q"]),
            ("Full-screen lyrics (in now playing)", &["F"]),
            ("Back / forward", &["Alt", "← / →"]),
            ("Close now playing", &["Esc"]),
        ];
        for (i, (what, combo)) in keys.iter().enumerate() {
            if i > 0 {
                divider(ui);
            }
            let (rect, response) =
                ui.allocate_exact_size(vec2(ui.available_width(), 44.0), Sense::hover());
            let spoken = format!("{what}: {}", combo.join(" plus "));
            response
                .widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, spoken.clone()));
            ui.painter().text(
                pos2(rect.left(), rect.center().y),
                Align2::LEFT_CENTER,
                *what,
                theme::regular(14.0),
                theme::text(),
            );
            let mut x = rect.right();
            for (j, k) in combo.iter().enumerate().rev() {
                x = keycap(ui, pos2(x, rect.center().y), k) - 6.0;
                if j > 0 {
                    let plus = ui.painter().text(
                        pos2(x, rect.center().y),
                        Align2::RIGHT_CENTER,
                        "+",
                        theme::regular(13.0),
                        theme::text_dim(),
                    );
                    x = plus.left() - 6.0;
                }
            }
        }
    });
}

/// Settings › Network: the proxy everything goes through, a connection test, and a
/// restart when the setting changed.
fn network(app: &mut App, ui: &mut Ui) {
    use crate::proxy::{self, Setting};
    let same = |a: &Setting, b: &Setting| std::mem::discriminant(a) == std::mem::discriminant(b);
    let mut pick = None;
    setting_row(
        ui,
        "Proxy",
        Some(
            "Send everything Marshall fetches through a proxy: songs, lyrics, sign-in and Listen together.",
        ),
        |ui| {
            let btn = dropdown_button(ui, app.proxy_mode.label(), 220.0, "Proxy");
            egui::Popup::menu(&btn).show(|ui| {
                ui.set_width(220.0);
                ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                ui.add_space(4.0);
                for s in [
                    Setting::System,
                    Setting::Off,
                    Setting::Custom(String::new()),
                ] {
                    if option_row(ui, s.label(), same(&s, &app.proxy_mode)).clicked() {
                        pick = Some(s);
                        ui.close();
                    }
                }
                ui.add_space(4.0);
            });
        },
    );
    if let Some(s) = pick {
        app.proxy_error = None;
        *app.proxy_test.lock().unwrap() = None;
        app.proxy_mode = s.clone();
        // System and Off take effect as picked; a custom address once it's saved.
        if !matches!(s, Setting::Custom(_)) {
            match proxy::save(app.backend.data_dir(), &s) {
                Ok(()) => app.proxy_saved = s,
                Err(e) => app.proxy_error = Some(format!("{e:#}")),
            }
        }
    }
    match &app.proxy_mode {
        Setting::System => {
            divider(ui);
            let desc = match proxy::system() {
                Some(p) => format!("Windows is set to use {}.", proxy::redact(&p)),
                None => {
                    "Windows isn't set to use a proxy, so Marshall connects directly.".to_owned()
                }
            };
            setting_row(ui, "Windows proxy", Some(&desc), |_| {});
        }
        Setting::Custom(_) => {
            divider(ui);
            ui.add_space(12.0);
            super::text_field(
                ui,
                "Address",
                &mut app.proxy_url,
                false,
                "http://127.0.0.1:8080 or socks5://user:pass@host:1080",
            );
            ui.add_space(10.0);
            let custom = Setting::Custom(app.proxy_url.trim().to_owned());
            let saved = app.proxy_saved == custom;
            let mut save = false;
            ui.horizontal(|ui| {
                save = pill(ui, if saved { "Saved" } else { "Save" }, None, !saved).clicked()
                    && !saved;
                ui.add_space(8.0);
                ui.label(
                    RichText::new(
                        "A password in the address is kept in Windows Credential Manager.",
                    )
                    .font(theme::regular(12.5))
                    .color(theme::text_dim()),
                );
            });
            if save {
                match proxy::save(app.backend.data_dir(), &custom) {
                    Ok(()) => {
                        app.proxy_saved = custom;
                        app.proxy_error = None;
                    }
                    Err(e) => app.proxy_error = Some(format!("{e:#}")),
                }
            }
            ui.add_space(12.0);
        }
        Setting::Off => {}
    }
    if let Some(e) = &app.proxy_error {
        ui.label(
            RichText::new(e)
                .font(theme::regular(12.5))
                .color(theme::RED),
        );
        ui.add_space(8.0);
    }
    divider(ui);
    let result = app.proxy_test.lock().unwrap().clone();
    let desc = match &result {
        Some(Some((_, msg))) => msg.clone(),
        Some(None) => "Checking…".to_owned(),
        None => "Checks that YouTube Music answers through this setting.".to_owned(),
    };
    setting_row(ui, "Test connection", Some(&desc), |ui| {
        if matches!(result, Some(None)) {
            ui.spinner();
        } else if pill(ui, "Test", None, false).clicked() {
            let s = match &app.proxy_mode {
                Setting::Custom(_) => Setting::Custom(app.proxy_url.trim().to_owned()),
                other => other.clone(),
            };
            let (slot, b) = (app.proxy_test.clone(), app.backend.clone());
            *slot.lock().unwrap() = Some(None);
            app.backend.spawn(async move {
                let out = match proxy::test(&s).await {
                    Ok(t) => (
                        true,
                        format!("Working: YouTube Music answered in {} ms.", t.as_millis()),
                    ),
                    Err(e) => (false, format!("Didn't connect: {e:#}")),
                };
                *slot.lock().unwrap() = Some(Some(out));
                b.wake();
            });
        }
    });
    if app.proxy_saved != *proxy::active() {
        divider(ui);
        let mut restart = false;
        setting_row(
            ui,
            "Restart to apply",
            Some("The new proxy setting is used from the next start."),
            |ui| {
                restart = pill(ui, "Restart now", None, true).clicked();
            },
        );
        if restart {
            app.restart(ui.ctx());
        }
    }
}

/// Paint `v` into `rect`, from the ICO entry that fits its size in physical pixels.
pub(super) fn paint_app_icon(ui: &Ui, v: &crate::appicon::Variant, rect: Rect) {
    let px = (rect.width() * ui.ctx().pixels_per_point()).ceil() as u32;
    let png = v.png(px);
    egui::Image::from_bytes(format!("bytes://app-icon/{}-{}.png", v.id, png.len()), png)
        .paint_at(ui, rect);
}

/// The font setting: the theme's own, a bundled face, or one installed on this PC.
/// Returns the new choice ("" for the theme's).
fn font_picker(ui: &mut Ui) -> Option<String> {
    let current = theme::font_choice();
    let btn = dropdown_button(
        ui,
        if current.is_empty() {
            "Theme's font"
        } else {
            &current
        },
        220.0,
        "Font",
    );
    let just_opened = btn.clicked();
    let mut picked = None;
    egui::Popup::menu(&btn).show(|ui| {
        ui.set_width(220.0);
        ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
        egui::ScrollArea::vertical()
            .max_height(360.0)
            .show(ui, |ui| {
                ui.add_space(4.0);
                let mut row = |ui: &mut Ui, label: &str, value: &str| {
                    let on = current.eq_ignore_ascii_case(value);
                    let r = option_row(ui, label, on);
                    if on && just_opened {
                        r.scroll_to_me(Some(egui::Align::Center));
                    }
                    if r.clicked() {
                        picked = Some(value.to_owned());
                        ui.close();
                    }
                };
                row(ui, "Theme's font", "");
                for name in theme::BUNDLED_FONTS {
                    row(ui, name, name);
                }
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.add_space(12.0);
                    ui.label(
                        RichText::new("Installed on this PC")
                            .font(theme::semibold(12.0))
                            .color(theme::text_faint()),
                    );
                });
                ui.add_space(4.0);
                let ctx = ui.ctx().clone();
                match theme::installed_fonts(move || ctx.request_repaint()) {
                    Some(fonts) => {
                        for f in fonts {
                            row(ui, &f.name, &f.name);
                        }
                    }
                    None => {
                        ui.horizontal(|ui| {
                            ui.add_space(12.0);
                            ui.label(
                                RichText::new("Looking for fonts…")
                                    .font(theme::regular(13.0))
                                    .color(theme::text_dim()),
                            );
                        });
                    }
                }
                ui.add_space(4.0);
            });
    });
    picked.filter(|p| *p != current)
}

const SYSTEM_DEFAULT: &str = "Windows default";

fn output_device_picker(app: &mut App, ui: &mut Ui) {
    let current = match &app.output_device {
        None => SYSTEM_DEFAULT.to_owned(),
        Some((id, name)) => match app.output_devices.iter().find(|d| d.id == *id) {
            Some(d) => d.name.clone(),
            None if app.output_devices.is_empty() => name.clone(),
            None => format!("{name} (not connected)"),
        },
    };
    let btn = dropdown_button(ui, &current, 210.0, "Output device");
    // Listed when opened, so a device that just connected shows up.
    if btn.clicked() || app.output_devices.is_empty() {
        app.output_devices = crate::audio::output_devices();
    }
    let mut pick: Option<Option<(String, String)>> = None;
    egui::Popup::menu(&btn).show(|ui| {
        ui.set_width(340.0);
        ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
        egui::ScrollArea::vertical()
            .max_height(320.0)
            .show(ui, |ui| {
                ui.add_space(4.0);
                if option_row(ui, SYSTEM_DEFAULT, app.output_device.is_none()).clicked() {
                    pick = Some(None);
                    ui.close();
                }
                for d in &app.output_devices {
                    if option_row(
                        ui,
                        &d.name,
                        app.output_device
                            .as_ref()
                            .is_some_and(|(id, _)| *id == d.id),
                    )
                    .clicked()
                    {
                        pick = Some(Some((d.id.clone(), d.name.clone())));
                        ui.close();
                    }
                }
                ui.add_space(4.0);
            });
    });
    if let Some(pick) = pick {
        app.audio.send(crate::audio::AudioCmd::SetDevice(
            pick.as_ref().map(|(id, _)| id.clone()),
        ));
        app.output_device = pick;
    }
}

/// A dropdown's closed state: the chosen value and a chevron on a glass pill.
fn dropdown_button(ui: &mut Ui, value: &str, width: f32, name: &str) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(width, 36.0), Sense::click());
    let resp = widgets::named(resp, name).on_hover_cursor(egui::CursorIcon::PointingHand);
    let open = egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&resp));
    let h = theme::anim_bool(ui.ctx(), resp.id.with("h"), resp.hovered() || open, 0.12);
    ui.painter()
        .rect_filled(rect, 10.0, theme::glass(0.08 + 0.05 * h));
    widgets::text_at(
        ui,
        pos2(rect.left() + 14.0, rect.center().y - 9.0),
        value,
        theme::regular(14.0),
        theme::text(),
        rect.width() - 44.0,
    );
    let glyph = if open {
        theme::icon::CHEVRON_UP
    } else {
        theme::icon::CHEVRON_DOWN
    };
    widgets::icon_at(
        ui,
        pos2(rect.right() - 18.0, rect.center().y),
        glyph,
        15.0,
        theme::lerp_color(theme::text_dim(), theme::text(), h),
    );
    resp
}

/// A dropdown option: hover glass, and a check on the chosen one.
fn option_row(ui: &mut Ui, label: &str, selected: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 34.0), Sense::click());
    let resp = widgets::named(resp, label).on_hover_cursor(egui::CursorIcon::PointingHand);
    let h = theme::anim_bool(ui.ctx(), resp.id.with("h"), resp.hovered(), 0.12);
    if h > 0.0 || selected {
        ui.painter().rect_filled(
            rect,
            8.0,
            theme::glass(0.05 * selected as u8 as f32 + 0.08 * h),
        );
    }
    let font = if selected {
        theme::semibold(14.0)
    } else {
        theme::regular(14.0)
    };
    widgets::text_at(
        ui,
        pos2(rect.left() + 12.0, rect.center().y - 9.0),
        label,
        font,
        theme::text(),
        rect.width() - 44.0,
    );
    if selected {
        widgets::icon_at(
            ui,
            pos2(rect.right() - 16.0, rect.center().y),
            theme::icon::CHECK,
            15.0,
            theme::text(),
        );
    }
    resp
}

/// 12345 -> "12,345".
pub(super) fn thousands(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// Small heading above a settings card, with an optional value on the right.
pub(super) fn section_label(ui: &mut Ui, title: &str, value: Option<&str>) {
    ui.add_space(30.0);
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 20.0), Sense::hover());
    ui.painter().text(
        pos2(rect.left() + 2.0, rect.center().y),
        Align2::LEFT_CENTER,
        title,
        theme::bold(17.0),
        theme::text(),
    );
    if let Some(v) = value {
        ui.painter().text(
            pos2(rect.right() - 2.0, rect.center().y),
            Align2::RIGHT_CENTER,
            v,
            theme::semibold(13.0),
            theme::text_dim(),
        );
    }
    ui.add_space(10.0);
}

pub(super) fn settings_card(ui: &mut Ui, body: impl FnOnce(&mut Ui)) {
    egui::Frame::new()
        .fill(theme::glass(0.06))
        .stroke(egui::Stroke::new(1.0, theme::glass(0.05)))
        .corner_radius(theme::panel_radius())
        .inner_margin(egui::Margin::symmetric(20, 6))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            body(ui);
        });
}

/// A settings row: title and description on the left, the control on the right.
pub(super) fn setting_row(
    ui: &mut Ui,
    title: &str,
    desc: Option<&str>,
    control: impl FnOnce(&mut Ui),
) {
    let w = ui.available_width();
    let text_w = (w - 230.0).max(200.0);
    let title_g = widgets::galley(ui, title, theme::semibold(14.5), theme::text(), text_w);
    let desc_g = desc.map(|d| {
        widgets::galley_wrapped(ui, d, theme::regular(12.5), theme::text_dim(), text_w, 3)
    });
    let text_h = title_g.size().y + desc_g.as_ref().map_or(0.0, |g| g.size().y + 3.0);
    let h = (text_h + 26.0).max(58.0);
    let (rect, _) = ui.allocate_exact_size(vec2(w, h), Sense::hover());
    let fade = if ui.is_enabled() { 1.0 } else { 0.45 };
    let top = rect.center().y - text_h / 2.0;
    ui.painter().galley(
        pos2(rect.left(), top),
        title_g.clone(),
        theme::with_alpha(theme::text(), fade),
    );
    if let Some(g) = desc_g {
        ui.painter().galley(
            pos2(rect.left(), top + title_g.size().y + 3.0),
            g,
            theme::with_alpha(theme::text_dim(), fade),
        );
    }
    let ctl = Rect::from_min_max(pos2(rect.left() + text_w + 20.0, rect.top()), rect.max);
    let mut cui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(ctl)
            .layout(egui::Layout::right_to_left(egui::Align::Center)),
    );
    control(&mut cui);
}

pub(super) fn divider(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter().rect_filled(rect, 0.0, theme::glass(0.07));
}

/// On/off switch tinted with the accent.
pub(super) fn toggle(ui: &mut Ui, on: &mut bool, name: &str) -> egui::Response {
    let (rect, mut resp) = ui.allocate_exact_size(vec2(44.0, 26.0), Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    let value = *on;
    resp.widget_info(|| {
        egui::WidgetInfo::selected(egui::Role::CheckBox, ui.is_enabled(), value, name)
    });
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    let t = theme::anim_bool(ui.ctx(), resp.id.with("t"), value, 0.16);
    let h = theme::anim_bool(ui.ctx(), resp.id.with("h"), resp.hovered(), 0.12);
    let accent = ui
        .ctx()
        .data(|d| d.get_temp::<Color32>(egui::Id::unique("accent")))
        .unwrap_or(theme::RED);
    let fade = if ui.is_enabled() { 1.0 } else { 0.4 };
    let off = theme::glass(0.16 + 0.06 * h);
    ui.painter().rect_filled(
        rect,
        13.0,
        theme::with_alpha(theme::lerp_color(off, accent, t), fade),
    );
    let knob = pos2(
        egui::lerp(rect.left() + 13.0..=rect.right() - 13.0, t),
        rect.center().y,
    );
    ui.painter()
        .circle_filled(knob + vec2(0.0, 1.0), 10.0, theme::shade(0.25 * fade));
    ui.painter()
        .circle_filled(knob, 10.0, theme::with_alpha(Color32::WHITE, fade));
    resp
}

/// Whole seconds on a track like the volume slider's, with the ends labelled and the
/// value beside it: click or drag to set (scrolling over it scrolls the page).
fn seconds_slider(
    ui: &mut Ui,
    v: &mut u32,
    range: std::ops::RangeInclusive<u32>,
    name: &str,
) -> egui::Response {
    let (lo, hi) = (*range.start(), *range.end());
    let label = |ui: &mut Ui, text: String, w: f32, font: egui::FontId, color: Color32| {
        let (rect, _) = ui.allocate_exact_size(vec2(w, 26.0), Sense::hover());
        let g = widgets::galley(ui, &text, font, color, w);
        ui.painter().galley(
            pos2(
                rect.right() - g.size().x,
                rect.center().y - g.size().y / 2.0,
            ),
            g,
            color,
        );
    };
    label(
        ui,
        format!("{v} s"),
        34.0,
        theme::semibold(13.0),
        theme::text(),
    );
    ui.add_space(6.0);
    label(
        ui,
        format!("{hi} s"),
        26.0,
        theme::regular(12.0),
        theme::text_dim(),
    );
    let (rect, mut resp) = ui.allocate_exact_size(vec2(170.0, 26.0), Sense::click_and_drag());
    label(
        ui,
        format!("{lo} s"),
        22.0,
        theme::regular(12.0),
        theme::text_dim(),
    );
    let track = Rect::from_center_size(rect.center(), vec2(rect.width() - 16.0, 4.0));
    let old = *v;
    if let Some(p) = resp.interact_pointer_pos()
        && (resp.dragged() || resp.clicked())
    {
        let t = ((p.x - track.left()) / track.width()).clamp(0.0, 1.0);
        *v = (lo as f32 + t * (hi - lo) as f32).round() as u32;
    }
    if *v != old {
        resp.mark_changed();
    }
    let value = *v;
    resp.widget_info(|| egui::WidgetInfo::slider(ui.is_enabled(), value as f64, name));
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    let h = theme::anim_bool(
        ui.ctx(),
        resp.id.with("h"),
        resp.hovered() || resp.dragged(),
        0.12,
    );
    let accent = ui
        .ctx()
        .data(|d| d.get_temp::<Color32>(egui::Id::unique("accent")))
        .unwrap_or(theme::RED);
    let t = (value - lo) as f32 / (hi - lo) as f32;
    let knob = pos2(
        egui::lerp(track.left()..=track.right(), t),
        track.center().y,
    );
    ui.painter().rect_filled(track, 2.0, theme::glass(0.15));
    ui.painter().rect_filled(
        Rect::from_min_max(track.min, pos2(knob.x, track.max.y)),
        2.0,
        accent,
    );
    ui.painter()
        .circle_filled(knob + vec2(0.0, 1.0), 7.0 + h, theme::shade(0.25));
    ui.painter().circle_filled(knob, 7.0 + h, Color32::WHITE);
    resp
}

/// Draw a key cap whose right edge is at `right`; returns its left edge.
fn keycap(ui: &Ui, right: egui::Pos2, key: &str) -> f32 {
    let g = widgets::galley(ui, key, theme::semibold(12.5), theme::text(), 200.0);
    let size = vec2((g.size().x + 18.0).max(30.0), 26.0);
    let rect = Rect::from_min_size(pos2(right.x - size.x, right.y - size.y / 2.0), size);
    ui.painter().rect_filled(rect, 7.0, theme::glass(0.08));
    ui.painter().rect_stroke(
        rect,
        7.0,
        egui::Stroke::new(1.0, theme::glass(0.12)),
        egui::StrokeKind::Inside,
    );
    ui.painter().rect_filled(
        Rect::from_min_max(
            pos2(rect.left() + 2.0, rect.bottom() - 2.0),
            pos2(rect.right() - 2.0, rect.bottom()),
        ),
        2.0,
        theme::shade(0.25),
    );
    ui.painter()
        .galley(rect.center() - g.size() / 2.0, g, theme::text());
    rect.left()
}
