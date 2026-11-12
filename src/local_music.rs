//! Local MP3/FLAC discovery and embedded metadata/artwork extraction.

use std::path::{Path, PathBuf};

use crate::model::{Link, Thumb, Track};
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::{MediaSourceStream, MediaSourceStreamOptions};
use symphonia::core::meta::{MetadataOptions, StandardTagKey};
use symphonia::core::probe::Hint;

const MIN_ARTWORK_SIDE: u32 = 512;

pub fn scan(folder: &Path, data_dir: &Path) -> Vec<Track> {
    let mut paths = Vec::new();
    let mut pending = vec![folder.to_path_buf()];
    while let Some(dir) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            let path = entry.path();
            if kind.is_dir() {
                pending.push(path);
            } else if kind.is_file() && supported_file(&path) {
                paths.push(path);
            }
        }
    }
    paths.sort_by_key(|p| p.to_string_lossy().to_ascii_lowercase());
    paths
        .into_iter()
        .filter_map(|p| read_track(&p, data_dir))
        .collect()
}

fn supported_file(path: &Path) -> bool {
    path.extension()
        .and_then(|x| x.to_str())
        .is_some_and(|x| matches!(x.to_ascii_lowercase().as_str(), "mp3" | "flac"))
}

fn read_track(path: &Path, data_dir: &Path) -> Option<Track> {
    let file = std::fs::File::open(path).ok()?;
    let mut hint = Hint::new();
    hint.with_extension(path.extension()?.to_str()?);
    let stream = MediaSourceStream::new(Box::new(file), MediaSourceStreamOptions::default());
    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            stream,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .ok()?;
    let mut format = probed.format;
    let primary = format.metadata().current().cloned();
    let mut title = None;
    let mut artist = None;
    let mut album = None;
    let mut cover: Option<Vec<u8>> = None;
    if let Some(meta) = primary {
        for tag in meta.tags() {
            let key = tag.std_key;
            let val = tag.value.to_string();
            match key {
                Some(StandardTagKey::TrackTitle) if title.is_none() => title = Some(val),
                Some(StandardTagKey::Artist | StandardTagKey::AlbumArtist) if artist.is_none() => {
                    artist = Some(val)
                }
                Some(StandardTagKey::Album) if album.is_none() => album = Some(val),
                _ => {}
            }
        }
        cover = meta
            .visuals()
            .iter()
            .max_by_key(|v| v.data.len())
            .map(|v| v.data.to_vec());
    }
    let title = title.filter(|s| !s.trim().is_empty()).unwrap_or_else(|| {
        path.file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned()
    });
    let artist = artist.filter(|s| !s.trim().is_empty());
    let album = album.filter(|s| !s.trim().is_empty());
    let track_info = format.default_track();
    let duration = track_info.and_then(|t| {
        Some(
            t.codec_params
                .time_base?
                .calc_time(t.codec_params.n_frames?)
                .seconds
                .min(u32::MAX as u64) as u32,
        )
    });
    let embedded = cover.and_then(|bytes| cache_art(path, data_dir, &bytes));
    let key = artwork_key_parts(
        &title,
        artist.as_deref().unwrap_or_default(),
        album.as_deref(),
    );
    let thumb = if embedded
        .as_ref()
        .is_none_or(|t| t.width.min(t.height) < MIN_ARTWORK_SIDE)
    {
        key.as_deref()
            .and_then(|key| cached_remote_art(data_dir, key))
            .or(embedded)
    } else {
        embedded
    };
    let artists = artist
        .map(|name| vec![Link { name, id: None }])
        .unwrap_or_default();
    let album = album.map(|name| Link { name, id: None });
    Some(Track {
        id: format!("local:{}", path.to_string_lossy()),
        title,
        artists,
        album,
        duration,
        thumbs: thumb.into_iter().collect(),
        track_nr: None,
        plays: None,
        music_video: false,
    })
}

