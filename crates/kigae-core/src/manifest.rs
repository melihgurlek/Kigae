//! `rice.toml`: what one rice is made of (SPEC.md §3).
//!
//! Every section is optional; a missing part means "leave it as it is". Keys a rice sets in
//! `[kconfig]`, `[links]` and `[vscode]` are *owned* by it: when you switch away, owned keys the
//! next rice doesn't set are deleted, so nothing of the old rice lingers.
//! Unknown keys are rejected so a typo fails before the desktop is touched.

use std::collections::BTreeMap;

use serde::Deserialize;

pub const SCHEMA: u32 = 1;

/// `file → "Group/Sub/Key" → value`, e.g. `kconfig.kwinrc."Plugins/blurEnabled" = true`.
pub type KConfig = BTreeMap<String, BTreeMap<String, toml::Value>>;

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub schema: u32,
    pub id: String,
    pub name: String,
    pub kanji: Option<String>,
    pub tagline: Option<String>,
    /// Only in local manifests (`manifests/<id>.toml`), for branches that stay untouched.
    pub branch: Option<String>,
    /// Path inside the rice's branch (or `./…` next to a local manifest).
    pub preview: Option<String>,
    #[serde(default)]
    pub plasma: Plasma,
    #[serde(default)]
    pub kconfig: KConfig,
    /// Kigae-owned symlink (`~/…`) → file it points at (`~/…`).
    #[serde(default)]
    pub links: BTreeMap<String, String>,
    /// VS Code `settings.json` keys. Tables become JSON objects.
    #[serde(default)]
    pub vscode: BTreeMap<String, toml::Value>,
    pub daynight: Option<DayNight>,
    #[serde(default)]
    pub services: Services,
    #[serde(default)]
    pub hooks: Hooks,
    /// Source → `$HOME`-relative target, copied once by `kigae install`. A source is a path in
    /// the rice's branch, or `./…` next to a local manifest.
    #[serde(default)]
    pub install: BTreeMap<String, String>,
    #[serde(default)]
    pub deep: Deep,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Plasma {
    pub colors: Option<String>,
    pub style: Option<String>,
    pub icons: Option<String>,
    pub cursor: Option<String>,
    /// Panel script, run after Kigae's prelude. Path in the branch or `./…`.
    pub layout: Option<String>,
    /// `~/…` or absolute image path.
    pub wallpaper: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DayNight {
    pub light: Mood,
    pub dark: Mood,
}

/// What changes between day and night: never layout, links or services.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Mood {
    pub colors: Option<String>,
    pub style: Option<String>,
    pub icons: Option<String>,
    pub cursor: Option<String>,
    pub wallpaper: Option<String>,
    #[serde(default)]
    pub kconfig: KConfig,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoodKind {
    Light,
    Dark,
}

impl MoodKind {
    pub fn name(self) -> &'static str {
        match self {
            MoodKind::Light => "light",
            MoodKind::Dark => "dark",
        }
    }

    /// `"light"` or `"dark"`, as [`MoodKind::name`] writes it.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "light" => Some(MoodKind::Light),
            "dark" => Some(MoodKind::Dark),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Services {
    /// systemd user units enabled and started while the rice is active.
    #[serde(default)]
    pub enable: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Hooks {
    /// Shell commands run (`sh -c`) after everything else is applied.
    #[serde(default)]
    pub after: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Deep {
    pub sddm: Option<String>,
    pub grub: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum ManifestError {
    #[error("{0}")]
    Toml(#[from] toml::de::Error),
    #[error("schema {found} is not supported (this Kigae reads schema {SCHEMA})")]
    Schema { found: u32 },
    #[error("id {0:?} must be lowercase letters, digits and '-'")]
    Id(String),
    #[error("[kconfig.{file}] key {key:?} must be \"Group/Key\" (nested groups: \"A/B/Key\")")]
    KConfigKey { file: String, key: String },
    #[error("[kconfig.{file}] {key:?} must be a string, integer, float or boolean")]
    KConfigValue { file: String, key: String },
    #[error("[links] {0:?}: both sides must start with \"~/\"")]
    Link(String),
    #[error("[install] target {0:?} must be relative to $HOME, without '..'")]
    InstallTarget(String),
}

/// A rice with its day/night choice made: what actually gets applied.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Look {
    pub colors: Option<String>,
    pub style: Option<String>,
    pub icons: Option<String>,
    pub cursor: Option<String>,
    pub wallpaper: Option<String>,
    pub kconfig: KConfig,
}

impl Manifest {
    pub fn parse(text: &str) -> Result<Self, ManifestError> {
        let m: Manifest = toml::from_str(text)?;
        m.validate()?;
        Ok(m)
    }

    fn validate(&self) -> Result<(), ManifestError> {
        if self.schema != SCHEMA {
            return Err(ManifestError::Schema { found: self.schema });
        }
        let id_ok = !self.id.is_empty()
            && self
                .id
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
        if !id_ok {
            return Err(ManifestError::Id(self.id.clone()));
        }
        let moods = self.daynight.iter().flat_map(|d| [&d.light, &d.dark]);
        for kc in std::iter::once(&self.kconfig).chain(moods.map(|m| &m.kconfig)) {
            validate_kconfig(kc)?;
        }
        for (link, target) in &self.links {
            if !link.starts_with("~/") || !target.starts_with("~/") {
                return Err(ManifestError::Link(link.clone()));
            }
        }
        for target in self.install.values() {
            let bad = target.is_empty()
                || target.starts_with('/')
                || target.starts_with('~')
                || target.split('/').any(|seg| seg == "..");
            if bad {
                return Err(ManifestError::InstallTarget(target.clone()));
            }
        }
        Ok(())
    }

    /// The parts to apply, with the mood's values over the base ones.
    pub fn look(&self, mood: Option<MoodKind>) -> Look {
        let p = &self.plasma;
        let mut look = Look {
            colors: p.colors.clone(),
            style: p.style.clone(),
            icons: p.icons.clone(),
            cursor: p.cursor.clone(),
            wallpaper: p.wallpaper.clone(),
            kconfig: self.kconfig.clone(),
        };
        let m = match (&self.daynight, mood) {
            (Some(dn), Some(MoodKind::Light)) => &dn.light,
            (Some(dn), Some(MoodKind::Dark)) => &dn.dark,
            _ => return look,
        };
        let or = |mine: &Option<String>, base: Option<String>| mine.clone().or(base);
        look.colors = or(&m.colors, look.colors);
        look.style = or(&m.style, look.style);
        look.icons = or(&m.icons, look.icons);
        look.cursor = or(&m.cursor, look.cursor);
        look.wallpaper = or(&m.wallpaper, look.wallpaper);
        for (file, keys) in &m.kconfig {
            let dst = look.kconfig.entry(file.clone()).or_default();
            dst.extend(keys.iter().map(|(k, v)| (k.clone(), v.clone())));
        }
        look
    }

    /// Every kconfig key this rice can set, in either mood: what it owns.
    pub fn owned_kconfig(&self) -> Vec<(String, String)> {
        let moods = self.daynight.iter().flat_map(|d| [&d.light, &d.dark]);
        let mut out: Vec<(String, String)> = std::iter::once(&self.kconfig)
            .chain(moods.map(|m| &m.kconfig))
            .flat_map(|kc| {
                kc.iter()
                    .flat_map(|(f, keys)| keys.keys().map(move |k| (f.clone(), k.clone())))
            })
            .collect();
        out.sort();
        out.dedup();
        out
    }
}

/// What an app looks like when a rice doesn't theme it (`[unset]` in config.toml).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Unset {
    /// Your `base.toml`, then the app's stock look.
    #[default]
    Base,
    /// The app's stock look: the previous rice's keys are deleted.
    Stock,
    /// Whatever the previous rice left.
    Keep,
}

/// `[unset]` per section.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct UnsetPolicy {
    pub kconfig: Unset,
    pub links: Unset,
    pub vscode: Unset,
}

impl Manifest {
    /// This rice with `base` filling the sections whose policy is [`Unset::Base`] (the rice
    /// wins key by key). Plasma parts the rice leaves out come from `base` too.
    pub fn with_base(&self, base: Option<&Manifest>, policy: UnsetPolicy) -> Manifest {
        let mut m = self.clone();
        let Some(base) = base else { return m };
        let p = &mut m.plasma;
        let b = &base.plasma;
        for (mine, theirs) in [
            (&mut p.colors, &b.colors),
            (&mut p.style, &b.style),
            (&mut p.icons, &b.icons),
            (&mut p.cursor, &b.cursor),
            (&mut p.wallpaper, &b.wallpaper),
        ] {
            if mine.is_none() {
                mine.clone_from(theirs);
            }
        }
        if policy.kconfig == Unset::Base {
            for (file, keys) in &base.kconfig {
                let dst = m.kconfig.entry(file.clone()).or_default();
                for (k, v) in keys {
                    // a mood setting the key counts as the rice setting it
                    let in_mood = m.daynight.iter().any(|d| {
                        [&d.light, &d.dark].iter().any(|md| md.kconfig.get(file).is_some_and(|f| f.contains_key(k)))
                    });
                    if !in_mood {
                        dst.entry(k.clone()).or_insert_with(|| v.clone());
                    }
                }
            }
        }
        if policy.links == Unset::Base {
            for (k, v) in &base.links {
                m.links.entry(k.clone()).or_insert_with(|| v.clone());
            }
        }
        if policy.vscode == Unset::Base {
            for (k, v) in &base.vscode {
                m.vscode.entry(k.clone()).or_insert_with(|| v.clone());
            }
        }
        m
    }
}

fn validate_kconfig(kc: &KConfig) -> Result<(), ManifestError> {
    for (file, keys) in kc {
        for (key, value) in keys {
            if kconfig_key(key).is_none() {
                return Err(ManifestError::KConfigKey { file: file.clone(), key: key.clone() });
            }
            if kconfig_value(value).is_none() {
                return Err(ManifestError::KConfigValue { file: file.clone(), key: key.clone() });
            }
        }
    }
    Ok(())
}

/// Split `"Group/Sub/Key"` into (`["Group", "Sub"]`, `"Key"`).
pub fn kconfig_key(key: &str) -> Option<(Vec<&str>, &str)> {
    let (groups, k) = key.rsplit_once('/')?;
    let groups: Vec<&str> = groups.split('/').collect();
    (!k.is_empty() && groups.iter().all(|g| !g.is_empty())).then_some((groups, k))
}

/// A kconfig value as written to the file, and whether it's a boolean.
pub fn kconfig_value(v: &toml::Value) -> Option<(String, bool)> {
    match v {
        toml::Value::String(s) => Some((s.clone(), false)),
        toml::Value::Integer(i) => Some((i.to_string(), false)),
        toml::Value::Float(f) => Some((f.to_string(), false)),
        toml::Value::Boolean(b) => Some((b.to_string(), true)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub const UKIYO: &str = r##"
schema = 1
id = "ukiyo"
name = "Ukiyo"
kanji = "浮世"
tagline = "ink & paper, floating islands"
preview = "ukiyo/screens/desktop.jpg"

[plasma]
colors = "Ukiyo"
style = "ukiyo"
icons = "Colloid-Yellow-Everforest-Dark"
cursor = "breeze_cursors"
layout = "ukiyo/kigae-layout.js"

[kconfig.kwinrc]
"Plugins/fadeEnabled" = true
"Effect-blur/BlurStrength" = 9

[kconfig.konsolerc]
"Desktop Entry/DefaultProfile" = "Ukiyo.profile"

[links]
"~/.config/kitty/kigae-current.conf" = "~/.config/kitty/ukiyo.conf"

[vscode]
"workbench.colorTheme" = "Kanagawa"
"workbench.colorCustomizations" = { "[Kanagawa]" = { "titleBar.border" = "#16161D" } }

[services]
enable = ["ukiyo-sky.timer"]

[hooks]
after = ["~/.local/bin/ukiyo-sky --force"]

[install]
"home/.local/share/plasma/desktoptheme/ukiyo" = ".local/share/plasma/desktoptheme/ukiyo"

[deep]
sddm = "ukiyo"
"##;

    const OG: &str = r#"schema = 1
id = "og"
name = "Scratchy & Itchy"
branch = "backup/og"
[plasma]
icons = "Papirus"
[kconfig.kwinrc]
"org.kde.kdecoration2/library" = "org.kde.kwin.aurorae"
[daynight.light]
colors = "Itchy"
style = "Itchy"
kconfig.kwinrc."org.kde.kdecoration2/theme" = "__aurorae__svg__Itchy"
[daynight.dark]
colors = "Scratchy"
style = "Scratchy"
kconfig.kwinrc."org.kde.kdecoration2/theme" = "__aurorae__svg__Scratchy"
"#;

    #[test]
    fn parses_full_manifest() {
        let m = Manifest::parse(UKIYO).unwrap();
        assert_eq!(m.id, "ukiyo");
        assert_eq!(m.plasma.style.as_deref(), Some("ukiyo"));
        assert_eq!(m.kconfig["kwinrc"]["Effect-blur/BlurStrength"], toml::Value::Integer(9));
        assert_eq!(m.links.len(), 1);
        assert!(m.vscode["workbench.colorCustomizations"].is_table());
        assert_eq!(m.services.enable, ["ukiyo-sky.timer"]);
        assert_eq!(m.hooks.after.len(), 1);
        assert!(m.daynight.is_none());
    }

    #[test]
    fn minimal_manifest_leaves_everything_alone() {
        let m = Manifest::parse("schema = 1\nid = \"bare\"\nname = \"Bare\"").unwrap();
        assert_eq!(m.look(None), Look::default());
        assert!(m.owned_kconfig().is_empty() && m.links.is_empty() && m.vscode.is_empty());
    }

    #[test]
    fn mood_overrides_base() {
        let m = Manifest::parse(OG).unwrap();
        let light = m.look(Some(MoodKind::Light));
        assert_eq!(light.colors.as_deref(), Some("Itchy"));
        assert_eq!(light.icons.as_deref(), Some("Papirus"));
        let kw = &light.kconfig["kwinrc"];
        assert_eq!(kw["org.kde.kdecoration2/library"].as_str(), Some("org.kde.kwin.aurorae"));
        assert_eq!(kw["org.kde.kdecoration2/theme"].as_str(), Some("__aurorae__svg__Itchy"));
        let dark = m.look(Some(MoodKind::Dark));
        assert_eq!(dark.style.as_deref(), Some("Scratchy"));
        // no mood picked: base only
        assert_eq!(m.look(None).colors, None);
    }

    #[test]
    fn owns_keys_of_both_moods() {
        let m = Manifest::parse(OG).unwrap();
        let owned = m.owned_kconfig();
        assert_eq!(
            owned,
            [
                ("kwinrc".into(), "org.kde.kdecoration2/library".into()),
                ("kwinrc".into(), "org.kde.kdecoration2/theme".into()),
            ]
        );
    }

    #[test]
    fn rejects_unknown_key() {
        let err = Manifest::parse(&UKIYO.replace("[plasma]\n", "[plasma]\ncolour = \"x\"\n"))
            .unwrap_err();
        assert!(matches!(err, ManifestError::Toml(_)), "{err}");
        assert!(err.to_string().contains("colour"), "{err}");
    }

    #[test]
    fn rejects_other_schema() {
        let err = Manifest::parse(&UKIYO.replace("schema = 1", "schema = 2")).unwrap_err();
        assert!(matches!(err, ManifestError::Schema { found: 2 }));
    }

    #[test]
    fn rejects_bad_id() {
        let err =
            Manifest::parse(&UKIYO.replace("id = \"ukiyo\"", "id = \"Ukiyo Rice\"")).unwrap_err();
        assert!(matches!(err, ManifestError::Id(_)));
    }

    #[test]
    fn rejects_kconfig_key_without_group() {
        let err =
            Manifest::parse(&UKIYO.replace("\"Plugins/fadeEnabled\"", "fadeEnabled")).unwrap_err();
        assert!(matches!(err, ManifestError::KConfigKey { .. }), "{err}");
    }

    #[test]
    fn rejects_kconfig_table_value() {
        let err = Manifest::parse(&UKIYO.replace("= 9", "= { a = 1 }")).unwrap_err();
        assert!(matches!(err, ManifestError::KConfigValue { .. }), "{err}");
    }

    #[test]
    fn rejects_link_outside_home() {
        let err = Manifest::parse(&UKIYO.replace("= \"~/.config/kitty/ukiyo.conf\"", "= \"/etc/x\""))
            .unwrap_err();
        assert!(matches!(err, ManifestError::Link(_)), "{err}");
    }

    #[test]
    fn rejects_install_target_escaping_home() {
        for target in ["/etc/x", "~/.local/x", ".local/../../etc", ""] {
            let text = UKIYO.replace(
                "= \".local/share/plasma/desktoptheme/ukiyo\"",
                &format!("= {target:?}"),
            );
            let err = Manifest::parse(&text).unwrap_err();
            assert!(matches!(err, ManifestError::InstallTarget(_)), "{target}: {err}");
        }
    }

    #[test]
    fn base_fills_what_the_rice_leaves_out() {
        let base = Manifest::parse(
            r#"schema = 1
id = "base"
name = "Base"
[plasma]
cursor = "WhiteSur-cursors"
colors = "BreezeDark"
[kconfig.kwinrc]
"Plugins/fadeEnabled" = false
"org.kde.kdecoration2/theme" = "Breeze"
[links]
"~/.config/kitty/kigae-current.conf" = "~/.config/kitty/base.conf"
"~/.config/kigae/p10k.zsh" = "~/.p10k.zsh"
[vscode]
"workbench.colorTheme" = "Default Dark Modern"
"workbench.iconTheme" = "vs-seti"
"#,
        )
        .unwrap();
        let rice = Manifest::parse(UKIYO).unwrap();
        let all_base = UnsetPolicy::default();
        let m = rice.with_base(Some(&base), all_base);
        assert_eq!(m.plasma.colors.as_deref(), Some("Ukiyo"), "rice wins");
        assert_eq!(m.plasma.cursor.as_deref(), Some("breeze_cursors"));
        assert_eq!(m.kconfig["kwinrc"]["Plugins/fadeEnabled"], toml::Value::Boolean(true));
        assert_eq!(m.kconfig["kwinrc"]["org.kde.kdecoration2/theme"].as_str(), Some("Breeze"));
        assert_eq!(m.links["~/.config/kitty/kigae-current.conf"], "~/.config/kitty/ukiyo.conf");
        assert_eq!(m.links["~/.config/kigae/p10k.zsh"], "~/.p10k.zsh");
        assert_eq!(m.vscode["workbench.colorTheme"].as_str(), Some("Kanagawa"));
        assert_eq!(m.vscode["workbench.iconTheme"].as_str(), Some("vs-seti"));

        // a mood's key beats the base, so day/night still switches it
        let og = Manifest::parse(OG).unwrap().with_base(Some(&base), all_base);
        assert!(!og.kconfig["kwinrc"].contains_key("org.kde.kdecoration2/theme"));
        let dark = og.look(Some(MoodKind::Dark));
        assert_eq!(dark.kconfig["kwinrc"]["org.kde.kdecoration2/theme"].as_str(), Some("__aurorae__svg__Scratchy"));

        // keep/stock sections take nothing from the base
        let keep = UnsetPolicy { vscode: Unset::Keep, links: Unset::Stock, ..all_base };
        let m = rice.with_base(Some(&base), keep);
        assert!(!m.vscode.contains_key("workbench.iconTheme"));
        assert!(!m.links.contains_key("~/.config/kigae/p10k.zsh"));
    }

    #[test]
    fn splits_kconfig_keys() {
        assert_eq!(kconfig_key("Plugins/blurEnabled"), Some((vec!["Plugins"], "blurEnabled")));
        assert_eq!(kconfig_key("A/B/Key"), Some((vec!["A", "B"], "Key")));
        assert_eq!(kconfig_key("noslash"), None);
        assert_eq!(kconfig_key("/x"), None);
        assert_eq!(kconfig_key("A//x"), None);
    }
}
