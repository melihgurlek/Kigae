use super::*;
use crate::manifest::{Unset, UnsetPolicy};
use crate::sys::{FakeSys, UnitState};

const UKIYO: &str = r##"
schema = 1
id = "ukiyo"
name = "Ukiyo"
[plasma]
colors = "Ukiyo"
style = "ukiyo"
icons = "Colloid-Yellow-Everforest-Dark"
cursor = "breeze_cursors"
layout = "ukiyo/kigae-layout.js"
wallpaper = "~/Pictures/Wallpapers/ukiyo/night-wave.jpg"
[kconfig.kwinrc]
"Plugins/fadeEnabled" = true
"org.kde.kdecoration2/library" = "org.kde.breeze"
[kconfig.konsolerc]
"Desktop Entry/DefaultProfile" = "Ukiyo.profile"
[links]
"~/.config/kitty/kigae-current.conf" = "~/.config/kitty/ukiyo.conf"
"~/.config/kitty/dark-theme.auto.conf" = "~/.config/kitty/ukiyo-ink.conf"
[vscode]
"workbench.colorTheme" = "Kanagawa"
"window.autoDetectColorScheme" = true
[services]
enable = ["ukiyo-sky.timer"]
[hooks]
after = ["ukiyo-sky --force"]
"##;

const OG: &str = r##"
schema = 1
id = "og"
name = "Scratchy & Itchy"
branch = "backup/og"
[plasma]
icons = "Papirus"
cursor = "breeze_cursors"
layout = "./og/layout.js"
wallpaper = "/wall/og.jpg"
[kconfig.kwinrc]
"org.kde.kdecoration2/library" = "org.kde.kwin.aurorae"
[links]
"~/.config/kitty/kigae-current.conf" = "~/.config/kitty/og.conf"
[vscode]
"workbench.colorTheme" = "Catppuccin Mocha"
[daynight.light]
colors = "Itchy"
style = "Itchy"
kconfig.kwinrc."org.kde.kdecoration2/theme" = "__aurorae__svg__Itchy"
[daynight.dark]
colors = "Scratchy"
style = "Scratchy"
kconfig.kwinrc."org.kde.kdecoration2/theme" = "__aurorae__svg__Scratchy"
"##;

const SETTINGS: &str = "{\n  // mine\n  \"workbench.colorTheme\": \"Kanagawa\",\n  \"window.autoDetectColorScheme\": true,\n  \"editor.fontSize\": 14\n}\n";

fn paths() -> Paths {
    Paths { home: "/h".into(), state: "/h/.local/state/kigae".into() }
}

/// The desktop as Ukiyo leaves it.
fn ukiyo_desktop() -> FakeSys {
    let mut s = FakeSys::default();
    s.set_kconfig("kdeglobals", "General", "ColorScheme", "Ukiyo");
    s.set_kconfig("plasmarc", "Theme", "name", "ukiyo");
    s.set_kconfig("kdeglobals", "Icons", "Theme", "Colloid-Yellow-Everforest-Dark");
    s.set_kconfig("kcminputrc", "Mouse", "cursorTheme", "breeze_cursors");
    s.set_kconfig("kdeglobals", "KDE", "AutomaticLookAndFeel", "false");
    s.set_kconfig("kwinrc", "Plugins", "fadeEnabled", "true");
    s.set_kconfig("kwinrc", "org.kde.kdecoration2", "library", "org.kde.breeze");
    s.set_kconfig("konsolerc", "Desktop Entry", "DefaultProfile", "Ukiyo.profile");
    let link = |s: &mut FakeSys, l: &str, t: &str| {
        s.files.insert(format!("/h/.config/kitty/{l}").into(), FileState::Link(format!("/h/.config/kitty/{t}").into()));
    };
    link(&mut s, "kigae-current.conf", "ukiyo.conf");
    link(&mut s, "dark-theme.auto.conf", "ukiyo-ink.conf");
    s.files.insert("/h/.config/Code/User/settings.json".into(), FileState::Content(SETTINGS.into()));
    s.files.insert("/h/.local/state/kigae/current".into(), FileState::Content("ukiyo\n".into()));
    s.units.insert("ukiyo-sky.timer".into(), UnitState { enabled: true, active: true });
    s.layout = "UKIYO-LAYOUT\nplasma.loadSerializedLayout(layout);".into();
    s.layout_state = r#"{"trays":[{"General":{"hiddenItems":["a"]}}],"desk":[]}"#.into();
    s.wallpaper = Some("file:///h/Pictures/Wallpapers/ukiyo/night-wave.jpg".into());
    s
}