fn cache_art(path: &Path, data_dir: &Path, bytes: &[u8]) -> Option<Thumb> {
    let decoded = image::load_from_memory(bytes).ok()?;
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    use std::hash::{Hash, Hasher};
    path.hash(&mut hash);
    std::fs::metadata(path)
        .ok()?
        .modified()
        .ok()?
        .hash(&mut hash);
    let key = format!("{:016x}", hash.finish());
    let dir = data_dir.join("local-art");
    std::fs::create_dir_all(&dir).ok()?;
    let art = dir.join(format!("{key}.png"));
    if !art.exists() {
        decoded.save(&art).ok()?;
    }
    Some(Thumb {
        url: format!("marshall-file://{}", art.to_string_lossy()),
        width: decoded.width(),
        height: decoded.height(),
    })
}

/// Whether a local track's cover is absent or too small for crisp app artwork.
pub fn needs_better_art(track: &Track) -> bool {
    track
        .thumbs
        .first()
        .is_none_or(|t| t.width.min(t.height) < MIN_ARTWORK_SIDE)
}

/// Stable key shared by tracks from the same album and artist.
pub fn artwork_key(track: &Track) -> Option<String> {
    let artist = track.artist_line();
    let album = track.album.as_ref().map(|a| a.name.as_str());
    artwork_key_parts(&track.title, &artist, album)
}

fn artwork_key_parts(title: &str, artist: &str, album: Option<&str>) -> Option<String> {
    let artist = artwork_fold(artist);
    let release = artwork_fold(album.filter(|a| !a.trim().is_empty()).unwrap_or(title));
    (!artist.is_empty() && !release.is_empty()).then(|| format!("{artist}:{release}"))
}

fn artwork_fold(text: &str) -> String {
    text.to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect()
}

fn remote_art_path(data_dir: &Path, key: &str) -> PathBuf {
    use std::hash::{Hash, Hasher};
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    key.hash(&mut hash);
    data_dir
        .join("local-art-online")
        .join(format!("{:016x}.png", hash.finish()))
}

/// Load a previously cached higher-resolution catalogue cover for a rescan.
pub fn cached_remote_art(data_dir: &Path, key: &str) -> Option<Thumb> {
    let art = remote_art_path(data_dir, key);
    let (width, height) = image::image_dimensions(&art).ok()?;
    (width.min(height) >= MIN_ARTWORK_SIDE).then(|| Thumb {
        url: format!("marshall-file://{}", art.to_string_lossy()),
        width,
        height,
    })
}

/// Cache a matching catalogue cover locally so future scans work offline too.
pub fn cache_remote_art(data_dir: &Path, key: &str, bytes: &[u8]) -> Option<Thumb> {
    if let Some(cached) = cached_remote_art(data_dir, key) {
        return Some(cached);
    }
    let decoded = image::load_from_memory(bytes).ok()?;
    let (width, height) = (decoded.width(), decoded.height());
    if width.min(height) < MIN_ARTWORK_SIDE {
        return None;
    }
    let art = remote_art_path(data_dir, key);
    std::fs::create_dir_all(art.parent()?).ok()?;
    decoded.save(&art).ok()?;
    Some(Thumb {
        url: format!("marshall-file://{}", art.to_string_lossy()),
        width,
        height,
    })
}

pub fn path_from_id(id: &str) -> Option<PathBuf> {
    id.strip_prefix("local:").map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scans_only_mp3_and_flac_case_insensitively() {
        assert!(supported_file(Path::new("song.mp3")));
        assert!(supported_file(Path::new("album/track.FLAC")));
        assert!(!supported_file(Path::new("album/track.wav")));
    }

    #[test]
    fn local_queue_ids_recover_the_source_path() {
        let path = PathBuf::from(r"C:\Music\album\track.flac");
        let id = format!("local:{}", path.display());
        assert_eq!(path_from_id(&id), Some(path));
        assert_eq!(path_from_id("video-id"), None);
    }
}
