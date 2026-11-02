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

/// How many journals `save` keeps around; older ones are deleted when a new one is first saved.
const KEEP: usize = 50;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Journal {
    /// Milliseconds since the epoch; also the id for `kigae undo <ts>`. Journals written by
    /// older versions hold seconds (see [`Journal::when_secs`]); they sort before new ones.
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
            .map(|d| d.as_millis() as u64)
            .unwrap_or_default();
        let name = |ts: u64| format!("{ts}-{}-{to}.json", from.unwrap_or("unknown"));
        // never overwrite an earlier switch with the same from/to
        let mut ts = ts;
        if let Some(d) = dir {
            while d.join(name(ts)).exists() {
                ts += 1;
            }
        }
        let path = dir.map(|d| d.join(name(ts)));
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

    /// When it happened, in seconds since the epoch (old journals stored seconds, new ones ms).
    pub fn when_secs(&self) -> u64 {
        if self.ts < 100_000_000_000 { self.ts } else { self.ts / 1000 }
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
        let first = !path.exists();
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, json)?;
        fs::rename(tmp, path)?;
        if first && let Some(dir) = path.parent() {
            self.prune(dir);
        }
        Ok(())
    }

    /// Delete all but the newest [`KEEP`] journals in `dir`; this one always stays.
    fn prune(&self, dir: &Path) {
        let Ok(all) = Journal::list(dir) else { return };
        let excess = all.len().saturating_sub(KEEP);
        for old in all.iter().take(excess).filter(|j| j.path != self.path) {
            if let Some(p) = &old.path {
                let _ = fs::remove_file(p);
            }
        }
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
                match Journal::load(&p) {
                    Ok(j) => out.push(j),
                    Err(e) => eprintln!("kigae: skipping unreadable journal {}: {e}", p.display()),
                }
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

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("kigae-journal-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn same_millisecond_same_pair_keeps_both() {
        let dir = scratch("clash");
        let mut seen = std::collections::BTreeSet::new();
        for _ in 0..20 {
            let j = Journal::new(Some("a"), "b", None, Some(&dir));
            j.save().unwrap();
            assert!(seen.insert(j.path.clone()), "overwrote {:?}", j.path);
        }
        assert_eq!(Journal::list(&dir).unwrap().len(), 20);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn bad_files_are_skipped_and_old_second_journals_sort_first() {
        let dir = scratch("bad");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("1700000000-a-b.json"), r#"{"ts":1700000000,"from":"a","to":"b","mood":null,"status":"done","entries":[]}"#).unwrap();
        fs::write(dir.join("garbage.json"), "{nope").unwrap();
        Journal::new(Some("a"), "b", None, Some(&dir)).save().unwrap();
        let all = Journal::list(&dir).unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].ts, 1700000000);
        assert_eq!(all[0].when_secs(), 1700000000);
        assert_eq!(all[1].when_secs(), all[1].ts / 1000);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn keeps_the_newest_fifty() {
        let dir = scratch("prune");
        fs::create_dir_all(&dir).unwrap();
        for i in 1..=55u64 {
            let j = Journal { ts: i, from: None, to: "b".into(), mood: None, status: Status::Done, entries: vec![], path: Some(dir.join(format!("{i}-x-b.json"))) };
            fs::write(j.path.as_ref().unwrap(), serde_json::to_string(&j).unwrap()).unwrap();
        }
        Journal::new(Some("a"), "b", None, Some(&dir)).save().unwrap();
        let all = Journal::list(&dir).unwrap();
        assert_eq!(all.len(), KEEP);
        assert_eq!(all[0].ts, 7, "the oldest six went");
        fs::remove_dir_all(dir).unwrap();
    }
}