fn opts() -> Opts {
    Opts { no_journal: true, ..Opts::default() }
}

fn target<'a>(m: &'a Manifest, js: &str) -> Target<'a> {
    Target { manifest: m, layout_js: Some(js.into()) }
}

fn rices() -> (Manifest, Manifest) {
    (Manifest::parse(UKIYO).unwrap(), Manifest::parse(OG).unwrap())
}

#[test]
fn ukiyo_to_og_at_night() {
    let (ukiyo, og) = rices();
    let mut sys = ukiyo_desktop();
    let out = apply(&mut sys, &paths(), Some(&ukiyo), target(&og, "OG-LAYOUT"), opts()).unwrap();
    assert_eq!(out.mood, Some(MoodKind::Dark), "fake NightTime says night");
    assert!(out.warnings.is_empty(), "{:?}", out.warnings);
    assert_eq!(
        sys.log(),
        [
            "systemctl --user disable+stop ukiyo-sky.timer",
            "kconfig kwinrc [org.kde.kdecoration2] library = \"org.kde.kwin.aurorae\"",
            "kconfig kwinrc [org.kde.kdecoration2] theme = \"__aurorae__svg__Scratchy\"",
            "kconfig konsolerc [Desktop Entry] DefaultProfile (delete)",
            "kconfig kwinrc [Plugins] fadeEnabled (delete)",
            "KWin reconfigure",
            "plasma-apply-colorscheme Scratchy",
            "plasma-apply-desktoptheme Scratchy",
            "/usr/libexec/plasma-changeicons Papirus",
            "plasmashell script: prelude + layout of og",
            "plasma-apply-wallpaperimage /wall/og.jpg",
            "link /h/.config/kitty/kigae-current.conf -> /h/.config/kitty/og.conf",
            "remove /h/.config/kitty/dark-theme.auto.conf",
            "reload kitty (SIGUSR1)",
            &format!("write /h/.config/Code/User/settings.json ({} bytes)", sys.files.get(std::path::Path::new("/h/.config/Code/User/settings.json")).map(|f| match f { FileState::Content(c) => c.len(), _ => 0 }).unwrap()),
            "write /h/.local/state/kigae/current (3 bytes)",
        ]
    );
    assert!(sys.layout.starts_with(&prelude()) && sys.layout.contains("OG-LAYOUT\nkigae.done();"));
    let FileState::Content(settings) = &sys.files[std::path::Path::new("/h/.config/Code/User/settings.json")] else { panic!() };
    assert!(settings.contains("\"Catppuccin Mocha\"") && settings.contains("// mine"), "{settings}");
    assert!(!settings.contains("autoDetectColorScheme"), "ukiyo's key goes with it: {settings}");
}

#[test]
fn undo_puts_everything_back() {
    let (ukiyo, og) = rices();
    let before = ukiyo_desktop();
    let mut sys = before.clone();
    let mut out = apply(&mut sys, &paths(), Some(&ukiyo), target(&og, "OG-LAYOUT"), opts()).unwrap();
    let errors = undo(&mut sys, &paths(), &mut out.journal).unwrap();
    assert!(errors.is_empty(), "{errors:?}");
    assert_eq!(sys.kconfig, before.kconfig);
    assert_eq!(sys.files, before.files);
    assert_eq!(sys.units, before.units);
    assert_eq!(sys.wallpaper, before.wallpaper);
    assert_eq!(
        sys.layout,
        format!("{}UKIYO-LAYOUT\nlayout.desktops.forEach(function (d) {{ d.applets = []; }});\nplasma.loadSerializedLayout(layout);", prelude()),
        "the dump from before, after the prelude, desktop widgets left to the second pass"
    );
    assert_eq!(sys.layout_state, before.layout_state, "tray settings and widget positions come back");
    assert_eq!(out.journal.status, Status::Undone);
}

