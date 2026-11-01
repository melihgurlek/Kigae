//! `kigae capture`: read the rice on screen into a manifest and a panel script, so any rice can
//! be switched without writing either by hand (PLAN.md phase 1b).

use std::collections::BTreeMap;
use std::path::Path;

use crate::engine::{Paths, file_url_path};
use crate::manifest::Manifest;
use crate::sys::{FileState, Sys, SysError};

/// Which KConfig keys make up a rice's look. `(file, group, keys)`; a group ending in `*`
/// matches any group with that prefix, and empty `keys` means every key in the group.
const LOOK_KEYS: &[(&str, &str, &[&str])] = &[
    ("kwinrc", "org.kde.kdecoration2", &[]),
    ("kwinrc", "Plugins", &[]),
    ("kwinrc", "Effect-*", &[]),
    ("kdeglobals", "KDE", &["widgetStyle"]),
    ("kdeglobals", "General", &["font", "fixed", "smallestReadableFont", "toolBarFont", "menuFont", "AccentColor", "accentColorFromWallpaper"]),
    ("kdeglobals", "WM", &["activeFont"]),
    ("kcminputrc", "Mouse", &["cursorSize"]),
    ("ksplashrc", "KSplash", &["Engine", "Theme"]),
    ("konsolerc", "Desktop Entry", &["DefaultProfile"]),
    ("katerc", "KTextEditor Renderer", &["Color Theme"]),
    ("kwriterc", "KTextEditor Renderer", &["Color Theme"]),
];

/// VS Code settings that are part of a look.
const VSCODE_KEYS: &[&str] = &[
    "workbench.colorTheme",
    "workbench.iconTheme",
    "workbench.productIconTheme",
    "window.autoDetectColorScheme",
    "workbench.preferredDarkColorTheme",
    "workbench.preferredLightColorTheme",
    "workbench.colorCustomizations",
    "editor.tokenColorCustomizations",
];

/// Links Kigae knows how to point; captured when they exist.
const KNOWN_LINKS: &[&str] = &[
    "~/.config/kitty/kigae-current.conf",
    "~/.config/kitty/dark-theme.auto.conf",
    "~/.config/kitty/light-theme.auto.conf",
    "~/.config/kigae/p10k.zsh",
    "~/.config/nvim/lua/plugins/kigae.lua",
];

#[derive(Debug, Clone, Default)]
pub struct Info {
    pub id: String,
    pub name: Option<String>,
    pub kanji: Option<String>,
    pub tagline: Option<String>,
    pub branch: Option<String>,
}

#[derive(Debug)]
pub struct Captured {
    /// `manifests/<id>.toml`
    pub manifest: String,
    /// `manifests/<id>/layout.js`
    pub layout: String,
    /// What the user should look at or add by hand.
    pub notes: Vec<String>,
}

/// Parse a KConfig file into `"Group/Sub" → key → value`, skipping localized and `$`-flagged keys.
pub fn parse_kconfig(text: &str) -> BTreeMap<String, BTreeMap<String, String>> {
    let mut out: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
    let mut group: Option<String> = None;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') {
            let parts: Vec<&str> = line.trim_start_matches('[').trim_end_matches(']').split("][").collect();
            group = (!parts.iter().any(|p| p.starts_with('$'))).then(|| parts.join("/"));
            continue;
        }
        let (Some(g), Some((k, v))) = (&group, line.split_once('=')) else { continue };
        if k.contains('[') {
            continue;
        }
        out.entry(g.clone()).or_default().insert(k.trim().to_string(), v.to_string());
    }
    out
}

fn kconfig_toml(v: &str) -> toml::Value {
    match v {
        "true" => toml::Value::Boolean(true),
        "false" => toml::Value::Boolean(false),
        _ => toml::Value::String(v.to_string()),
    }
}

fn tilde(paths: &Paths, p: &Path) -> String {
    match p.strip_prefix(&paths.home) {
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => p.display().to_string(),
    }
}

