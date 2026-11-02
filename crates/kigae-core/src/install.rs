//! `kigae install` / `uninstall`: copy a rice's asset folders into `$HOME` once, and remember
//! exactly what was copied (SPEC.md §10). Switching never copies anything after this.

use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};

use crate::discover::{DiscoverError, Rice, git};
use crate::engine::Paths;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    File { executable: bool },
    Symlink,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    New,
    Same,
    Differs,
}

#[derive(Debug, Clone)]
pub struct Item {
    pub dest: PathBuf,
    pub contents: Vec<u8>,
    pub kind: Kind,
    pub state: State,
}

fn state(dest: &Path, contents: &[u8], kind: Kind) -> State {
    match kind {
        Kind::Symlink => match fs::read_link(dest) {
            Ok(t) if t.as_os_str().as_encoded_bytes() == contents => State::Same,
            Ok(_) => State::Differs,
            Err(_) if fs::symlink_metadata(dest).is_ok() => State::Differs,
            Err(_) => State::New,
        },
        Kind::File { .. } => match fs::read(dest) {
            Ok(c) if c == contents => State::Same,
            Ok(_) => State::Differs,
            Err(_) => State::New,
        },
    }
}

/// What `install` would write, without writing it.
pub fn plan(rice: &Rice, paths: &Paths) -> Result<Vec<Item>, DiscoverError> {
    let mut items = vec![];
    for (src, target) in &rice.manifest.install {
        let dest_root = paths.home.join(target);
        let mut push = |dest: PathBuf, contents: Vec<u8>, kind: Kind| {
            let state = state(&dest, &contents, kind);
            items.push(Item { dest, contents, kind, state });
        };
        if let (Some(rest), Some(dir)) = (src.strip_prefix("./"), &rice.local_dir) {
            let root = dir.join(rest);
            walk(&root, &mut |p| {
                let rel = p.strip_prefix(&root).unwrap_or(Path::new(""));
                let dest = if rel.as_os_str().is_empty() { dest_root.clone() } else { dest_root.join(rel) };
                let meta = fs::symlink_metadata(p).map_err(|e| DiscoverError::Io(p.into(), e))?;
                if meta.file_type().is_symlink() {
                    let t = fs::read_link(p).map_err(|e| DiscoverError::Io(p.into(), e))?;
                    push(dest, t.into_os_string().into_encoded_bytes(), Kind::Symlink);
                } else {
                    let c = fs::read(p).map_err(|e| DiscoverError::Io(p.into(), e))?;
                    push(dest, c, Kind::File { executable: meta.permissions().mode() & 0o111 != 0 });
                }
                Ok(())
            })?;
            continue;
        }
        let branch = rice.branch.as_ref().ok_or_else(|| DiscoverError::NoSource {
            rice: rice.manifest.id.clone(),
            path: src.clone(),
        })?;
        let listing = git(&rice.repo, &["ls-tree", "-r", "-z", "--full-tree", branch, "--", src])?;
        let listing = String::from_utf8_lossy(&listing).into_owned();
        if listing.is_empty() {
            return Err(DiscoverError::Git { args: format!("ls-tree {branch} -- {src}"), msg: "nothing there".into() });
        }
        for entry in listing.split('\0').filter(|e| !e.is_empty()) {
            // "<mode> <type> <sha>\t<path>"
            let Some((meta, path)) = entry.split_once('\t') else { continue };
            let mut f = meta.split(' ');
            let (mode, kind, sha) = (f.next().unwrap_or(""), f.next().unwrap_or(""), f.next().unwrap_or(""));
            if kind != "blob" {
                continue;
            }
            let rel = path.strip_prefix(src.as_str()).unwrap_or(path).trim_start_matches('/');
            let dest = if rel.is_empty() { dest_root.clone() } else { dest_root.join(rel) };
            let contents = git(&rice.repo, &["cat-file", "blob", sha])?;
            let kind = match mode {
                "120000" => Kind::Symlink,
                "100755" => Kind::File { executable: true },
                _ => Kind::File { executable: false },
            };
            push(dest, contents, kind);
        }
    }
    Ok(items)
}

