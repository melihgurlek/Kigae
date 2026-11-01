//! `rice.toml`: what one rice is made of (SPEC.md §3).
//!
//! Every section is optional; a missing part means "leave it as it is".
//! Unknown keys are rejected so a typo fails before the desktop is touched.

use std::collections::BTreeMap;

use serde::Deserialize;

pub const SCHEMA: u32 = 1;

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub schema: u32,
    pub id: String,
    pub name: String,
    pub kanji: Option<String>,
    pub tagline: Option<String>,
    /// Only in local manifests (`manifests/<id>.toml`) for branches that stay untouched.
    pub branch: Option<String>,
    /// Path inside the rice's branch.
    pub preview: Option<String>,
    #[serde(default)]
    pub plasma: Plasma,
    /// `"Group/Key" = value`, written to kwinrc.
    #[serde(default)]
    pub kwin: BTreeMap<String, toml::Value>,
    pub daynight: Option<DayNight>,
    #[serde(default)]
    pub apps: Apps,
    #[serde(default)]
    pub services: Services,
    /// Branch path → `$HOME`-relative path, copied once by `kigae install`.
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
    pub decoration: Option<Decoration>,
    /// Path inside the rice's branch; run after `data/prelude.js`.
    pub layout: Option<String>,
    pub wallpaper: Option<String>,
    pub splash: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Decoration {
    pub library: String,
    pub theme: String,
    pub borders: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DayNight {
    pub light: Mood,
    pub dark: Mood,
}

/// The parts that change between day and night. No layout, no services.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Mood {
    pub colors: Option<String>,
    pub style: Option<String>,
    pub icons: Option<String>,
    pub wallpaper: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Apps {
    pub kitty: Option<String>,
    pub p10k: Option<String>,
    pub vscode: Option<VsCode>,
    pub konsole: Option<String>,
    pub kate: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VsCode {
    pub theme: String,
    /// Path inside the rice's branch to a JSON object for `workbench.colorCustomizations`.
    pub customizations: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Services {
    #[serde(default)]
    pub enable: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Deep {
    pub sddm: Option<String>,
    pub grub: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum ManifestError {
    #[error("rice.toml: {0}")]
    Toml(#[from] toml::de::Error),
    #[error("rice.toml: schema {found} is not supported (this Kigae reads schema {SCHEMA})")]
    Schema { found: u32 },
    #[error("rice.toml: id {0:?} must be lowercase letters, digits and '-'")]
    Id(String),
    #[error("rice.toml: [kwin] key {0:?} must be \"Group/Key\"")]
    KwinKey(String),
    #[error("rice.toml: [kwin] {key:?} must be a string, integer, float or boolean")]
    KwinValue { key: String },
    #[error("rice.toml: [install] target {0:?} must be relative to $HOME, without '..'")]
    InstallTarget(String),
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
        for (key, value) in &self.kwin {
            if kwin_key(key).is_none() {
                return Err(ManifestError::KwinKey(key.clone()));
            }
            if !matches!(
                value,
                toml::Value::String(_)
                    | toml::Value::Integer(_)
                    | toml::Value::Float(_)
                    | toml::Value::Boolean(_)
            ) {
                return Err(ManifestError::KwinValue { key: key.clone() });
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
}

/// Split a `[kwin]` key into (group, key). Nested groups use further slashes:
/// `"Effect-blur/BlurStrength"` → `("Effect-blur", "BlurStrength")`.
pub fn kwin_key(key: &str) -> Option<(&str, &str)> {
    let (group, k) = key.rsplit_once('/')?;
    (!group.is_empty() && !k.is_empty()).then_some((group, k))
}

#[cfg(test)]
mod tests {
    use super::*;

    const UKIYO: &str = r#"
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
decoration = { library = "org.kde.breeze", theme = "Breeze", borders = "None" }
layout = "ukiyo/kigae-layout.js"
splash = "org.ukiyo.desktop"

[kwin]
"Plugins/fadeEnabled" = true
"Effect-blur/BlurStrength" = 9

[apps]
kitty = "~/.config/kitty/themes/ukiyo.conf"
vscode = { theme = "Kanagawa" }
konsole = "Ukiyo"

[services]
enable = ["ukiyo-sky.timer"]

[install]
"home/.local/share/plasma/desktoptheme/ukiyo" = ".local/share/plasma/desktoptheme/ukiyo"

[deep]
sddm = "ukiyo"
"#;

    #[test]
    fn parses_full_manifest() {
        let m = Manifest::parse(UKIYO).unwrap();
        assert_eq!(m.id, "ukiyo");
        assert_eq!(m.plasma.style.as_deref(), Some("ukiyo"));
        assert_eq!(m.plasma.decoration.unwrap().borders.as_deref(), Some("None"));
        assert_eq!(m.kwin["Effect-blur/BlurStrength"], toml::Value::Integer(9));
        assert_eq!(m.apps.vscode.unwrap().theme, "Kanagawa");
        assert_eq!(m.services.enable, ["ukiyo-sky.timer"]);
        assert!(m.daynight.is_none());
    }

    #[test]
    fn minimal_manifest_leaves_everything_alone() {
        let m = Manifest::parse("schema = 1\nid = \"bare\"\nname = \"Bare\"").unwrap();
        assert_eq!(m.plasma, Plasma::default());
        assert!(m.kwin.is_empty() && m.install.is_empty() && m.services.enable.is_empty());
    }

    #[test]
    fn parses_daynight() {
        let m = Manifest::parse(
            r#"schema = 1
id = "og"
name = "Scratchy & Itchy"
branch = "backup/og"
[daynight]
light = { colors = "Itchy", style = "Itchy" }
dark = { colors = "Scratchy", style = "Scratchy" }"#,
        )
        .unwrap();
        let dn = m.daynight.unwrap();
        assert_eq!(dn.light.colors.as_deref(), Some("Itchy"));
        assert_eq!(dn.dark.style.as_deref(), Some("Scratchy"));
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
        let err = Manifest::parse(&UKIYO.replace("id = \"ukiyo\"", "id = \"Ukiyo Rice\"")).unwrap_err();
        assert!(matches!(err, ManifestError::Id(_)));
    }

    #[test]
    fn rejects_kwin_key_without_group() {
        let err = Manifest::parse(&UKIYO.replace("\"Plugins/fadeEnabled\"", "fadeEnabled")).unwrap_err();
        assert!(matches!(err, ManifestError::KwinKey(_)));
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
    fn splits_kwin_keys() {
        assert_eq!(kwin_key("Plugins/blurEnabled"), Some(("Plugins", "blurEnabled")));
        assert_eq!(kwin_key("noslash"), None);
        assert_eq!(kwin_key("/x"), None);
    }
}
