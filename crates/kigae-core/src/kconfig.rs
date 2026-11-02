//! Reading a KConfig key in-process, the way `kreadconfig6` would, without spawning it (a switch
//! reads about 30 keys; each spawn costs ~18 ms). Only reads: writes still go through
//! `kwriteconfig6` (SPEC.md §11). Anything this doesn't model exactly (immutable `[$i]` entries,
//! `$e` expansion, deleted `[$d]` markers, unknown escapes) returns `None`, and the caller asks
//! `kreadconfig6` instead.

use std::path::PathBuf;

/// Config directories, most important first: `$XDG_CONFIG_HOME`, then `$XDG_CONFIG_DIRS`.
pub fn dirs() -> Vec<PathBuf> {
    let env = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty());
    let home = env("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| env("HOME").map(|h| PathBuf::from(h).join(".config")));
    let rest: Vec<PathBuf> = env("XDG_CONFIG_DIRS")
        .map(|v| std::env::split_paths(&v).collect())
        .unwrap_or_else(|| vec![PathBuf::from("/etc/xdg")]);
    home.into_iter().chain(rest).collect()
}

/// `Some(value)` (`Some(None)`: not set anywhere), or `None` when only `kreadconfig6` can tell.
pub fn read(dirs: &[PathBuf], file: &str, groups: &[String], key: &str) -> Option<Option<String>> {
    if file.contains('/') {
        return None;
    }
    let texts: Vec<String> = dirs
        .iter()
        .filter_map(|d| match std::fs::read(d.join(file)) {
            Ok(b) => Some(String::from_utf8(b).ok()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(_) => Some(None),
        })
        .collect::<Option<_>>()?;
    // an immutable file, group or key below can override a value above: leave it to KConfig
    if texts.iter().any(|t| t.contains("[$i]")) {
        return None;
    }
    for text in &texts {
        if let Some(v) = lookup(text, groups, key)? {
            return Some(Some(v));
        }
    }
    Some(None)
}

/// The key's value in one file: `Some(Some(v))`, `Some(None)` if absent, `None` if unsure.
fn lookup(text: &str, groups: &[String], key: &str) -> Option<Option<String>> {
    let mut group: Vec<String> = vec![];
    let mut found = None;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') {
            group = header(line)?;
            continue;
        }
        if group.as_slice() != groups {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else { continue };
        let k = k.trim();
        let name = match k.split_once('[') {
            // `key[$e]`, `key[$d]` and friends: options we don't model
            Some((name, opt)) if opt.starts_with('$') => {
                if name.trim() == key {
                    return None;
                }
                continue;
            }
            // `key[de]`: a translation, not the value kreadconfig6 prints
            Some(_) => continue,
            None => k,
        };
        if name == key {
            found = Some(unescape(v.trim())?);
        }
    }
    Some(found)
}

/// `[A][B]` → `["A", "B"]`; `None` for anything unusual.
fn header(line: &str) -> Option<Vec<String>> {
    let mut out = vec![];
    let mut rest = line;
    while let Some(r) = rest.strip_prefix('[') {
        let end = r.find(']')?;
        let seg = &r[..end];
        if seg.starts_with('$') {
            return None;
        }
        out.push(unescape(seg)?);
        rest = &r[end + 1..];
    }
    rest.trim().is_empty().then_some(out)
}

