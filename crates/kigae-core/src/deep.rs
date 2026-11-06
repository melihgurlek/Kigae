//! The deep switch (SPEC.md §11): the parts of a rice that live outside `$HOME` and need root,
//! the SDDM login theme and the GRUB theme. `kigae deep <id>` runs them behind one pkexec prompt.
//!
//! The login can also follow every switch: `kigae setup-login` installs (once, as root) a helper
//! that can do one thing, point SDDM at a theme already installed in /usr/share/sddm/themes, and
//! a polkit action that lets the active session run it without a password. GRUB never follows a
//! switch: it rewrites grub.cfg.

use std::path::{Path, PathBuf};

pub const SDDM_CONF: &str = "/etc/sddm.conf";
pub const SDDM_CONF_D: &str = "/etc/sddm.conf.d";
pub const SDDM_THEMES: &str = "/usr/share/sddm/themes";
pub const GRUB_DEFAULT: &str = "/etc/default/grub";

/// The helper a switch runs as `pkexec LOGIN_HELPER <theme>` once `kigae setup-login` put it here.
pub const LOGIN_HELPER: &str = "/usr/local/libexec/kigae-login";
/// The polkit action that lets the active session run it without a password.
pub const LOGIN_POLICY: &str = "/usr/share/polkit-1/actions/dev.kigae.login.policy";

/// The helper: point SDDM at an installed theme. Installed as a file root owns by `kigae
/// setup-login`, and run inline by ROOT_SCRIPT for `kigae deep`. The first change keeps a copy
/// of sddm.conf as sddm.conf.pre-kigae.
pub const LOGIN_SCRIPT: &str = r#"#!/bin/sh
# kigae-login: point SDDM at an installed login theme (installed by `kigae setup-login`).
# Runs as root through pkexec without a password, so it takes one theme name and nothing else.
set -eu
[ $# -eq 1 ] || { echo "usage: kigae-login <sddm theme>" >&2; exit 2; }
theme=$1 conf=/etc/sddm.conf
case $theme in ''|*[!A-Za-z0-9._-]*|.*) echo "bad theme name: $theme" >&2; exit 2;; esac
[ -f "/usr/share/sddm/themes/$theme/metadata.desktop" ] || { echo "SDDM theme $theme is not installed" >&2; exit 1; }
[ -e "$conf" ] || : > "$conf"
[ -e "$conf.pre-kigae" ] || cp -a "$conf" "$conf.pre-kigae"
if grep -q '^[[:space:]]*Current=' "$conf"; then
  sed -i "s/^\([[:space:]]*\)Current=.*/\1Current=$theme/" "$conf"
else
  printf '\n[Theme]\nCurrent=%s\n' "$theme" >> "$conf"
fi
"#;

pub const LOGIN_POLICY_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE policyconfig PUBLIC "-//freedesktop//DTD PolicyKit Policy Configuration 1.0//EN"
 "http://www.freedesktop.org/standards/PolicyKit/1/policyconfig.dtd">
<policyconfig>
  <vendor>Kigae</vendor>
  <action id="dev.kigae.login">
    <description>Switch the login screen to the rice's theme</description>
    <message>Kigae wants to switch the login screen theme</message>
    <defaults>
      <allow_any>auth_admin</allow_any>
      <allow_inactive>auth_admin</allow_inactive>
      <allow_active>yes</allow_active>
    </defaults>
    <annotate key="org.freedesktop.policykit.exec.path">/usr/local/libexec/kigae-login</annotate>
  </action>
</policyconfig>
"#;

/// Root's half of `kigae setup-login`, as `pkexec /bin/sh -c SETUP_SCRIPT sh <helper> <policy>`
/// (the files travel as arguments, like ROOT_SCRIPT), or with `--remove` as the only argument.
pub const SETUP_SCRIPT: &str = r#"set -eu
helper=/usr/local/libexec/kigae-login policy=/usr/share/polkit-1/actions/dev.kigae.login.policy
if [ "$1" = --remove ]; then
  rm -f "$helper" "$policy"
  echo "removed $helper and $policy"
  exit 0
fi
install -d -m 0755 /usr/local/libexec
printf '%s' "$1" > "$helper.new" && chmod 0755 "$helper.new" && mv -f "$helper.new" "$helper"
printf '%s' "$2" > "$policy.new" && chmod 0644 "$policy.new" && mv -f "$policy.new" "$policy"
echo "installed $helper and $policy"
"#;

/// The command a switch runs to point the login at `theme` (no password once set up).
pub fn login_command(theme: &str) -> Vec<String> {
    vec!["pkexec".into(), LOGIN_HELPER.into(), theme.into()]
}

/// What root runs, as `pkexec /bin/sh -c ROOT_SCRIPT sh LOGIN_SCRIPT <sddm theme> <grub script>`
/// (either may be empty). The commands travel as arguments, not as a file a user process could
/// swap, and the login half is LOGIN_SCRIPT itself, so the theme checks live in one place.
pub const ROOT_SCRIPT: &str = r#"set -eu
login=$1 theme=$2 grub=$3
if [ -n "$theme" ]; then
  sh -c "$login" kigae-login "$theme"
  echo "login: $theme"
fi
if [ -n "$grub" ]; then
  bash "$grub"
fi
"#;

/// `[Theme] Current` from an sddm.conf-style text, if it sets one.
pub fn current_in(text: &str) -> Option<String> {
    crate::kconfig::groups(text).remove("Theme")?.remove("Current")
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
    fn root_scripts_are_valid_sh() {
        for script in [ROOT_SCRIPT, LOGIN_SCRIPT, SETUP_SCRIPT] {
            let out = std::process::Command::new("sh").args(["-n", "-c", script]).output().unwrap();
            assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        }
        assert!(LOGIN_POLICY_XML.contains(LOGIN_HELPER) && SETUP_SCRIPT.contains(LOGIN_HELPER));
        assert!(SETUP_SCRIPT.contains(LOGIN_POLICY));
    }

    /// The helper refuses anything but one plain theme name, before it touches a file.
    #[test]
    fn login_helper_refuses_odd_names() {
        for bad in ["", "../etc", ".hidden", "a b", "x;rm", "a/b"] {
            let out = std::process::Command::new("sh").args(["-c", LOGIN_SCRIPT, "kigae-login", bad]).output().unwrap();
            assert_eq!(out.status.code(), Some(2), "{bad:?}");
        }
        let out = std::process::Command::new("sh").args(["-c", LOGIN_SCRIPT, "kigae-login", "a", "b"]).output().unwrap();
        assert_eq!(out.status.code(), Some(2));
        // `kigae deep` runs the same checks
        for bad in ["../etc", ".hidden", "x;rm"] {
            let out = std::process::Command::new("sh").args(["-c", ROOT_SCRIPT, "sh", LOGIN_SCRIPT, bad, ""]).output().unwrap();
            assert_eq!(out.status.code(), Some(2), "{bad:?}");
        }
    }
}
