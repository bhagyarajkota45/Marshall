//! Track Canvas lookup used by the Now Playing cover.
//!
//! The Windows player already owns a native motion decoder. This module ports
//! Android Marshall's source order and verifies catalogue matches before use.

use std::path::Path;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use reqwest::Client;
use serde_json::Value;
use unicode_normalization::{UnicodeNormalization, char::is_combining_mark};

const TIDAL_SEARCH: &str = "https://api.tidal.com/v1/search";
const TIDAL_EMBED_TOKEN: &str = "vNVdglQOjFJJGG2U";
const COMMUNITY_MANIFEST: &str = "https://vivimusicanvas.mkmdevilmi.workers.dev/canvas.json";
const COMMUNITY_TTL: Duration = Duration::from_secs(30 * 60);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(8);
const CANVAS_UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36";

#[derive(Clone)]
struct CommunityEntry {
    title: String,
    artist: String,
    album: String,
    url: String,
}

#[derive(Default)]
struct CommunityCache {
    checked_at: Option<Instant>,
    entries: Vec<CommunityEntry>,
}

static COMMUNITY: LazyLock<Mutex<CommunityCache>> =
    LazyLock::new(|| Mutex::new(CommunityCache::default()));

/// Resolves a track's motion cover in Android Marshall's default provider order.
pub async fn motion_for_track(
    http: &Client,
    title: &str,
    artist: &str,
    album: Option<&str>,
    data_dir: &Path,
) -> Option<String> {
    if title.trim().is_empty() || artist.trim().is_empty() {
        return None;
    }

    if let Some(url) = crate::apple::song_motion(http, title, artist, album)
        .await
        .and_then(|motion| motion.square)
    {
        log::debug!("Canvas: Apple motion found for '{title}' by '{artist}'");
        return Some(url);
    }
    log::debug!("Canvas: Apple found no clip for '{title}' by '{artist}'");
    if let Some(url) = tidal_track(http, title, artist, album).await {
        log::debug!("Canvas: Tidal cover found for '{title}' by '{artist}'");
        return Some(url);
    }
    log::debug!("Canvas: Tidal found no clip for '{title}' by '{artist}'");
    if let Some(url) = community_track(http, title, artist, album).await {
        log::debug!("Canvas: community cover found for '{title}' by '{artist}'");
        return Some(url);
    }
    log::debug!("Canvas: community index found no clip for '{title}' by '{artist}'");
    if let Some(cookie) =
        crate::vault::get("spotify sp_dc", data_dir).and_then(|bytes| String::from_utf8(bytes).ok())
    {
        if let Ok(client) = crate::transfer::spotify::Client::connect(Some(&cookie), data_dir).await
        {
            if let Ok(Some(url)) = client.canvas_url_for(title, artist, album).await {
                log::debug!("Canvas: Spotify clip found for '{title}' by '{artist}'");
                return Some(url);
            }
        }
    }
    log::debug!("Canvas: no provider found a clip for '{title}' by '{artist}'");
    None
}

async fn tidal_track(
    http: &Client,
    title: &str,
    artist: &str,
    album: Option<&str>,
) -> Option<String> {
    let query = match album.filter(|a| !a.trim().is_empty()) {
        Some(album) => format!("{album} {artist} {title}"),
        None => format!("{artist} {title}"),
    };
    let country = country_code();
    let root: Value = http
        .get(TIDAL_SEARCH)
        .query(&[
            ("query", query.as_str()),
            ("limit", "10"),
            ("types", "TRACKS"),
            ("countryCode", country.as_str()),
        ])
        .header("X-Tidal-Token", TIDAL_EMBED_TOKEN)
        .header("User-Agent", CANVAS_UA)
        .timeout(REQUEST_TIMEOUT)
        .send()
        .await
        .ok()?
        .error_for_status()
        .ok()?
        .json()
        .await
        .ok()?;
    let items = root.get("tracks")?.get("items")?.as_array()?;
    for track in items {
        let Some(got_title) = track.get("title").and_then(Value::as_str) else {
            continue;
        };
        let artists = track
            .get("artists")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|item| item.get("name").and_then(Value::as_str))
            .map(str::to_owned)
            .collect::<Vec<_>>();
        let album_obj = track.get("album");
        let got_album = album_obj
            .and_then(|value| value.get("title"))
            .and_then(Value::as_str);
        if !matches_song(got_title, &artists, got_album, title, artist, album) {
            continue;
        }
        let Some(id) = album_obj
            .and_then(|value| value.get("videoCover"))
            .and_then(Value::as_str)
        else {
            continue;
        };
        if let Some(url) = tidal_cover_url(id) {
            return Some(url);
        }
    }
    None
}

fn country_code() -> String {
    std::env::var("LC_ALL")
        .or_else(|_| std::env::var("LANG"))
        .ok()
        .and_then(|locale| locale.split('_').nth(1).map(str::to_owned))
        .and_then(|country| country.get(..2).map(str::to_ascii_uppercase))
        .filter(|country| country.len() == 2 && country.bytes().all(|b| b.is_ascii_alphabetic()))
        .unwrap_or_else(|| "US".to_owned())
}

fn tidal_cover_url(id: &str) -> Option<String> {
    let parts = id.split('-').collect::<Vec<_>>();
    (parts.len() == 5 && parts.iter().all(|p| !p.is_empty())).then(|| {
        format!(
            "https://resources.tidal.com/videos/{}/1280x1280.mp4",
            parts.join("/")
        )
    })
}

