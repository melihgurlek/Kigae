//! Find rices: `rice.toml` on each matching branch, then local manifests (SPEC.md §2).
//! Nothing is checked out; files are read straight from git.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::manifest::Manifest;

#[derive(Debug, Clone)]
pub struct Rice {
    pub manifest: Manifest,
    pub branch: Option<String>,
    /// Directory of a local manifest; `./…` paths resolve here.
    pub local_dir: Option<PathBuf>,
    pub repo: PathBuf,
}

#[derive(Debug, thiserror::Error)]
pub enum DiscoverError {
    #[error("git {args}: {msg}")]
    Git { args: String, msg: String },
    #[error("{0}: {1}")]
    Io(PathBuf, std::io::Error),
    #[error("{0}: {1}")]
    Manifest(String, crate::ManifestError),
    #[error("{rice}: {path} needs a branch (or a ./ path next to the manifest)")]
    NoSource { rice: String, path: String },
}

pub fn git(repo: &Path, args: &[&str]) -> Result<Vec<u8>, DiscoverError> {
    let out = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .map_err(|e| DiscoverError::Git { args: args.join(" "), msg: e.to_string() })?;
    if !out.status.success() {
        let msg = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(DiscoverError::Git { args: args.join(" "), msg });
    }
    Ok(out.stdout)
}

fn matches(pattern: &str, branch: &str) -> bool {
    match pattern.strip_suffix('*') {
        Some(prefix) => branch.starts_with(prefix),
        None => pattern == branch,
    }
}

impl Rice {
    pub fn id(&self) -> &str {
        &self.manifest.id
    }

    /// Read a file a manifest names: `./x` next to a local manifest, else `branch:x`.
    pub fn read(&self, path: &str) -> Result<Vec<u8>, DiscoverError> {
        if let (Some(rest), Some(dir)) = (path.strip_prefix("./"), &self.local_dir) {
            let p = dir.join(rest);
            return std::fs::read(&p).map_err(|e| DiscoverError::Io(p, e));
        }
        let branch = self.branch.as_ref().ok_or_else(|| DiscoverError::NoSource {
            rice: self.manifest.id.clone(),
            path: path.into(),
        })?;
        git(&self.repo, &["show", &format!("{branch}:{path}")])
    }

    pub fn layout_js(&self) -> Result<Option<String>, DiscoverError> {
        let Some(path) = &self.manifest.plasma.layout else { return Ok(None) };
        Ok(Some(String::from_utf8_lossy(&self.read(path)?).into_owned()))
    }
}

/// All rices, sorted by id. Broken manifests come back as warnings, not errors, so one bad
/// branch doesn't hide the others.
pub fn discover(repo: &Path, patterns: &[String], manifests: &Path) -> (Vec<Rice>, Vec<String>) {
    let mut rices: Vec<Rice> = vec![];
    let mut warnings = vec![];
    match git(repo, &["for-each-ref", "--format=%(refname:short)", "refs/heads/"]) {
        Err(e) => warnings.push(e.to_string()),
        Ok(out) => {
            for branch in String::from_utf8_lossy(&out).lines() {
                if !patterns.iter().any(|p| matches(p, branch)) {
                    continue;
                }
                let Ok(text) = git(repo, &["show", &format!("{branch}:rice.toml")]) else { continue };
                match Manifest::parse(&String::from_utf8_lossy(&text)) {
                    Ok(manifest) => rices.push(Rice {
                        manifest,
                        branch: Some(branch.to_string()),
                        local_dir: None,
                        repo: repo.to_path_buf(),
                    }),
                    Err(e) => warnings.push(format!("{branch}:rice.toml: {e}")),
                }
            }
        }
    }
    let mut local: Vec<PathBuf> = std::fs::read_dir(manifests)
        .map(|d| d.filter_map(|e| e.ok().map(|e| e.path())).collect())
        .unwrap_or_default();
    local.sort();
    for path in local.into_iter().filter(|p| p.extension().is_some_and(|x| x == "toml")) {
        let parsed = std::fs::read_to_string(&path)
            .map_err(|e| e.to_string())
            .and_then(|t| Manifest::parse(&t).map_err(|e| e.to_string()));
        match parsed {
            Err(e) => warnings.push(format!("{}: {e}", path.display())),
            Ok(m) if rices.iter().any(|r| r.manifest.id == m.id) => {
                warnings.push(format!("{}: {:?} is also on a branch; the branch wins", path.display(), m.id));
            }
            Ok(manifest) => rices.push(Rice {
                branch: manifest.branch.clone(),
                manifest,
                local_dir: path.parent().map(Path::to_path_buf),
                repo: repo.to_path_buf(),
            }),
        }
    }
    rices.sort_by(|a, b| a.manifest.id.cmp(&b.manifest.id));
    (rices, warnings)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sh(dir: &Path, script: &str) {
        let ok = Command::new("sh").arg("-c").arg(script).current_dir(dir).status().unwrap().success();
        assert!(ok, "{script}");
    }

    #[test]
    fn finds_branch_and_local_manifests() {
        let root = std::env::temp_dir().join(format!("kigae-discover-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let repo = root.join("repo");
        let local = root.join("manifests");
        std::fs::create_dir_all(&repo).unwrap();
        std::fs::create_dir_all(local.join("og")).unwrap();
        sh(&repo, "git init -q -b main && git -c user.name=t -c user.email=t@t commit -q --allow-empty -m init \
            && git checkout -q -b rice/ukiyo \
            && printf 'schema = 1\\nid = \"ukiyo\"\\nname = \"Ukiyo\"\\n[plasma]\\nlayout = \"l.js\"\\n' > rice.toml \
            && echo 'var x;' > l.js && git add . && git -c user.name=t -c user.email=t@t commit -q -m u \
            && git checkout -q main && git branch backup/og && git branch rice/broken");
        std::fs::write(local.join("og.toml"), "schema = 1\nid = \"og\"\nname = \"OG\"\nbranch = \"backup/og\"\n[plasma]\nlayout = \"./og/layout.js\"\n").unwrap();
        std::fs::write(local.join("og/layout.js"), "var og;").unwrap();

        let (rices, warnings) = discover(&repo, &["rice/*".into(), "backup/og".into()], &local);
        assert!(warnings.is_empty(), "{warnings:?}");
        let ids: Vec<&str> = rices.iter().map(Rice::id).collect();
        assert_eq!(ids, ["og", "ukiyo"]);
        assert_eq!(rices[1].layout_js().unwrap().unwrap().trim(), "var x;");
        assert_eq!(rices[0].layout_js().unwrap().unwrap(), "var og;");
        assert_eq!(rices[0].branch.as_deref(), Some("backup/og"));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn branch_patterns() {
        assert!(matches("rice/*", "rice/ukiyo"));
        assert!(!matches("rice/*", "backup/og"));
        assert!(matches("backup/og", "backup/og"));
    }
}
