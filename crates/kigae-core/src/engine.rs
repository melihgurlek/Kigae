//! Apply a rice, undo a switch, check a rice is installed (SPEC.md §4).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::journal::{Entry, Journal, Status};
use crate::manifest::{Manifest, MoodKind, Unset, UnsetPolicy, kconfig_key, kconfig_value};
use crate::sys::{Action, FileState, Sys, SysError};

/// The `kigae` helpers every Kigae plasmashell script starts with (SPEC.md §5).
pub const HELPERS: &str = include_str!("prelude.js");

/// Runs before every rice layout: remembers the dock launchers in `kigae.launchers`, then
/// clears every panel and desktop widget.
pub fn prelude() -> String {
    format!("{HELPERS}\nkigae.clear();\n")
}

/// Panels need a moment before Plasma accepts tray settings written into them.
const SETTLE_MS: u64 = 600;

/// The JSON after `KIGAE-STATE ` in a script's output.
pub fn kigae_state(out: &str) -> Option<&str> {
    out.lines().find_map(|l| l.trim().strip_prefix("KIGAE-STATE ")).map(str::trim)
}

/// A `dumpCurrentLayoutJS` script that leaves desktop widgets out.
fn without_desktop_widgets(dump: &str) -> String {
    const LOAD: &str = "plasma.loadSerializedLayout(layout);";
    dump.replacen(LOAD, &format!("layout.desktops.forEach(function (d) {{ d.applets = []; }});\n{LOAD}"), 1)
}

fn settle(state: &str) -> Action {
    Action::EvaluateScript {
        what: "tray settings and desktop widget positions".into(),
        script: format!("{HELPERS}\nkigae.restore({state});"),
        delay_ms: SETTLE_MS,
    }
}

#[derive(Debug, Clone)]
pub struct Paths {
    pub home: PathBuf,
    /// `~/.local/state/kigae`
    pub state: PathBuf,
}