fn json_to_toml(v: &serde_json::Value) -> Option<toml::Value> {
    Some(match v {
        serde_json::Value::Null => return None,
        serde_json::Value::Bool(b) => toml::Value::Boolean(*b),
        serde_json::Value::Number(n) => match n.as_i64() {
            Some(i) => toml::Value::Integer(i),
            None => toml::Value::Float(n.as_f64()?),
        },
        serde_json::Value::String(s) => toml::Value::String(s.clone()),
        serde_json::Value::Array(a) => toml::Value::Array(a.iter().filter_map(json_to_toml).collect()),
        serde_json::Value::Object(o) => {
            toml::Value::Table(o.iter().filter_map(|(k, v)| Some((k.clone(), json_to_toml(v)?))).collect())
        }
    })
}

fn kread(sys: &mut impl Sys, file: &str, group: &str, key: &str) -> Result<Option<String>, SysError> {
    sys.kread(file, &[group.into()], key)
}

/// Read the rice on screen. Values equal to `base` are left out: they apply under every rice.
pub fn capture<S: Sys>(sys: &mut S, paths: &Paths, info: &Info, base: Option<&Manifest>) -> Result<Captured, SysError> {
    let empty = Manifest::parse("schema = 1\nid = \"base\"\nname = \"base\"").expect("valid");
    let base = base.unwrap_or(&empty);
    let mut notes = vec![];
    let mut doc = toml::Table::new();
    doc.insert("schema".into(), 1.into());
    doc.insert("id".into(), info.id.clone().into());
    doc.insert("name".into(), info.name.clone().unwrap_or_else(|| info.id.clone()).into());
    for (k, v) in [("kanji", &info.kanji), ("tagline", &info.tagline), ("branch", &info.branch)] {
        if let Some(v) = v {
            doc.insert(k.into(), v.clone().into());
        }
    }

    // plasma parts
    let mut plasma = toml::Table::new();
    for (key, file, group, k) in [
        ("colors", "kdeglobals", "General", "ColorScheme"),
        ("style", "plasmarc", "Theme", "name"),
        ("icons", "kdeglobals", "Icons", "Theme"),
        ("cursor", "kcminputrc", "Mouse", "cursorTheme"),
    ] {
        let in_base = match key {
            "colors" => &base.plasma.colors,
            "style" => &base.plasma.style,
            "icons" => &base.plasma.icons,
            _ => &base.plasma.cursor,
        };
        if let Some(v) = kread(sys, file, group, k)?
            && in_base.as_ref() != Some(&v)
        {
            plasma.insert(key.into(), v.into());
        }
    }
    plasma.insert("layout".into(), format!("./{}/layout.js", info.id).into());
    match sys.wallpaper()? {
        Some(url) => {
            plasma.insert("wallpaper".into(), tilde(paths, &file_url_path(&url)).into());
        }
        None => notes.push("the wallpaper isn't a plain image (slideshow or plugin); the layout keeps it".into()),
    }
    doc.insert("plasma".into(), plasma.into());

    // look keys from KConfig files
    let mut kconfig = toml::Table::new();
    let mut files: BTreeMap<&str, BTreeMap<String, BTreeMap<String, String>>> = BTreeMap::new();
    for (file, group, keys) in LOOK_KEYS {
        let parsed = match files.get(file) {
            Some(p) => p,
            None => {
                let text = match sys.file_state(&paths.home.join(".config").join(file))? {
                    FileState::Content(t) => t,
                    _ => String::new(),
                };
                files.entry(file).or_insert(parse_kconfig(&text))
            }
        };
        let groups = parsed.iter().filter(|(g, _)| match group.strip_suffix('*') {
            Some(prefix) => g.starts_with(prefix),
            None => g == group,
        });
        let mut found: Vec<(String, toml::Value)> = vec![];
        for (g, entries) in groups {
            for (k, v) in entries {
                let key = format!("{g}/{k}");
                let value = kconfig_toml(v);
                let same_as_base = base.kconfig.get(*file).and_then(|f| f.get(&key)).is_some_and(|b| {
                    crate::manifest::kconfig_value(b).map(|(s, _)| s) == crate::manifest::kconfig_value(&value).map(|(s, _)| s)
                });
                if (keys.is_empty() || keys.contains(&k.as_str())) && !same_as_base {
                    found.push((key, value));
                }
            }
        }
        if !found.is_empty() {
            let t = kconfig.entry(file.to_string()).or_insert_with(|| toml::Table::new().into());
            let t = t.as_table_mut().expect("table");
            t.extend(found);
        }
    }
    if !kconfig.is_empty() {
        doc.insert("kconfig".into(), kconfig.into());
    }

    // links Kigae owns, where they exist
    let mut links = toml::Table::new();
    for link in KNOWN_LINKS {
        match sys.file_state(&paths.expand(link))? {
            FileState::Link(target) => {
                let target = if target.is_absolute() {
                    target
                } else {
                    paths.expand(link).parent().map(|d| d.join(&target)).unwrap_or(target)
                };
                let target = tilde(paths, &target);
                if base.links.get(*link) != Some(&target) {
                    links.insert(link.to_string(), target.into());
                }
            }
            FileState::Content(_) => notes.push(format!(
                "{link} is a plain file, not a Kigae link: move it next to the rice's files and add it to [links]"
            )),
            FileState::Missing => {}
        }
    }
    if !links.is_empty() {
        doc.insert("links".into(), links.into());
    }

    // VS Code theme keys
    if let FileState::Content(text) = sys.file_state(&paths.home.join(".config/Code/User/settings.json"))? {
        match jsonc_parser::parse_to_serde_value::<serde_json::Map<String, serde_json::Value>>(&text, &Default::default()) {
            Ok(settings) => {
                let mut vs: toml::Table = VSCODE_KEYS
                    .iter()
                    .filter_map(|k| Some((k.to_string(), json_to_toml(settings.get(*k)?)?)))
                    .collect();
                // the icon theme's own settings ("material-icon-theme.saturation", ...)
                if let Some(icons) = settings.get("workbench.iconTheme").and_then(|v| v.as_str()) {
                    let prefix = format!("{icons}.");
                    for (k, v) in settings.iter().filter(|(k, _)| k.starts_with(&prefix)) {
                        if let Some(v) = json_to_toml(v) {
                            vs.insert(k.clone(), v);
                        }
                    }
                }
                vs.retain(|k, v| base.vscode.get(k) != Some(v));
                if !vs.is_empty() {
                    doc.insert("vscode".into(), vs.into());
                }
            }
            Err(e) => notes.push(format!("VS Code settings.json didn't parse ({e}); [vscode] left out")),
        }
    }

    // panels: the dump, rewritten to run after the prelude, plus what it leaves out
    let dump = sys.dump_layout()?;
    let state = sys.capture_layout()?;
    let layout = layout_script(&info.id, &dump, &state)
        .map_err(|msg| SysError::Command { cmd: "capture layout".into(), msg })?;

    notes.push("services and hooks can't be seen from the desktop: add [services] / [hooks] if the rice has them".into());
    notes.push("for a rice that changes by day and night, add [daynight] (capture at both times and compare)".into());

    let header = format!(
        "# {} for Kigae, captured from the live desktop by `kigae capture {}`.\n\
         # Everything below was on screen; trim what isn't part of the rice.\n",
        info.name.as_deref().unwrap_or(&info.id),
        info.id
    );
    let body = toml::to_string(&doc).map_err(|e| SysError::Command { cmd: "capture".into(), msg: e.to_string() })?;
    Ok(Captured { manifest: header + &body, layout, notes })
}

