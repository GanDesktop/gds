//! Filesystem capacity is sampled independently of device discovery, so a slow
//! mount cannot stall hotplug events or queries for other volumes.

use std::{
    collections::HashMap,
    path::PathBuf,
    time::{Duration, Instant},
};

use super::{Snapshot, VolumeId};

const REFRESH_INTERVAL: Duration = Duration::from_secs(5);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Capacity {
    pub total: u64,
    /// Bytes available to an unprivileged user, excluding reserved blocks.
    pub available: u64,
}

impl Capacity {
    /// The occupied/reserved part of the bar; the unfilled part is available.
    pub fn unavailable_fraction(self) -> f32 {
        if self.total == 0 {
            return 0.;
        }
        self.total.saturating_sub(self.available) as f32 / self.total as f32
    }

    /// # Errors
    /// Rejects a missing block size or a capacity that cannot fit in bytes.
    fn from_blocks(block_size: u64, blocks: u64, available: u64) -> Result<Self, ReadError> {
        if block_size == 0 {
            return Err(ReadError::InvalidBlockSize);
        }
        Ok(Self {
            total: blocks.checked_mul(block_size).ok_or(ReadError::Overflow)?,
            available: available
                .min(blocks)
                .checked_mul(block_size)
                .ok_or(ReadError::Overflow)?,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CapacityState {
    Loading,
    Ready(Capacity),
    Unavailable(String),
}

#[derive(Debug, thiserror::Error)]
pub(super) enum ReadError {
    #[error("cannot read filesystem capacity: {0}")]
    Io(#[from] std::io::Error),
    #[error("the filesystem reported an invalid block size")]
    InvalidBlockSize,
    #[error("the filesystem capacity exceeds the supported range")]
    Overflow,
    #[cfg(not(target_os = "linux"))]
    #[error("filesystem capacity is not supported on this platform")]
    Unsupported,
}

#[derive(Clone, Debug)]
pub(super) struct ReadRequest {
    id: u64,
    volume: VolumeId,
    root: PathBuf,
}

impl ReadRequest {
    /// # Errors
    /// Reports an inaccessible/disconnected mount, unsupported statistics, or overflow.
    pub fn read(&self) -> Result<Capacity, ReadError> {
        #[cfg(target_os = "linux")]
        {
            let directory = std::fs::File::open(&self.root)?;
            let stats = rustix::fs::fstatvfs(&directory).map_err(std::io::Error::from)?;
            Capacity::from_blocks(stats.f_frsize, stats.f_blocks, stats.f_bavail)
        }
        #[cfg(not(target_os = "linux"))]
        {
            Err(ReadError::Unsupported)
        }
    }
}

struct Entry {
    root: PathBuf,
    state: CapacityState,
    pending: Option<u64>,
    sampled: Option<Instant>,
}

#[derive(Default)]
pub(super) struct UsageCache {
    entries: HashMap<VolumeId, Entry>,
    next_request: u64,
}

impl UsageCache {
    pub fn invalidate(&mut self) {
        for entry in self.entries.values_mut() {
            entry.sampled = None;
        }
    }

    pub fn get(&self, volume: &VolumeId) -> Option<&CapacityState> {
        self.entries.get(volume).map(|entry| &entry.state)
    }

    pub fn requests(&mut self, snapshot: &Snapshot, now: Instant) -> Vec<ReadRequest> {
        self.entries.retain(|id, entry| {
            snapshot.volume(id).is_some_and(|volume| {
                volume.visible && volume.mount_points.first() == Some(&entry.root)
            })
        });
        let mut requests = Vec::new();
        for volume in snapshot.volumes.iter().filter(|volume| volume.visible) {
            let Some(root) = volume.mount_points.first() else {
                continue;
            };
            let entry = self
                .entries
                .entry(volume.id.clone())
                .or_insert_with(|| Entry {
                    root: root.clone(),
                    state: CapacityState::Loading,
                    pending: None,
                    sampled: None,
                });
            if entry.pending.is_some()
                || entry.sampled.is_some_and(|sampled| {
                    now.saturating_duration_since(sampled) < REFRESH_INTERVAL
                })
            {
                continue;
            }
            self.next_request += 1;
            entry.pending = Some(self.next_request);
            requests.push(ReadRequest {
                id: self.next_request,
                volume: volume.id.clone(),
                root: root.clone(),
            });
        }
        requests
    }

    pub fn finish(
        &mut self,
        request: ReadRequest,
        result: Result<Capacity, ReadError>,
        now: Instant,
    ) -> bool {
        let Some(entry) = self.entries.get_mut(&request.volume) else {
            return false;
        };
        if entry.pending != Some(request.id) || entry.root != request.root {
            return false;
        }
        let state = match result {
            Ok(capacity) => CapacityState::Ready(capacity),
            Err(error) => CapacityState::Unavailable(error.to_string()),
        };
        let changed = entry.state != state;
        entry.state = state;
        entry.pending = None;
        entry.sampled = Some(now);
        changed
    }
}

#[cfg(test)]
mod tests;