#[test]
fn declared_tray_settings_get_a_second_pass() {
    let (ukiyo, og) = rices();
    let mut sys = ukiyo_desktop();
    let st = r#"{"trays":[{"General":{"hiddenItems":["b"]}}],"desk":[]}"#;
    sys.script_output = vec![format!("KIGAE-STATE {st}\n")];
    apply(&mut sys, &paths(), Some(&ukiyo), target(&og, "OG-LAYOUT"), opts()).unwrap();
    let log = sys.log();
    let i = log.iter().position(|a| a == "plasmashell script: prelude + layout of og").unwrap();
    assert_eq!(log[i + 1], "plasmashell script: tray settings and desktop widget positions");
    assert_eq!(sys.layout_state, st);
}

#[test]
fn critical_failure_rolls_back() {
    let (ukiyo, og) = rices();
    let before = ukiyo_desktop();
    let mut sys = before.clone();
    sys.fail_on = Some("plasma-apply-desktoptheme Scratchy".into());
    let err = apply(&mut sys, &paths(), Some(&ukiyo), target(&og, "OG-LAYOUT"), opts()).unwrap_err();
    assert!(matches!(err, ApplyError::Critical { step: "style", ref rollback, .. } if rollback.is_empty()), "{err}");
    assert_eq!(sys.kconfig, before.kconfig);
    assert_eq!(sys.units, before.units);
    assert_eq!(sys.layout, before.layout, "never got to the layout");
}