async fn community_track(
    http: &Client,
    title: &str,
    artist: &str,
    album: Option<&str>,
) -> Option<String> {
    let entries = community_manifest(http).await;
    entries
        .iter()
        .find(|entry| {
            matches_song(
                &entry.title,
                std::slice::from_ref(&entry.artist),
                Some(&entry.album),
                title,
                artist,
                album,
            )
        })
        .map(|entry| entry.url.clone())
}

async fn community_manifest(http: &Client) -> Vec<CommunityEntry> {
    if let Ok(cache) = COMMUNITY.lock() {
        if cache
            .checked_at
            .is_some_and(|at| at.elapsed() < COMMUNITY_TTL)
        {
            return cache.entries.clone();
        }
    }
    let now = Instant::now();
    let response = http
        .get(COMMUNITY_MANIFEST)
        .header("User-Agent", CANVAS_UA)
        .timeout(REQUEST_TIMEOUT)
        .send()
        .await
        .ok()
        .and_then(|response| response.error_for_status().ok());
    let parsed = if let Some(response) = response {
        response
            .json::<Value>()
            .await
            .ok()
            .and_then(|root| root.get("items").and_then(Value::as_array).cloned())
            .unwrap_or_default()
            .into_iter()
            .filter_map(|item| {
                let title = item.get("song")?.as_str()?.trim();
                let artist = item.get("artist")?.as_str()?.trim();
                let album = item
                    .get("album")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .trim();
                let url = item.get("url")?.as_str()?.trim();
                let playable_url = url
                    .split(['?', '#'])
                    .next()?
                    .to_ascii_lowercase()
                    .ends_with(".mp4")
                    || url
                        .split(['?', '#'])
                        .next()?
                        .to_ascii_lowercase()
                        .ends_with(".m3u8");
                if title.is_empty()
                    || artist.is_empty()
                    || !url.starts_with("https://")
                    || !playable_url
                {
                    return None;
                }
                Some(CommunityEntry {
                    title: title.to_owned(),
                    artist: artist.to_owned(),
                    album: album.to_owned(),
                    url: url.to_owned(),
                })
            })
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    if let Ok(mut cache) = COMMUNITY.lock() {
        if !parsed.is_empty() {
            cache.entries = parsed;
        }
        cache.checked_at = Some(now);
        return cache.entries.clone();
    }
    Vec::new()
}

/// Provider-independent post-check, matching Android CanvasArtwork.matches.
pub(crate) fn matches_song(
    got_title: &str,
    got_artists: &[String],
    got_album: Option<&str>,
    want_title: &str,
    want_artist: &str,
    want_album: Option<&str>,
) -> bool {
    let title_ok = got_title.trim().is_empty()
        || want_title.trim().is_empty()
        || normalize_for_match(got_title) == normalize_for_match(want_title);
    let wanted = split_artists(want_artist);
    let credited = got_artists
        .iter()
        .flat_map(|artist| split_artists(artist))
        .collect::<Vec<_>>();
    let artist_ok = !wanted.is_empty()
        && !credited.is_empty()
        && wanted
            .iter()
            .all(|want| credited.iter().any(|got| got == want));
    let album_ok = match (
        got_album.filter(|album| !album.trim().is_empty()),
        want_album.filter(|album| !album.trim().is_empty()),
    ) {
        (Some(got), Some(want)) => normalize_for_match(got) == normalize_for_match(want),
        _ => true,
    };
    title_ok && artist_ok && album_ok
}

fn normalize_for_match(value: &str) -> String {
    value
        .nfd()
        .filter(|ch| !is_combining_mark(*ch))
        .flat_map(char::to_lowercase)
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch.is_ascii_whitespace() {
                ch
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn split_artists(value: &str) -> Vec<String> {
    let mut out = Vec::new();
    for group in value.split([',', ';', '&', '×']) {
        let mut parts = Vec::new();
        let normalized = normalize_for_match(group);
        for word in normalized.split_whitespace() {
            if matches!(word, "feat" | "ft" | "featuring" | "with" | "x") {
                if !parts.is_empty() {
                    out.push(parts.join(" "));
                    parts.clear();
                }
            } else {
                parts.push(word);
            }
        }
        if !parts.is_empty() {
            out.push(parts.join(" "));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{matches_song, normalize_for_match, split_artists, tidal_cover_url};

    #[test]
    fn normalization_folds_accents_and_punctuation() {
        assert_eq!(
            normalize_for_match("Beyoncé — CRAZY IN LOVE (feat. JAY-Z)"),
            "beyonce crazy in love feat jay z"
        );
    }

    #[test]
    fn artist_credits_match_individually() {
        let credits = vec!["Beyonce".to_owned(), "Jay-Z".to_owned()];
        assert!(matches_song(
            "Crazy in Love",
            &credits,
            Some("Dangerously in Love"),
            "Crazy In Love",
            "Beyoncé feat. Jay-Z",
            Some("Dangerously In Love"),
        ));
        assert!(!matches_song(
            "Crazy in Love",
            &credits,
            None,
            "Crazy In Love",
            "Beyoncé feat. Someone Else",
            None,
        ));
        assert_eq!(
            split_artists("A, B feat. C"),
            vec!["a".to_owned(), "b".to_owned(), "c".to_owned()]
        );
    }

    #[test]
    fn tidal_cover_id_must_have_five_segments() {
        assert_eq!(
            tidal_cover_url("a-b-c-d-e").as_deref(),
            Some("https://resources.tidal.com/videos/a/b/c/d/e/1280x1280.mp4")
        );
        assert!(tidal_cover_url("bad-id").is_none());
    }
}
