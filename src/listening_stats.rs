//! Local listening history for Marshall's Statistics page.
//!
//! Records stay on this device. Playback position deltas (rather than wall-clock
//! time) keep pauses and seeks from inflating listening totals.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::model::Track;

const MAX_ENTRIES: usize = 100_000;
const SAVE_EVERY: Duration = Duration::from_secs(12);

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Entry {
    pub started_at: i64,
    pub track: Track,
    pub listened_seconds: f64,
}

struct Active {
    generation: u64,
    track_id: String,
    track: Track,
    started_at: i64,
    last_position: Duration,
    entry: Option<usize>,
}

pub struct Store {
    path: PathBuf,
    entries: Vec<Entry>,
    active: Option<Active>,
    last_save: Instant,
}

impl Store {
    pub fn open(data_dir: &Path) -> Self {
        let path = data_dir.join("listening_stats.json");
        let mut entries: Vec<Entry> = std::fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        if entries.len() > MAX_ENTRIES {
            entries.drain(..entries.len() - MAX_ENTRIES);
        }
        Self {
            path,
            entries,
            active: None,
            last_save: Instant::now(),
        }
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    pub fn tick(
        &mut self,
        generation: u64,
        track: Option<Track>,
        playing: bool,
        position: Duration,
    ) {
        let Some(track) = track else {
            if self.active.take().is_some() {
                self.flush();
            }
            return;
        };
        let changed = self
            .active
            .as_ref()
            .is_some_and(|a| a.generation != generation || a.track_id != track.id);
        if changed {
            self.active = None;
            self.flush();
        }
        if self.active.is_none() {
            self.active = Some(Active {
                generation,
                track_id: track.id.clone(),
                track: track.clone(),
                started_at: unix_now()
                    .saturating_sub(position.as_secs().min(i64::MAX as u64) as i64),
                last_position: position,
                entry: None,
            });
        }

        let delta = {
            let active = self
                .active
                .as_mut()
                .expect("active session was just created");
            let delta = if playing {
                position.saturating_sub(active.last_position)
            } else {
                Duration::ZERO
            };
            active.last_position = position;
            if delta <= Duration::from_secs(3) {
                delta.as_secs_f64()
            } else {
                0.0
            }
        };
        if delta > 0.0 {
            let index = if let Some(index) = self.active.as_ref().and_then(|a| a.entry) {
                index
            } else {
                let active = self.active.as_ref().expect("active session exists");
                self.entries.push(Entry {
                    started_at: active.started_at,
                    track: active.track.clone(),
                    listened_seconds: 0.0,
                });
                let index = self.entries.len() - 1;
                if let Some(active) = &mut self.active {
                    active.entry = Some(index);
                }
                index
            };
            self.entries[index].listened_seconds += delta;
        }
        if self.last_save.elapsed() >= SAVE_EVERY {
            self.flush();
        }
    }

    pub fn flush(&mut self) {
        if self.entries.len() > MAX_ENTRIES {
            let dropped = self.entries.len() - MAX_ENTRIES;
            self.entries.drain(..dropped);
            if let Some(active) = &mut self.active {
                active.entry = active.entry.and_then(|i| i.checked_sub(dropped));
            }
        }
        if let Some(parent) = self.path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        match serde_json::to_vec(&self.entries)
            .and_then(|bytes| std::fs::write(&self.path, bytes).map_err(serde_json::Error::io))
        {
            Ok(()) => self.last_save = Instant::now(),
            Err(e) => log::warn!("statistics: couldn't save local listening history: {e}"),
        }
    }
}

pub fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .min(i64::MAX as u64) as i64
}

pub fn time_parts(timestamp: i64) -> (i32, u8, u8, u8, i32) {
    let days = timestamp.div_euclid(86_400);
    let seconds = timestamp.rem_euclid(86_400);
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 }.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let mut year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    (
        year as i32,
        month as u8,
        day as u8,
        (seconds / 3_600) as u8,
        days as i32,
    )
}