fn walk(p: &Path, f: &mut dyn FnMut(&Path) -> Result<(), DiscoverError>) -> Result<(), DiscoverError> {
    let meta = fs::symlink_metadata(p).map_err(|e| DiscoverError::Io(p.into(), e))?;
    if meta.is_dir() {
        let mut entries: Vec<PathBuf> = fs::read_dir(p)
            .map_err(|e| DiscoverError::Io(p.into(), e))?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .collect();
        entries.sort();
        for e in entries {
            walk(&e, f)?;
        }
        Ok(())
    } else {
        f(p)
    }
}

fn record_path(paths: &Paths, id: &str) -> PathBuf {
    paths.state.join("installed").join(format!("{id}.json"))
}

/// Write the planned items. Files that exist with other contents are only overwritten with
/// `force`. Returns what was written; the record of installed files (only those) is updated
/// either way.
pub fn run(items: &[Item], force: bool, paths: &Paths, id: &str) -> Result<Vec<PathBuf>, String> {
    let conflicts: Vec<String> =
        items.iter().filter(|i| i.state == State::Differs).map(|i| i.dest.display().to_string()).collect();
    if !conflicts.is_empty() && !force {
        return Err(format!(
            "these exist with other contents (use --force to overwrite):\n  {}",
            conflicts.join("\n  ")
        ));
    }
    let mut written = vec![];
    for item in items.iter().filter(|i| i.state != State::Same) {
        let err = |e: std::io::Error| format!("{}: {e}", item.dest.display());
        if let Some(dir) = item.dest.parent() {
            fs::create_dir_all(dir).map_err(err)?;
        }
        if fs::symlink_metadata(&item.dest).is_ok() {
            fs::remove_file(&item.dest).map_err(err)?;
        }
        match item.kind {
            Kind::Symlink => {
                let target = String::from_utf8_lossy(&item.contents).into_owned();
                symlink(target, &item.dest).map_err(err)?;
            }
            Kind::File { executable } => {
                fs::write(&item.dest, &item.contents).map_err(err)?;
                if executable {
                    fs::set_permissions(&item.dest, fs::Permissions::from_mode(0o755)).map_err(err)?;
                }
            }
        }
        written.push(item.dest.clone());
    }
    let record = record_path(paths, id);
    let mut all: Vec<PathBuf> = fs::read_to_string(&record)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    // only what we wrote: a file that was already there (State::Same) isn't ours to remove
    all.extend(written.iter().cloned());
    all.sort();
    all.dedup();
    fs::create_dir_all(record.parent().expect("has parent")).map_err(|e| e.to_string())?;
    fs::write(&record, serde_json::to_string_pretty(&all).expect("paths serialize")).map_err(|e| e.to_string())?;
    Ok(written)
}

/// Remove exactly what `install` recorded for this rice, then any folders that became empty.
pub fn uninstall(paths: &Paths, id: &str) -> Result<Vec<PathBuf>, String> {
    let record = record_path(paths, id);
    let text = fs::read_to_string(&record).map_err(|e| format!("{}: {e}", record.display()))?;
    let files: Vec<PathBuf> = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    let mut removed = vec![];
    for f in &files {
        if fs::remove_file(f).is_ok() {
            removed.push(f.clone());
        }
        let mut dir = f.parent();
        while let Some(d) = dir {
            if d == paths.home || !d.starts_with(&paths.home) || fs::remove_dir(d).is_err() {
                break;
            }
            dir = d.parent();
        }
    }
    fs::remove_file(&record).map_err(|e| e.to_string())?;
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uninstall_leaves_files_it_did_not_write() {
        let home = std::env::temp_dir().join(format!("kigae-install-{}", std::process::id()));
        let _ = fs::remove_dir_all(&home);
        fs::create_dir_all(&home).unwrap();
        let paths = Paths { home: home.clone(), state: home.join("state") };
        let (shared, own) = (home.join("shared.txt"), home.join("own.txt"));
        fs::write(&shared, b"same").unwrap();
        let item = |dest: &Path| {
            let kind = Kind::File { executable: false };
            Item { dest: dest.into(), contents: b"same".to_vec(), kind, state: state(dest, b"same", kind) }
        };
        let items = [item(&shared), item(&own)];
        assert_eq!(run(&items, false, &paths, "a").unwrap(), vec![own.clone()]);
        assert_eq!(uninstall(&paths, "a").unwrap(), vec![own.clone()]);
        assert!(shared.exists() && !own.exists());
        fs::remove_dir_all(home).unwrap();
    }
}
