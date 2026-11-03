//! The deep switch (SPEC.md §11): the parts of a rice that live outside `$HOME` and need root,
//! the SDDM login theme and the GRUB theme. Never on the hotkey: `kigae deep <id>` runs them
//! behind one pkexec prompt, and `kigae apply` only says when the login no longer matches.

use std::path::{Path, PathBuf};

pub const SDDM_CONF: &str = "/etc/sddm.conf";
pub const SDDM_CONF_D: &str = "/etc/sddm.conf.d";
pub const SDDM_THEMES: &str = "/usr/share/sddm/themes";
pub const GRUB_DEFAULT: &str = "/etc/default/grub";

/// What root runs, as `pkexec /bin/sh -c ROOT_SCRIPT sh <sddm theme> <grub script>` (either may be
/// empty). The commands travel as an argument, not as a file a user process could swap. The
/// first change keeps a copy of sddm.conf as sddm.conf.pre-kigae.
pub const ROOT_SCRIPT: &str = r#"set -eu
theme=$1 grub=$2 conf=/etc/sddm.conf
if [ -n "$theme" ]; then
  case $theme in *[!A-Za-z0-9._-]*|.*) echo "bad theme name: $theme" >&2; exit 2;; esac
  [ -f "/usr/share/sddm/themes/$theme/metadata.desktop" ] || { echo "SDDM theme $theme is not installed" >&2; exit 1; }
  [ -e "$conf" ] || : > "$conf"
  [ -e "$conf.pre-kigae" ] || cp -a "$conf" "$conf.pre-kigae"
  if grep -q '^[[:space:]]*Current=' "$conf"; then
    sed -i "s/^\([[:space:]]*\)Current=.*/\1Current=$theme/" "$conf"
  else
    printf '\n[Theme]\nCurrent=%s\n' "$theme" >> "$conf"
  fi
  echo "login: $theme"
fi
if [ -n "$grub" ]; then
  bash "$grub"
fi
"#;

/// `[Theme] Current` from an sddm.conf-style text, if it sets one.
pub fn current_in(text: &str) -> Option<String> {
    let mut group = String::new();
    let mut found = None;
    for line in text.lines().map(str::trim) {
        if let Some(g) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            group = g.to_string();
        } else if group == "Theme"
            && let Some(v) = line.strip_prefix("Current=")
        {
            found = Some(v.trim().to_string());
        }
    }
    found
}

/// The login theme SDDM will use: /etc/sddm.conf wins over /etc/sddm.conf.d, where the last
/// file in name order wins.
pub fn login_theme() -> Option<String> {
    if let Some(t) = std::fs::read_to_string(SDDM_CONF).ok().as_deref().and_then(current_in) {
        return Some(t);
    }
    let mut files: Vec<PathBuf> = std::fs::read_dir(SDDM_CONF_D)
        .map(|d| d.filter_map(|e| e.ok().map(|e| e.path())).collect())
        .unwrap_or_default();
    files.sort();
    files.iter().rev().find_map(|f| std::fs::read_to_string(f).ok().as_deref().and_then(current_in))
}

pub fn sddm_installed(theme: &str) -> bool {
    Path::new(SDDM_THEMES).join(theme).join("metadata.desktop").is_file()
}

/// Whether GRUB already shows this rice's theme. Rices install their GRUB theme as
/// /boot/grub2/themes/<id>/ (Ukiyo's script does), so the line names the id.
pub fn grub_shows(id: &str) -> bool {
    std::fs::read_to_string(GRUB_DEFAULT)
        .map(|t| t.lines().any(|l| l.starts_with("GRUB_THEME=") && l.contains(&format!("/themes/{id}/"))))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_theme_group_only() {
        let conf = "[General]\nCurrent=nope\n\n[Theme]\n    Current=ukiyo\nCursorTheme=breeze\n[Users]\nMaximumUid=60000\n";
        assert_eq!(current_in(conf).as_deref(), Some("ukiyo"));
        assert_eq!(current_in("[General]\nNumlock=on\n"), None);
        assert_eq!(current_in("[Theme]\nCurrent=a\n[Theme]\nCurrent=b\n").as_deref(), Some("b"));
    }

    #[test]
    fn root_script_is_valid_sh() {
        let out = std::process::Command::new("sh").args(["-n", "-c", ROOT_SCRIPT]).output().unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    }
}