/// Turn a `dumpCurrentLayoutJS` script into a rice layout script.
pub fn layout_script(id: &str, dump: &str, state: &str) -> Result<String, String> {
    const LOAD: &str = "plasma.loadSerializedLayout(layout);";
    if !dump.contains(LOAD) {
        return Err("the layout dump doesn't end in loadSerializedLayout".into());
    }
    let st: serde_json::Value = serde_json::from_str(state).map_err(|e| format!("layout state: {e}"))?;
    let mut tail = String::from(
        "// Desktop widgets come back through kigae.desktop() below (the folder view can't place them\n\
         // here), and the dock keeps whatever launchers you have now.\n\
         layout.desktops.forEach(function (d) { d.applets = []; });\n\
         layout.panels.forEach(function (p) {\n\
         \x20   p.applets.forEach(function (a) {\n\
         \x20       if (kigae.launchers && (a.plugin == \"org.kde.plasma.icontasks\" || a.plugin == \"org.kde.plasma.taskmanager\")) {\n\
         \x20           a.config[\"/General\"] = a.config[\"/General\"] || {};\n\
         \x20           a.config[\"/General\"].launchers = kigae.launchers;\n\
         \x20       }\n\
         \x20   });\n\
         });\n",
    );
    tail.push_str(LOAD);
    tail.push('\n');
    let mut extra = String::new();
    for tray in st["trays"].as_array().into_iter().flatten() {
        extra.push_str(&format!("kigae.tray(null, {tray});\n"));
    }
    for d in st["desk"].as_array().into_iter().flatten() {
        let r: Vec<String> = d["rect"].as_array().into_iter().flatten().map(|v| v.to_string()).collect();
        let [x, y, w, h] = r.as_slice() else { continue };
        let ty = &d["type"];
        let cfg = if d["config"].as_object().is_some_and(|o| !o.is_empty()) { format!(", {}", d["config"]) } else { String::new() };
        extra.push_str(&format!("kigae.desktop({ty}, {x}, {y}, {w}, {h}{cfg});\n"));
    }
    if !extra.is_empty() {
        tail.push_str("\n// what the dump leaves out, applied once the panels exist\n");
        tail.push_str(&extra);
    }
    Ok(format!(
        "// {id} panels for Kigae, captured from the live desktop by `kigae capture {id}`.\n\
         // Runs after Kigae's prelude (no panels, kigae.launchers set).\n{}",
        dump.replacen(LOAD, &tail, 1)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::Manifest;
    use crate::sys::FakeSys;

    const KWINRC: &str = "[Desktops]\nNumber=1\n\n[Effect-blur]\nBlurStrength=9\n\n[Plugins]\nfadeEnabled=true\n\n[Tiling][abc]\npadding=4\n\n[org.kde.kdecoration2]\nlibrary=org.kde.breeze\nBorderSize=None\nButtonsOnLeft=\n";
    const KDEGLOBALS: &str = "[General]\nColorScheme=Ukiyo\nfont=Inter,10,-1\nName[de]=x\n\n[KDE]\nwidgetStyle=Breeze\nSingleClick=false\n\n[Colors:View]\nBackgroundNormal=1,2,3\n";
    const DUMP: &str = "var plasma = getApiVersion(1);\n\nvar layout = {\"desktops\": [], \"panels\": []}\n;\n\nplasma.loadSerializedLayout(layout);";
    const STATE: &str = r#"{"trays":[{"General":{"hiddenItems":"a,b"}}],"desk":[{"type":"org.ukiyo.clock","rect":[1216,368,528,240],"config":{}}]}"#;

    fn desktop() -> (FakeSys, Paths) {
        let paths = Paths { home: "/h".into(), state: "/h/.local/state/kigae".into() };
        let mut s = FakeSys::default();
        s.set_kconfig("kdeglobals", "General", "ColorScheme", "Ukiyo");
        s.set_kconfig("plasmarc", "Theme", "name", "ukiyo");
        s.set_kconfig("kdeglobals", "Icons", "Theme", "Papirus");
        s.set_kconfig("kcminputrc", "Mouse", "cursorTheme", "breeze_cursors");
        s.files.insert("/h/.config/kwinrc".into(), FileState::Content(KWINRC.into()));
        s.files.insert("/h/.config/kdeglobals".into(), FileState::Content(KDEGLOBALS.into()));
        s.files.insert("/h/.config/kitty/kigae-current.conf".into(), FileState::Link("/h/.config/kitty/ukiyo.conf".into()));
        s.files.insert("/h/.config/kitty/dark-theme.auto.conf".into(), FileState::Content("include x.conf".into()));
        s.files.insert(
            "/h/.config/Code/User/settings.json".into(),
            FileState::Content("{\n // c\n \"workbench.colorTheme\": \"Kanagawa\",\n \"workbench.iconTheme\": \"mat\",\n \"mat.saturation\": 0.45,\n \"editor.fontSize\": 14,\n \"workbench.colorCustomizations\": {\"[K]\": {\"a\": \"#000\"}}\n}".into()),
        );
        s.wallpaper = Some("file:///h/Pictures/wave%20night.jpg".into());
        s.layout = DUMP.into();
        s.layout_state = STATE.into();
        (s, paths)
    }

    #[test]
    fn captures_a_manifest_that_parses() {
        let (mut sys, paths) = desktop();
        let info = Info { id: "ukiyo-live".into(), kanji: Some("浮世".into()), ..Default::default() };
        let c = capture(&mut sys, &paths, &info, None).unwrap();
        let m = Manifest::parse(&c.manifest).unwrap_or_else(|e| panic!("{e}\n{}", c.manifest));
        assert_eq!(m.id, "ukiyo-live");
        assert_eq!(m.name, "ukiyo-live");
        assert_eq!(m.plasma.colors.as_deref(), Some("Ukiyo"));
        assert_eq!(m.plasma.style.as_deref(), Some("ukiyo"));
        assert_eq!(m.plasma.wallpaper.as_deref(), Some("~/Pictures/wave night.jpg"));
        assert_eq!(m.plasma.layout.as_deref(), Some("./ukiyo-live/layout.js"));
        let kw = &m.kconfig["kwinrc"];
        assert_eq!(kw["Effect-blur/BlurStrength"].as_str(), Some("9"));
        assert_eq!(kw["Plugins/fadeEnabled"], toml::Value::Boolean(true));
        assert_eq!(kw["org.kde.kdecoration2/ButtonsOnLeft"].as_str(), Some(""));
        assert!(!kw.contains_key("Desktops/Number") && !kw.contains_key("Tiling/abc/padding"), "{kw:?}");
        let kg = &m.kconfig["kdeglobals"];
        assert_eq!(kg["General/font"].as_str(), Some("Inter,10,-1"));
        assert_eq!(kg["KDE/widgetStyle"].as_str(), Some("Breeze"));
        assert!(!kg.contains_key("General/ColorScheme") && !kg.contains_key("KDE/SingleClick"), "{kg:?}");
        assert_eq!(m.links["~/.config/kitty/kigae-current.conf"], "~/.config/kitty/ukiyo.conf");
        assert!(!m.links.contains_key("~/.config/kitty/dark-theme.auto.conf"));
        assert!(c.notes.iter().any(|n| n.contains("dark-theme.auto.conf is a plain file")), "{:?}", c.notes);
        assert_eq!(m.vscode["workbench.colorTheme"].as_str(), Some("Kanagawa"));
        assert!(m.vscode["workbench.colorCustomizations"].is_table());
        assert!(!m.vscode.contains_key("editor.fontSize"));
        assert_eq!(m.vscode["mat.saturation"].as_float(), Some(0.45), "icon theme settings come along");
    }

    #[test]
    fn leaves_out_what_the_base_already_sets() {
        let (mut sys, paths) = desktop();
        let base = Manifest::parse(
            "schema = 1\nid = \"base\"\nname = \"Base\"\n[plasma]\ncursor = \"breeze_cursors\"\n[kconfig.kdeglobals]\n\"General/font\" = \"Inter,10,-1\"\n[kconfig.kwinrc]\n\"Plugins/fadeEnabled\" = true\n",
        )
        .unwrap();
        let c = capture(&mut sys, &paths, &Info { id: "x".into(), ..Default::default() }, Some(&base)).unwrap();
        let m = Manifest::parse(&c.manifest).unwrap();
        assert_eq!(m.plasma.cursor, None);
        assert!(!m.kconfig["kdeglobals"].contains_key("General/font"));
        assert!(!m.kconfig["kwinrc"].contains_key("Plugins/fadeEnabled"));
        assert!(m.kconfig["kwinrc"].contains_key("Effect-blur/BlurStrength"));
    }

    #[test]
    fn layout_runs_after_the_prelude() {
        let js = layout_script("x", DUMP, STATE).unwrap();
        assert!(js.contains("d.applets = []"));
        assert!(js.contains("a.config[\"/General\"].launchers = kigae.launchers"));
        assert!(js.contains(r#"kigae.tray(null, {"General":{"hiddenItems":"a,b"}});"#), "{js}");
        assert!(js.contains(r#"kigae.desktop("org.ukiyo.clock", 1216, 368, 528, 240);"#), "{js}");
        let load = js.find("plasma.loadSerializedLayout(layout);").unwrap();
        assert!(js.find("d.applets = []").unwrap() < load && load < js.find("kigae.tray").unwrap());
    }

    #[test]
    fn parses_kconfig_groups() {
        let k = parse_kconfig("[A][B]\nx=1\ny[de]=2\n[$Version]\nz=3\n[C]\nw=a=b\n");
        assert_eq!(k["A/B"]["x"], "1");
        assert!(!k["A/B"].contains_key("y"));
        assert!(!k.contains_key("$Version"));
        assert_eq!(k["C"]["w"], "a=b");
    }
}