impl Paths {
    pub fn from_env() -> Option<Self> {
        let home = PathBuf::from(std::env::var_os("HOME")?);
        let state = std::env::var_os("XDG_STATE_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".local/state"))
            .join("kigae");
        Some(Paths { home, state })
    }

    /// `~/x` → `$HOME/x`; anything else as is.
    pub fn expand(&self, s: &str) -> PathBuf {
        match s.strip_prefix("~/") {
            Some(rest) => self.home.join(rest),
            None if s == "~" => self.home.clone(),
            None => PathBuf::from(s),
        }
    }

    /// A KConfig value that names a file in home, `~/x` or `file://~/x`, with the home spelled
    /// out (KConfig doesn't expand `~`); anything else as is. Lets a manifest stay portable.
    pub fn expand_value(&self, s: &str) -> String {
        match s.strip_prefix("file://~/") {
            Some(rest) => format!("file://{}", self.home.join(rest).display()),
            None if s.starts_with("~/") => self.expand(s).display().to_string(),
            None => s.to_string(),
        }
    }

    pub fn current(&self) -> PathBuf {
        self.state.join("current")
    }

    pub fn undo_dir(&self) -> PathBuf {
        self.state.join("undo")
    }

    fn vscode_settings(&self) -> PathBuf {
        self.home.join(".config/Code/User/settings.json")
    }

    fn kitty_dir(&self) -> PathBuf {
        self.home.join(".config/kitty")
    }
}

/// The rice to switch to, with its layout script already read from git.
pub struct Target<'a> {
    pub manifest: &'a Manifest,
    pub layout_js: Option<String>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Opts {
    /// `None`: ask NightTime (only matters for rices with `[daynight]`).
    pub mood: Option<MoodKind>,
    /// Re-apply only what `[daynight]` changes (the day/night switch of the active rice).
    pub only_mood: bool,
    /// Keep the journal in memory only.
    pub no_journal: bool,
    /// Sections set to [`Unset::Keep`] don't delete what the previous rice set. (`Base` is
    /// handled before `apply`: pass rices through [`Manifest::with_base`].)
    pub unset: UnsetPolicy,
}

#[derive(Debug)]
pub struct Outcome {
    pub journal: Journal,
    pub mood: Option<MoodKind>,
    pub warnings: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum ApplyError {
    #[error("{step} failed: {err}{}", rollback_note(.rollback))]
    Critical { step: &'static str, err: SysError, rollback: Vec<String> },
    #[error("{0}")]
    Sys(#[from] SysError),
    #[error("journal: {0}")]
    Journal(#[from] std::io::Error),
}

fn rollback_note(errs: &[String]) -> String {
    if errs.is_empty() {
        "; everything changed so far was put back".into()
    } else {
        format!("; rolling back also failed: {}", errs.join("; "))
    }
}

struct Run<'s, S: Sys> {
    sys: &'s mut S,
    journal: Journal,
    warnings: Vec<String>,
}

impl<S: Sys> Run<'_, S> {
    fn record(&mut self, e: Entry) -> Result<(), ApplyError> {
        Ok(self.journal.push(e)?)
    }
}

pub fn apply<S: Sys>(
    sys: &mut S,
    paths: &Paths,
    from: Option<&Manifest>,
    to: Target<'_>,
    opts: Opts,
) -> Result<Outcome, ApplyError> {
    let m = to.manifest;
    let mood = match (&m.daynight, opts.mood) {
        (None, _) => None,
        (Some(_), Some(k)) => Some(k),
        (Some(_), None) => Some(if sys.is_day()? { MoodKind::Light } else { MoodKind::Dark }),
    };
    let dir = (!opts.no_journal).then(|| paths.undo_dir());
    let journal = Journal::new(from.map(|f| f.id.as_str()), &m.id, mood.map(MoodKind::name), dir.as_deref());
    let mut run = Run { sys, journal, warnings: vec![] };
    run.journal.save()?;

    type Step<'a, S> = (&'static str, bool, Box<dyn Fn(&mut Run<'_, S>) -> Result<(), ApplyError> + 'a>);
    let look = m.look(mood);
    let full = !opts.only_mood;
    // the panels as they are, for undo: read now, while plasmashell is idle (after the theme
    // tools it's busy repainting and the same two calls take ~0.6 s instead of ~0.1 s)
    let panels = match (&to.layout_js, full) {
        (Some(_), true) => Some((run.sys.capture_layout()?, run.sys.dump_layout()?)),
        _ => None,
    };
    let steps: Vec<Step<'_, S>> = vec![
        ("services-stop", true, Box::new(|r| if full { services_stop(r, from, m) } else { Ok(()) })),
        ("kconfig", true, Box::new(|r| kconfig(r, paths, from, m, &look, opts.unset.kconfig))),
        ("colors", true, Box::new(|r| plasma_part(r, look.colors.as_deref(), Part::Colors))),
        ("style", true, Box::new(|r| plasma_part(r, look.style.as_deref(), Part::Style))),
        ("icons", true, Box::new(|r| plasma_part(r, look.icons.as_deref(), Part::Icons))),
        ("cursor", true, Box::new(|r| plasma_part(r, look.cursor.as_deref(), Part::Cursor))),
        ("layout", true, Box::new(|r| match (&to.layout_js, &panels) {
            (Some(js), Some(before)) => layout(r, &m.id, js, before),
            _ => Ok(()),
        })),
        ("wallpaper", false, Box::new(|r| wallpaper(r, paths, look.wallpaper.as_deref()))),
        ("links", false, Box::new(|r| if full { links(r, paths, from, m, opts.unset.links) } else { Ok(()) })),
        ("vscode", false, Box::new(|r| if full { vscode(r, paths, from, m, opts.unset.vscode) } else { Ok(()) })),
        ("services-start", false, Box::new(|r| if full { services_start(r, m) } else { Ok(()) })),
        // after a mood change too: a rice's own timer (ukiyo-sky) takes over from the new look
        ("hooks", false, Box::new(|r| {
            for cmd in &m.hooks.after {
                r.sys.act(&Action::Shell(cmd.clone()))?;
            }
            Ok(())
        })),
        ("current", false, Box::new(|r| current(r, paths, &m.id))),
    ];

    for (step, critical, f) in steps {
        match f(&mut run) {
            Ok(()) => {}
            Err(ApplyError::Sys(err)) if critical => {
                let rollback = undo_entries(run.sys, paths, &run.journal.entries);
                // a rollback that failed leaves the journal open, so `kigae undo` still finds it
                if rollback.is_empty() {
                    run.journal.set_status(Status::RolledBack)?;
                }
                return Err(ApplyError::Critical { step, err, rollback });
            }
            Err(ApplyError::Sys(err)) => run.warnings.push(format!("{step}: {err}")),
            Err(e @ ApplyError::Journal(_)) => {
                // the journal can't be saved, so put back what we can from memory
                let rollback = undo_entries(run.sys, paths, &run.journal.entries);
                return Err(if rollback.is_empty() {
                    e
                } else {
                    ApplyError::Journal(std::io::Error::other(format!("{e}{}", rollback_note(&rollback))))
                });
            }
            Err(e) => return Err(e),
        }
    }
    run.journal.set_status(Status::Done)?;
    Ok(Outcome { journal: run.journal, mood, warnings: run.warnings })
}

fn services_stop<S: Sys>(r: &mut Run<'_, S>, from: Option<&Manifest>, to: &Manifest) -> Result<(), ApplyError> {
    let Some(from) = from else { return Ok(()) };
    for unit in from.services.enable.iter().filter(|u| !to.services.enable.contains(u)) {
        let old = r.sys.unit_state(unit)?;
        if old.enabled || old.active {
            r.record(Entry::Unit { unit: unit.to_string(), old })?;
            r.sys.act(&Action::Unit { unit: unit.to_string(), enable: Some(false), active: Some(false) })?;
        }
    }
    Ok(())
}

fn services_start<S: Sys>(r: &mut Run<'_, S>, to: &Manifest) -> Result<(), ApplyError> {
    for unit in &to.services.enable {
        let old = r.sys.unit_state(unit)?;
        if !(old.enabled && old.active) {
            r.record(Entry::Unit { unit: unit.to_string(), old })?;
            r.sys.act(&Action::Unit { unit: unit.to_string(), enable: Some(true), active: Some(true) })?;
        }
    }
    Ok(())
}

fn kconfig<S: Sys>(
    r: &mut Run<'_, S>,
    paths: &Paths,
    from: Option<&Manifest>,
    to: &Manifest,
    look: &crate::manifest::Look,
    unset: Unset,
) -> Result<(), ApplyError> {
    // (file, "Group/Key") → (value, boolean)
    let mut want: Vec<(String, String, String, bool)> = vec![(
        "kdeglobals".into(),
        "KDE/AutomaticLookAndFeel".into(),
        "false".into(),
        true,
    )];
    for (file, keys) in &look.kconfig {
        for (key, value) in keys {
            let (v, b) = kconfig_value(value).expect("validated");
            let v = if b { v } else { paths.expand_value(&v) };
            want.push((file.clone(), key.clone(), v, b));
        }
    }
    let wanted: BTreeSet<(&str, &str)> = want.iter().map(|(f, k, ..)| (f.as_str(), k.as_str())).collect();
    // this rice's other-mood keys always go; the previous rice's unless "keep"
    let mut owned = to.owned_kconfig();
    if let Some(f) = from.filter(|_| unset != Unset::Keep) {
        owned.extend(f.owned_kconfig());
    }
    let drop: BTreeSet<(String, String)> =
        owned.into_iter().filter(|(f, k)| !wanted.contains(&(f.as_str(), k.as_str()))).collect();

    let mut kwin = false;
    let changes = want
        .iter()
        .map(|(f, k, v, b)| (f.clone(), k.clone(), Some(v.clone()), *b))
        .chain(drop.into_iter().map(|(f, k)| (f, k, None, false)));
    for (file, key, value, boolean) in changes {
        let (groups, k) = kconfig_key(&key).expect("validated");
        let groups: Vec<String> = groups.into_iter().map(str::to_string).collect();
        let old = r.sys.kread(&file, &groups, k)?;
        if old == value {
            continue;
        }
        r.record(Entry::KConfig { file: file.clone(), groups: groups.clone(), key: k.into(), old })?;
        r.sys.act(&Action::KWrite { file: file.clone(), groups, key: k.into(), value, boolean })?;
        kwin |= file == "kwinrc";
    }
    if kwin {
        r.sys.act(&Action::KWinReconfigure)?;
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum Part {
    Colors,
    Style,
    Icons,
    Cursor,
}

impl Part {
    /// Where Plasma keeps the current value, and the tool that changes it live.
    fn spec(self) -> (&'static str, &'static str, &'static str, &'static str) {
        match self {
            Part::Colors => ("kdeglobals", "General", "ColorScheme", "plasma-apply-colorscheme"),
            Part::Style => ("plasmarc", "Theme", "name", "plasma-apply-desktoptheme"),
            Part::Icons => ("kdeglobals", "Icons", "Theme", "/usr/libexec/plasma-changeicons"),
            Part::Cursor => ("kcminputrc", "Mouse", "cursorTheme", "plasma-apply-cursortheme"),
        }
    }

    fn entry(self, old: Option<String>) -> Entry {
        match self {
            Part::Colors => Entry::Colors { old },
            Part::Style => Entry::Style { old },
            Part::Icons => Entry::Icons { old },
            Part::Cursor => Entry::Cursor { old },
        }
    }
}

fn plasma_part<S: Sys>(r: &mut Run<'_, S>, want: Option<&str>, part: Part) -> Result<(), ApplyError> {
    let Some(want) = want else { return Ok(()) };
    let (file, group, key, tool) = part.spec();
    let old = r.sys.kread(file, &[group.into()], key)?;
    if old.as_deref() == Some(want) {
        return Ok(());
    }
    r.record(part.entry(old))?;
    r.sys.act(&Action::Exec(vec![tool.into(), want.into()]))?;
    Ok(())
}

/// `before`: `capture_layout` and `dump_layout`, read before the switch started.
fn layout<S: Sys>(r: &mut Run<'_, S>, id: &str, js: &str, before: &(String, String)) -> Result<(), ApplyError> {
    let (state, script) = before.clone();
    r.record(Entry::Layout { script, state: Some(state) })?;
    let out = r.sys.act(&Action::EvaluateScript {
        what: format!("prelude + layout of {id}"),
        script: format!("{}{js}\nkigae.done();\n", prelude()),
        delay_ms: 0,
    })?;
    if let Some(state) = kigae_state(&out) {
        r.sys.act(&settle(state))?;
    }
    Ok(())
}

fn wallpaper<S: Sys>(r: &mut Run<'_, S>, paths: &Paths, want: Option<&str>) -> Result<(), ApplyError> {
    let Some(want) = want else { return Ok(()) };
    let path = paths.expand(want);
    let old = r.sys.wallpaper()?;
    if old.as_deref().map(file_url_path) == Some(path.clone()) {
        return Ok(());
    }
    r.record(Entry::Wallpaper { old })?;
    r.sys.act(&Action::Exec(vec!["plasma-apply-wallpaperimage".into(), path.display().to_string()]))?;
    Ok(())
}

/// `file:///a%20b` → `/a b`
pub fn file_url_path(url: &str) -> PathBuf {
    let raw = url.strip_prefix("file://").unwrap_or(url);
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let (Some(hi), Some(lo)) = ((bytes[i + 1] as char).to_digit(16), (bytes[i + 2] as char).to_digit(16))
        {
            out.push((hi * 16 + lo) as u8);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    PathBuf::from(String::from_utf8_lossy(&out).into_owned())
}

fn set_file<S: Sys>(r: &mut Run<'_, S>, path: &Path, want: FileState) -> Result<bool, ApplyError> {
    let old = r.sys.file_state(path)?;
    if old == want {
        return Ok(false);
    }
    r.record(Entry::File { path: path.to_path_buf(), old })?;
    r.sys.act(&match want {
        FileState::Missing => Action::Remove(path.to_path_buf()),
        FileState::Link(target) => Action::Symlink { link: path.to_path_buf(), target },
        FileState::Content(contents) => Action::WriteFile { path: path.to_path_buf(), contents },
    })?;
    Ok(true)
}

fn links<S: Sys>(r: &mut Run<'_, S>, paths: &Paths, from: Option<&Manifest>, to: &Manifest, unset: Unset) -> Result<(), ApplyError> {
    let mut changed = vec![];
    for (link, target) in &to.links {
        let link = paths.expand(link);
        if set_file(r, &link, FileState::Link(paths.expand(target)))? {
            changed.push(link);
        }
    }
    let from = from.filter(|_| unset != Unset::Keep);
    for link in from.iter().flat_map(|f| f.links.keys()).filter(|l| !to.links.contains_key(*l)) {
        let link = paths.expand(link);
        if set_file(r, &link, FileState::Missing)? {
            changed.push(link);
        }
    }
    if changed.iter().any(|l| l.starts_with(paths.kitty_dir())) {
        r.sys.act(&Action::ReloadKitty)?;
    }
    Ok(())
}

fn vscode<S: Sys>(r: &mut Run<'_, S>, paths: &Paths, from: Option<&Manifest>, to: &Manifest, unset: Unset) -> Result<(), ApplyError> {
    let remove: Vec<String> = from.filter(|_| unset != Unset::Keep).iter().flat_map(|f| f.vscode.keys()).cloned().collect();
    if to.vscode.is_empty() && remove.is_empty() {
        return Ok(());
    }
    let path = paths.vscode_settings();
    let text = match r.sys.file_state(&path)? {
        FileState::Content(t) => t,
        FileState::Missing => {
            r.warnings.push(format!("vscode: {} doesn't exist, skipped", path.display()));
            return Ok(());
        }
        FileState::Link(_) => {
            r.warnings.push(format!("vscode: {} is a symlink, skipped", path.display()));
            return Ok(());
        }
    };
    let new = crate::vscode::edit(&text, &to.vscode, &remove)
        .map_err(|msg| SysError::Command { cmd: format!("edit {}", path.display()), msg })?;
    if new != text {
        set_file(r, &path, FileState::Content(new))?;
    }
    Ok(())
}

fn current<S: Sys>(r: &mut Run<'_, S>, paths: &Paths, id: &str) -> Result<(), ApplyError> {
    set_file(r, &paths.current(), FileState::Content(format!("{id}\n")))?;
    Ok(())
}

/// Put back what the entries changed, newest first. Keeps going past failures and returns them.
pub fn undo_entries<S: Sys>(sys: &mut S, paths: &Paths, entries: &[Entry]) -> Vec<String> {
    let mut errors = vec![];
    let mut kwin = false;
    let mut kitty = false;
    for e in entries.iter().rev() {
        let actions: Vec<Action> = match e {
            Entry::KConfig { file, groups, key, old } => {
                kwin |= file == "kwinrc";
                vec![Action::KWrite { file: file.clone(), groups: groups.clone(), key: key.clone(), value: old.clone(), boolean: false }]
            }
            Entry::Colors { old } => tool(Part::Colors, old),
            Entry::Style { old } => tool(Part::Style, old),
            Entry::Icons { old } => tool(Part::Icons, old),
            Entry::Cursor { old } => tool(Part::Cursor, old),
            Entry::Layout { script, state } => {
                // with a captured state, desktop widgets come back from it (in place) rather
                // than from the dump, which can't position them
                let script = match state {
                    Some(_) => without_desktop_widgets(script),
                    None => script.clone(),
                };
                let mut v = vec![Action::EvaluateScript {
                    what: "prelude + layout from before".into(),
                    script: format!("{}{script}", prelude()),
                    delay_ms: 0,
                }];
                v.extend(state.as_deref().map(settle));
                v
            }
            Entry::Wallpaper { old } => old
                .iter()
                .map(|u| Action::Exec(vec!["plasma-apply-wallpaperimage".into(), file_url_path(u).display().to_string()]))
                .collect(),
            Entry::File { path, old } => {
                kitty |= path.starts_with(paths.kitty_dir());
                vec![match old {
                    FileState::Missing => Action::Remove(path.clone()),
                    FileState::Link(t) => Action::Symlink { link: path.clone(), target: t.clone() },
                    FileState::Content(c) => Action::WriteFile { path: path.clone(), contents: c.clone() },
                }]
            }
            Entry::Unit { unit, old } => {
                vec![Action::Unit { unit: unit.clone(), enable: Some(old.enabled), active: Some(old.active) }]
            }
        };
        for a in actions {
            if let Err(err) = sys.act(&a) {
                errors.push(err.to_string());
            }
        }
    }
    let tail = kwin.then_some(Action::KWinReconfigure).into_iter().chain(kitty.then_some(Action::ReloadKitty));
    for a in tail {
        if let Err(err) = sys.act(&a) {
            errors.push(err.to_string());
        }
    }
    errors
}

fn tool(part: Part, old: &Option<String>) -> Vec<Action> {
    old.iter().map(|v| Action::Exec(vec![part.spec().3.into(), v.clone()])).collect()
}

/// Undo a finished switch and mark its journal undone.
pub fn undo<S: Sys>(sys: &mut S, paths: &Paths, journal: &mut Journal) -> Result<Vec<String>, std::io::Error> {
    let errors = undo_entries(sys, paths, &journal.entries);
    journal.set_status(Status::Undone)?;
    Ok(errors)
}

/// Which rice is on screen, when `current` is missing: match the Plasma style, then the colours.
pub fn infer_current<'a, S: Sys>(sys: &mut S, rices: &'a [Manifest]) -> Option<&'a Manifest> {
    let style = sys.kread("plasmarc", &["Theme".into()], "name").ok().flatten();
    let colors = sys.kread("kdeglobals", &["General".into()], "ColorScheme").ok().flatten();
    let looks = |m: &Manifest| {
        let mut v = vec![m.look(None)];
        if m.daynight.is_some() {
            v.push(m.look(Some(MoodKind::Light)));
            v.push(m.look(Some(MoodKind::Dark)));
        }
        v
    };
    let by = |pick: &dyn Fn(&crate::manifest::Look) -> Option<String>, want: &Option<String>| {
        want.as_ref().and_then(|w| rices.iter().find(|m| looks(m).iter().any(|l| pick(l).as_ref() == Some(w))))
    };
    by(&|l| l.style.clone(), &style).or_else(|| by(&|l| l.colors.clone(), &colors))
}

/// Problems that would make `apply` fail or look wrong, found without changing anything.
pub fn doctor<S: Sys>(sys: &mut S, paths: &Paths, m: &Manifest) -> Vec<String> {
    let mut problems = vec![];
    let h = |p: &str| paths.home.join(p);
    let mut looks = vec![m.look(None)];
    if m.daynight.is_some() {
        looks.push(m.look(Some(MoodKind::Light)));
        looks.push(m.look(Some(MoodKind::Dark)));
    }
    let icon_dirs = [h(".local/share/icons"), h(".icons"), PathBuf::from("/usr/share/icons")];
    for look in &looks {
        let mut check = |what: &str, name: &Option<String>, candidates: Vec<PathBuf>| {
            if let Some(n) = name
                && !candidates.iter().any(|c| sys.exists(c))
            {
                problems.push(format!("{what} {n:?} is not installed"));
            }
        };
        let colors = look.colors.as_ref().map(|n| format!("{n}.colors"));
        check("colour scheme", &look.colors, colors.iter().flat_map(|f| [h(".local/share/color-schemes").join(f), Path::new("/usr/share/color-schemes").join(f)]).collect());
        check("Plasma style", &look.style, look.style.iter().flat_map(|n| [h(".local/share/plasma/desktoptheme").join(n), Path::new("/usr/share/plasma/desktoptheme").join(n)]).collect());
        check("icon theme", &look.icons, look.icons.iter().flat_map(|n| icon_dirs.iter().map(move |d| d.join(n).join("index.theme"))).collect());
        check("cursor theme", &look.cursor, look.cursor.iter().flat_map(|n| icon_dirs.iter().map(move |d| d.join(n).join("cursors"))).collect());
        if let Some(w) = &look.wallpaper
            && !sys.exists(&paths.expand(w))
        {
            problems.push(format!("wallpaper {w} doesn't exist"));
        }
    }
    let mut seen = BTreeSet::new();
    problems.retain(|p| seen.insert(p.clone()));
    for (link, target) in &m.links {
        if !sys.exists(&paths.expand(target)) {
            problems.push(format!("link {link} points at {target}, which doesn't exist"));
        }
    }
    for unit in &m.services.enable {
        if !sys.unit_exists(unit) {
            problems.push(format!("service {unit} is not installed"));
        }
    }
    problems
}

#[cfg(test)]
mod tests;
