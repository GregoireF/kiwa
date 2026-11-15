// SPDX-License-Identifier: MIT OR Apache-2.0

//! Run history with frecency ranking (frequency x recency), stored as JSON in the
//! platform state/data directory. Corrupt or missing state is never fatal.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Entry {
    pub path: PathBuf,
    pub task: String,
    pub count: u32,
    pub last: u64,
    #[serde(default)]
    pub exit_code: Option<i32>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct State {
    #[serde(default)]
    pub history: Vec<Entry>,
}

const MAX_ENTRIES: usize = 500;

pub fn state_path() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("KIWA_STATE") {
        return Some(PathBuf::from(p));
    }
    dirs::state_dir()
        .or_else(dirs::data_local_dir)
        .map(|d| d.join("kiwa").join("state.json"))
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

impl State {
    pub fn load() -> Self {
        state_path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let Some(p) = state_path() else { return };
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(json) = serde_json::to_string_pretty(self) {
            // Write-then-rename so a crash never leaves a truncated file.
            let tmp = p.with_extension("json.tmp");
            if std::fs::write(&tmp, json).is_ok() {
                let _ = std::fs::rename(&tmp, &p);
            }
        }
    }

    pub fn record(&mut self, path: &Path, task: &str, exit_code: Option<i32>) {
        let t = now();
        match self
            .history
            .iter_mut()
            .find(|e| e.path == path && e.task == task)
        {
            Some(e) => {
                e.count = e.count.saturating_add(1);
                e.last = t;
                e.exit_code = exit_code;
            }
            None => self.history.push(Entry {
                path: path.to_path_buf(),
                task: task.to_owned(),
                count: 1,
                last: t,
                exit_code,
            }),
        }
        if self.history.len() > MAX_ENTRIES {
            self.history.sort_by_key(|e| std::cmp::Reverse(e.last));
            self.history.truncate(MAX_ENTRIES);
        }
    }

    pub fn last(&self) -> Option<&Entry> {
        self.history.iter().max_by_key(|e| e.last)
    }

    /// Frecency score, 0 when never run. Same buckets idea as Firefox / zoxide.
    pub fn score(&self, path: &Path, task: &str) -> u32 {
        let Some(e) = self
            .history
            .iter()
            .find(|e| e.path == path && e.task == task)
        else {
            return 0;
        };
        let age = now().saturating_sub(e.last);
        let weight = match age {
            a if a < 3_600 => 16,
            a if a < 86_400 => 8,
            a if a < 604_800 => 4,
            _ => 1,
        };
        e.count.saturating_mul(weight)
    }

    pub fn clear(&mut self) {
        self.history.clear();
    }
}