#[test]
fn failed_rollback_keeps_the_journal_open() {
    let (ukiyo, og) = rices();
    let mut sys = ukiyo_desktop();
    // fails going forward, and again when the style is put back
    sys.fail_on = Some("plasma-apply-desktoptheme".into());
    let dir = std::env::temp_dir().join(format!("kigae-engine-{}", std::process::id()));
    let p = Paths { home: "/h".into(), state: dir.clone() };
    let o = Opts { no_journal: false, ..opts() };
    let err = apply(&mut sys, &p, Some(&ukiyo), target(&og, "OG-LAYOUT"), o).unwrap_err();
    assert!(matches!(err, ApplyError::Critical { ref rollback, .. } if !rollback.is_empty()), "{err}");
    let journals = Journal::list(&p.undo_dir()).unwrap();
    assert_eq!(journals.len(), 1);
    assert_eq!(journals[0].status, Status::InProgress);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn non_critical_failure_is_a_warning() {
    let (ukiyo, og) = rices();
    let mut sys = ukiyo_desktop();
    sys.fail_on = Some("plasma-apply-wallpaperimage".into());
    let out = apply(&mut sys, &paths(), Some(&ukiyo), target(&og, "OG-LAYOUT"), opts()).unwrap();
    assert_eq!(out.warnings.len(), 1);
    assert!(out.warnings[0].starts_with("wallpaper:"), "{:?}", out.warnings);
    assert_eq!(out.journal.status, Status::Done);
}

#[test]
fn same_rice_again_changes_nothing() {
    let (ukiyo, _) = rices();
    let mut sys = ukiyo_desktop();
    let out = apply(&mut sys, &paths(), Some(&ukiyo), Target { manifest: &ukiyo, layout_js: None }, opts()).unwrap();
    assert_eq!(sys.log(), ["sh -c \"ukiyo-sky --force\""], "only the hook runs");
    assert!(out.journal.entries.is_empty());
}

#[test]
fn mood_switch_touches_only_the_mood() {
    let (ukiyo, og) = rices();
    let mut sys = ukiyo_desktop();
    apply(&mut sys, &paths(), Some(&ukiyo), target(&og, "OG-LAYOUT"), opts()).unwrap();
    sys.actions.clear();
    let o = Opts { mood: Some(MoodKind::Light), only_mood: true, ..opts() };
    apply(&mut sys, &paths(), Some(&og), Target { manifest: &og, layout_js: Some("OG-LAYOUT".into()) }, o).unwrap();
    assert_eq!(
        sys.log(),
        [
            "kconfig kwinrc [org.kde.kdecoration2] theme = \"__aurorae__svg__Itchy\"",
            "KWin reconfigure",
            "plasma-apply-colorscheme Itchy",
            "plasma-apply-desktoptheme Itchy",
        ]
    );
}

#[test]
fn keep_leaves_the_previous_rice_alone() {
    let (ukiyo, og) = rices();
    let mut sys = ukiyo_desktop();
    let keep = UnsetPolicy { kconfig: Unset::Keep, links: Unset::Keep, vscode: Unset::Keep };
    apply(&mut sys, &paths(), Some(&ukiyo), target(&og, "OG-LAYOUT"), Opts { unset: keep, ..opts() }).unwrap();
    let log = sys.log().join("\n");
    assert!(!log.contains("(delete)") && !log.contains("remove "), "{log}");
    assert_eq!(sys.get_kconfig("konsolerc", "Desktop Entry", "DefaultProfile"), Some("Ukiyo.profile"));
    let FileState::Content(settings) = &sys.files[std::path::Path::new("/h/.config/Code/User/settings.json")] else { panic!() };
    assert!(settings.contains("autoDetectColorScheme"), "{settings}");
}

#[test]
fn base_fills_a_rice_without_vscode() {
    let (ukiyo, mut og) = rices();
    og.vscode.clear();
    let base = Manifest::parse("schema = 1\nid = \"base\"\nname = \"Base\"\n[vscode]\n\"workbench.colorTheme\" = \"Default Dark Modern\"").unwrap();
    let policy = UnsetPolicy::default();
    let (from, to) = (ukiyo.with_base(Some(&base), policy), og.with_base(Some(&base), policy));
    let mut sys = ukiyo_desktop();
    apply(&mut sys, &paths(), Some(&from), target(&to, "OG-LAYOUT"), opts()).unwrap();
    let FileState::Content(settings) = &sys.files[std::path::Path::new("/h/.config/Code/User/settings.json")] else { panic!() };
    assert!(settings.contains("\"Default Dark Modern\""), "{settings}");
    assert!(!settings.contains("autoDetectColorScheme"), "{settings}");
}

#[test]
fn infers_the_rice_on_screen() {
    let (ukiyo, og) = rices();
    let all = [og.clone(), ukiyo.clone()];
    let mut sys = ukiyo_desktop();
    assert_eq!(infer_current(&mut sys, &all).map(|m| m.id.as_str()), Some("ukiyo"));
    sys.set_kconfig("plasmarc", "Theme", "name", "Itchy");
    assert_eq!(infer_current(&mut sys, &all).map(|m| m.id.as_str()), Some("og"), "by a mood's style");
    sys.set_kconfig("plasmarc", "Theme", "name", "Unknown");
    sys.set_kconfig("kdeglobals", "General", "ColorScheme", "Other");
    assert!(infer_current(&mut sys, &all).is_none());
}

#[test]
fn doctor_reports_missing_parts() {
    let (ukiyo, _) = rices();
    let mut sys = FakeSys::default();
    let problems = doctor(&mut sys, &paths(), &ukiyo);
    let all = problems.join("\n");
    for want in ["colour scheme \"Ukiyo\"", "Plasma style \"ukiyo\"", "icon theme", "cursor theme", "wallpaper", "link", "service ukiyo-sky.timer"] {
        assert!(all.contains(want), "missing {want:?} in\n{all}");
    }
    sys.present = vec![
        "/h/.local/share/color-schemes/Ukiyo.colors".into(),
        "/h/.local/share/plasma/desktoptheme/ukiyo".into(),
        "/h/.local/share/icons/Colloid-Yellow-Everforest-Dark/index.theme".into(),
        "/usr/share/icons/breeze_cursors/cursors".into(),
        "/h/Pictures/Wallpapers/ukiyo/night-wave.jpg".into(),
        "/h/.config/kitty/ukiyo.conf".into(),
        "/h/.config/kitty/ukiyo-ink.conf".into(),
    ];
    sys.units.insert("ukiyo-sky.timer".into(), UnitState { enabled: false, active: false });
    assert_eq!(doctor(&mut sys, &paths(), &ukiyo), Vec::<String>::new());
}

#[test]
fn decodes_file_urls() {
    assert_eq!(file_url_path("file:///a/Fedora%20Backup/x.jpg"), std::path::PathBuf::from("/a/Fedora Backup/x.jpg"));
    assert_eq!(file_url_path("file:///a/b c.jpg"), std::path::PathBuf::from("/a/b c.jpg"));
    assert_eq!(file_url_path("/a/100%aé.jpg"), std::path::PathBuf::from("/a/100%aé.jpg"), "no slicing inside a character");
    assert_eq!(file_url_path("/a%+f"), std::path::PathBuf::from("/a%+f"), "a sign is not a hex digit");
    assert_eq!(file_url_path("/a%C3%A9%2f"), std::path::PathBuf::from("/aé/"));
    assert_eq!(file_url_path("/plain%2"), std::path::PathBuf::from("/plain%2"));
}

#[test]
fn kconfig_values_naming_home_files_are_spelled_out() {
    let paths = Paths { home: "/h".into(), state: "/h/.local/state/kigae".into() };
    assert_eq!(paths.expand_value("file://~/a/lock.jpg"), "file:///h/a/lock.jpg");
    assert_eq!(paths.expand_value("~/a.jpg"), "/h/a.jpg");
    assert_eq!(paths.expand_value("org.kde.image"), "org.kde.image");
    assert_eq!(paths.expand_value("file:///etc/x"), "file:///etc/x");
}

#[test]
fn the_login_follows_a_switch_once_set_up_and_undo_puts_it_back() {
    let (ukiyo, _) = rices();
    let mut og = Manifest::parse(&format!("{OG}\n[deep]\nsddm = \"astronaut\"\n")).unwrap();
    // not set up: the switch leaves the login alone (the CLI hints at `kigae deep`)
    let mut sys = ukiyo_desktop();
    sys.login = Some("ukiyo".into());
    apply(&mut sys, &paths(), Some(&ukiyo), target(&og, "OG-LAYOUT"), opts()).unwrap();
    assert_eq!(sys.login.as_deref(), Some("ukiyo"));

    // set up: no prompt, the login becomes og's, and undo makes it ukiyo's again
    let mut sys = ukiyo_desktop();
    sys.login = Some("ukiyo".into());
    sys.present = vec![deep::LOGIN_HELPER.into(), deep::LOGIN_POLICY.into()];
    let mut out = apply(&mut sys, &paths(), Some(&ukiyo), target(&og, "OG-LAYOUT"), opts()).unwrap();
    assert_eq!(sys.login.as_deref(), Some("astronaut"));
    assert!(sys.log().contains(&format!("pkexec {} astronaut", deep::LOGIN_HELPER)));
    let errors = undo(&mut sys, &paths(), &mut out.journal).unwrap();
    assert!(errors.is_empty(), "{errors:?}");
    assert_eq!(sys.login.as_deref(), Some("ukiyo"));

    // a mood change never touches it
    let n = sys.actions.len();
    sys.login = Some("other".into());
    apply(&mut sys, &paths(), Some(&og), target(&og, "OG-LAYOUT"), Opts { only_mood: true, ..opts() }).unwrap();
    assert!(!sys.log()[n..].iter().any(|a| a.contains("kigae-login")));
}
