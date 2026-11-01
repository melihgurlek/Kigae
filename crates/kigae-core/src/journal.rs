//! The undo journal: before Kigae changes anything it writes down what was there (SPEC.md §4).
//! A journal is saved after every entry, so even a crash mid-switch leaves a usable one.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::sys::{FileState, UnitState};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Entry {
    KConfig { file: String, groups: Vec<String>, key: String, old: Option<String> },
    Colors { old: Option<String> },
    Style { old: Option<String> },
    Icons { old: Option<String> },
    Cursor { old: Option<String> },
    /// `dumpCurrentLayoutJS` output from before the switch, plus what it leaves out (tray
    /// settings, desktop widget positions) for `kigae.restore`.
    Layout {
        script: String,
        #[serde(default)]
        state: Option<String>,
    },
    Wallpaper { old: Option<String> },
    File { path: PathBuf, old: FileState },
    Unit { unit: String, old: UnitState },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    InProgress,
    Done,
    RolledBack,
    Undone,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Journal {
    pub ts: u64,
    pub from: Option<String>,
    pub to: String,
    pub mood: Option<String>,
    pub status: Status,
    pub entries: Vec<Entry>,
    /// Where it's saved; `None` for dry runs and tests.
    #[serde(skip)]
    pub path: Option<PathBuf>,
}

impl Journal {
    pub fn new(from: Option<&str>, to: &str, mood: Option<&str>, dir: Option<&Path>) -> Self {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or_default();
        let path = dir.map(|d| d.join(format!("{ts}-{}-{to}.json", from.unwrap_or("unknown"))));
        Journal {
            ts,
            from: from.map(str::to_string),
            to: to.to_string(),
            mood: mood.map(str::to_string),
            status: Status::InProgress,
            entries: vec![],
            path,
        }
    }

    pub fn push(&mut self, entry: Entry) -> std::io::Result<()> {
        self.entries.push(entry);
        self.save()
    }

    pub fn set_status(&mut self, status: Status) -> std::io::Result<()> {
        self.status = status;
        self.save()
    }

    pub fn save(&self) -> std::io::Result<()> {
        let Some(path) = &self.path else { return Ok(()) };
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let json = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, json)?;
        fs::rename(tmp, path)
    }

    pub fn load(path: &Path) -> std::io::Result<Self> {
        let text = fs::read_to_string(path)?;
        let mut j: Journal = serde_json::from_str(&text).map_err(std::io::Error::other)?;
        j.path = Some(path.to_path_buf());
        Ok(j)
    }

    /// All journals in `dir`, oldest first.
    pub fn list(dir: &Path) -> std::io::Result<Vec<Journal>> {
        let mut out = vec![];
        let entries = match fs::read_dir(dir) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(out),
            r => r?,
        };
        for e in entries {
            let p = e?.path();
            if p.extension().is_some_and(|x| x == "json") {
                out.push(Journal::load(&p)?);
            }
        }
        out.sort_by_key(|j| (j.ts, j.path.clone()));
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_disk() {
        let dir = std::env::temp_dir().join(format!("kigae-journal-{}", std::process::id()));
        let mut j = Journal::new(Some("ukiyo"), "og", Some("dark"), Some(&dir));
        j.push(Entry::Colors { old: Some("Ukiyo".into()) }).unwrap();
        j.push(Entry::File { path: "/x".into(), old: FileState::Link("/y".into()) }).unwrap();
        j.set_status(Status::Done).unwrap();
        let all = Journal::list(&dir).unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].entries, j.entries);
        assert_eq!(all[0].status, Status::Done);
        fs::remove_dir_all(dir).unwrap();
    }
}