/// KConfig's escapes; `\;` and `\,` stay as they are (they matter only to list readers).
fn unescape(s: &str) -> Option<String> {
    if !s.contains('\\') {
        return Some(s.to_string());
    }
    let mut out = Vec::with_capacity(s.len());
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] != b'\\' {
            out.push(b[i]);
            i += 1;
            continue;
        }
        let c = *b.get(i + 1)?;
        i += 2;
        match c {
            b'\\' => out.push(b'\\'),
            b'n' => out.push(b'\n'),
            b't' => out.push(b'\t'),
            b'r' => out.push(b'\r'),
            b's' => out.push(b' '),
            b';' | b',' => out.extend([b'\\', c]),
            b'x' => {
                let hex = |c: u8| (c as char).to_digit(16);
                let (h, l) = (hex(*b.get(i)?)?, hex(*b.get(i + 1)?)?);
                out.push((h * 16 + l) as u8);
                i += 2;
            }
            _ => return None,
        }
    }
    String::from_utf8(out).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str, files: &[(&str, &str)]) -> PathBuf {
        let d = std::env::temp_dir().join(format!("kigae-kconfig-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        for (file, text) in files {
            std::fs::write(d.join(file), text).unwrap();
        }
        d
    }

    fn g(groups: &[&str]) -> Vec<String> {
        groups.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn reads_values_groups_and_escapes() {
        let d = dir("read", &[(
            "rc",
            "top=1\n[General]\nfont=Inter,10\n  spaced =  a b  \nesc=\\sx\\ty\\\\z\\x41\n\
             name[de]=Deutsch\nname=English\n[Plugins][Sub]\nk=nested\n[General]\nfont=Later\n",
        )]);
        let dirs = [d];
        assert_eq!(read(&dirs, "rc", &[], "top"), Some(Some("1".into())));
        assert_eq!(read(&dirs, "rc", &g(&["General"]), "font"), Some(Some("Later".into())));
        assert_eq!(read(&dirs, "rc", &g(&["General"]), "spaced"), Some(Some("a b".into())));
        assert_eq!(read(&dirs, "rc", &g(&["General"]), "esc"), Some(Some(" x\ty\\zA".into())));
        assert_eq!(read(&dirs, "rc", &g(&["General"]), "name"), Some(Some("English".into())));
        assert_eq!(read(&dirs, "rc", &g(&["Plugins", "Sub"]), "k"), Some(Some("nested".into())));
        assert_eq!(read(&dirs, "rc", &g(&["Plugins"]), "k"), Some(None));
        assert_eq!(read(&dirs, "missing", &g(&["General"]), "font"), Some(None));
    }

    #[test]
    fn cascade_and_unsure_cases() {
        let user = dir("user", &[("rc", "[G]\nk=user\n"), ("exp", "[G]\nk[$e]=$HOME\n"), ("bad", "[G]\nk=a\\qb\n")]);
        let sys = dir("sys", &[("rc", "[G]\nk=system\nonly=sys\n"), ("imm", "[G][$i]\nk=locked\n")]);
        let dirs = [user, sys];
        assert_eq!(read(&dirs, "rc", &g(&["G"]), "k"), Some(Some("user".into())));
        assert_eq!(read(&dirs, "rc", &g(&["G"]), "only"), Some(Some("sys".into())));
        assert_eq!(read(&dirs, "imm", &g(&["G"]), "k"), None);
        assert_eq!(read(&dirs, "exp", &g(&["G"]), "k"), None);
        assert_eq!(read(&dirs, "bad", &g(&["G"]), "k"), None);
    }

    /// Every key of the real config files on this machine reads the same as `kreadconfig6`.
    /// `cargo test -p kigae-core -- --ignored matches_kreadconfig6`
    #[test]
    #[ignore]
    fn matches_kreadconfig6() {
        let dirs = dirs();
        let mut checked = 0;
        for file in ["kdeglobals", "kwinrc", "plasmarc", "kcminputrc", "ksplashrc", "konsolerc", "katerc", "kwriterc"] {
            let Ok(text) = std::fs::read_to_string(dirs[0].join(file)) else { continue };
            let mut group = vec![];
            for line in text.lines() {
                let line = line.trim();
                if line.starts_with('[') {
                    group = header(line).unwrap_or_default();
                    continue;
                }
                let Some((k, _)) = line.split_once('=') else { continue };
                if k.contains('[') || group.is_empty() {
                    continue;
                }
                let Some(ours) = read(&dirs, file, &group, k.trim()) else { continue };
                let mut argv = vec!["kreadconfig6", "--file", file];
                for g in &group {
                    argv.extend(["--group", g.as_str()]);
                }
                argv.extend(["--key", k.trim(), "--default", "\u{1}unset"]);
                let out = crate::sys::run(&argv).unwrap();
                let theirs = out.strip_suffix('\n').unwrap_or(&out);
                let theirs = (theirs != "\u{1}unset").then(|| theirs.to_string());
                assert_eq!(ours, theirs, "{file} {group:?} {k}");
                checked += 1;
            }
        }
        assert!(checked > 50, "only {checked} keys checked");
    }
}
