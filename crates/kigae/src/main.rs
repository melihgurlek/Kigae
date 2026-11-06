//! `kigae`: the CLI (SPEC.md §9).

use std::fs::File;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand, ValueEnum};
use kigae_core::config::{Config, config_dir, load_base};
use kigae_core::discover::{Rice, discover};
use kigae_core::engine::{self, Opts, Paths, Target};
use kigae_core::journal::{Journal, Status};
use kigae_core::manifest::{Manifest, MoodKind, UnsetPolicy};
use kigae_core::sys::{Action, DryRun, FileState, RealSys, Sys, Traced};
use kigae_core::card;
use kigae_core::install;
use kigae_core::capture;
use kigae_core::deep as deep_switch;

#[cfg(feature = "overlay")]
mod overlay;
#[cfg(feature = "overlay")]
mod setup;

#[derive(Parser)]
#[command(version, about = "Live rice switcher for KDE Plasma")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Clone, Copy, ValueEnum)]
enum Mood {
    Light,
    Dark,
}

impl From<Mood> for MoodKind {
    fn from(m: Mood) -> Self {
        match m {
            Mood::Light => MoodKind::Light,
            Mood::Dark => MoodKind::Dark,
        }
    }
}

#[derive(Subcommand)]
enum Command {
    /// List the rices found, current one marked
    List,
    /// Print the active rice
    Current,
    /// Switch to a rice
    Apply {
        id: String,
        /// Print what would change, change nothing
        #[arg(long)]
        dry_run: bool,
        /// Day or night look (default: Plasma's NightTime schedule)
        #[arg(long)]
        mood: Option<Mood>,
        /// The rice on screen now, when Kigae can't tell
        #[arg(long)]
        from: Option<String>,
        /// Apply even if `doctor` finds problems
        #[arg(long)]
        force: bool,
    },
    /// Re-apply the day or night look of the active rice
    Mood {
        mood: Option<Mood>,
        #[arg(long)]
        dry_run: bool,
    },
    /// Put back what the last switch (or the given one) changed
    Undo {
        /// Show the switches Kigae remembers
        #[arg(long)]
        list: bool,
        /// Timestamp of the switch to undo (from --list)
        ts: Option<u64>,
    },
    /// Copy a rice's [install] assets into $HOME once
    Install {
        id: String,
        #[arg(long)]
        dry_run: bool,
        /// Overwrite files that exist with other contents
        #[arg(long)]
        force: bool,
    },
    /// Remove exactly what `install` copied
    Uninstall { id: String },
    /// Read the rice on screen into manifests/<id>.toml and manifests/<id>/layout.js
    Capture {
        id: String,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        kanji: Option<String>,
        #[arg(long)]
        tagline: Option<String>,
        /// The dotfiles branch the rice's files are on
        #[arg(long)]
        branch: Option<String>,
        /// Print the manifest and layout, write nothing
        #[arg(long)]
        dry_run: bool,
        /// Overwrite an existing manifest
        #[arg(long)]
        force: bool,
    },
    /// Check that rices' parts are installed, without applying
    Doctor { ids: Vec<String> },
    /// Apply each rice in turn and take its preview screenshot
    Shoot { ids: Vec<String> },
    /// Switch login screen and GRUB (one pkexec prompt)
    Deep {
        id: String,
        /// Print what would change, change nothing
        #[arg(long)]
        dry_run: bool,
        /// Run the rice's GRUB script even if GRUB already shows its theme
        #[arg(long)]
        grub: bool,
    },
    /// Let every switch set the login screen too: installs a root helper and a polkit rule
    /// (asks for your password once)
    SetupLogin {
        /// Remove the helper and the polkit rule
        #[arg(long)]
        remove: bool,
    },
    /// Run the D-Bus service and overlay
    Daemon,
    /// Ask the daemon to show the overlay (bound to Meta+Shift+T)
    Overlay,
    /// Run the daemon at login (systemd user unit, D-Bus activation) and bind Meta+Shift+T
    Setup {
        /// Stop the daemon, remove the unit and release the shortcut
        #[arg(long)]
        remove: bool,
    },
}

struct Ctx {
    paths: Paths,
    manifests: PathBuf,
    rices: Vec<Rice>,
    base: Option<Manifest>,
    policy: UnsetPolicy,
}

type Res<T = ()> = Result<T, String>;

fn ctx() -> Res<Ctx> {
    let paths = Paths::from_env().ok_or("HOME is not set")?;
    let cfg = Config::load().map_err(|e| e.to_string())?;
    let base = load_base().map_err(|e| e.to_string())?;
    let manifests = cfg.manifests_dir().map(|m| paths.expand(&m)).unwrap_or_else(|| config_dir().join("manifests"));
    let (rices, warnings) = discover(&paths.expand(&cfg.repo), &cfg.branches, &manifests);
    for w in warnings {
        eprintln!("kigae: warning: {w}");
    }
    Ok(Ctx { paths, manifests, rices, base, policy: cfg.unset.policy() })
}

impl Ctx {
    fn rice(&self, id: &str) -> Res<&Rice> {
        self.rices.iter().find(|r| r.id() == id).ok_or_else(|| {
            let ids: Vec<&str> = self.rices.iter().map(Rice::id).collect();
            format!("no rice {id:?} (found: {})", ids.join(", "))
        })
    }

    /// The active rice: `current` if Kigae switched last, else a guess from the Plasma style.
    fn current(&self, sys: &mut impl Sys) -> Option<(&Rice, bool)> {
        if let Ok(FileState::Content(id)) = sys.file_state(&self.paths.current())
            && let Ok(r) = self.rice(id.trim())
        {
            return Some((r, false));
        }
        let manifests: Vec<Manifest> = self.rices.iter().map(|r| r.manifest.clone()).collect();
        let m = engine::infer_current(sys, &manifests)?;
        self.rice(&m.id).ok().map(|r| (r, true))
    }

    fn effective(&self, m: &Manifest) -> Manifest {
        m.with_base(self.base.as_ref(), self.policy)
    }
}

/// One switch at a time, whoever starts it (SPEC.md §1).
fn lock() -> Res<File> {
    let dir = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    let path = dir.join("kigae.lock");
    let f = File::create(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    f.try_lock().map_err(|_| "another switch is running".to_string())?;
    Ok(f)
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.command {
        Command::List => list(),
        Command::Current => current(),
        Command::Apply { id, dry_run, mood, from, force } => apply(&id, dry_run, mood, from.as_deref(), force),
        Command::Mood { mood, dry_run } => mood_cmd(mood, dry_run),
        Command::Undo { list, ts } => undo(list, ts),
        Command::Install { id, dry_run, force } => install_cmd(&id, dry_run, force),
        Command::Uninstall { id } => uninstall(&id),
        Command::Capture { id, name, kanji, tagline, branch, dry_run, force } => {
            capture_cmd(capture::Info { id, name, kanji, tagline, branch }, dry_run, force)
        }
        Command::Doctor { ids } => doctor(&ids),
        #[cfg(feature = "overlay")]
        Command::Daemon => overlay::run(),
        #[cfg(feature = "overlay")]
        Command::Overlay => overlay::show(),
        #[cfg(feature = "overlay")]
        Command::Setup { remove } => if remove { setup::remove() } else { setup::install() },
        #[cfg(not(feature = "overlay"))]
        Command::Daemon | Command::Overlay | Command::Setup { .. } => Err("built without the overlay feature".into()),
        Command::Shoot { ids } => shoot(&ids),
        Command::Deep { id, dry_run, grub } => deep(&id, dry_run, grub),
        Command::SetupLogin { remove } => setup_login(remove),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("kigae: {e}");
            ExitCode::FAILURE
        }
    }
}

fn list() -> Res {
    let c = ctx()?;
    let cur = c.current(&mut RealSys::new()).map(|(r, _)| r.id().to_string());
    for r in &c.rices {
        let m = &r.manifest;
        let mark = if cur.as_deref() == Some(r.id()) { "*" } else { " " };
        let kanji = m.kanji.as_deref().map(|k| format!(" {k}")).unwrap_or_default();
        let dn = if m.daynight.is_some() { "  day/night" } else { "" };
        let from = r.branch.as_deref().unwrap_or("local");
        println!("{mark} {:<12} {}{kanji}  ({from}){dn}", r.id(), m.name);
    }
    Ok(())
}

fn current() -> Res {
    let c = ctx()?;
    match c.current(&mut RealSys::new()) {
        Some((r, false)) => println!("{}", r.id()),
        Some((r, true)) => println!("{} (guessed from the Plasma style)", r.id()),
        None => return Err("can't tell which rice is on screen".into()),
    }
    Ok(())
}

fn apply(id: &str, dry_run: bool, mood: Option<Mood>, from: Option<&str>, force: bool) -> Res {
    let c = ctx()?;
    let mut real = RealSys::new();
    let to = c.rice(id)?;
    let from = match from {
        Some(f) => Some(c.rice(f)?),
        None => c.current(&mut real).map(|(r, _)| r),
    };
    // check what will be applied: base.toml's parts too
    let to_m = c.effective(&to.manifest);
    let problems = engine::doctor(&mut real, &c.paths, &to_m);
    if !problems.is_empty() {
        let list = problems.join("\n  ");
        if !force {
            return Err(format!("{id} isn't ready (kigae install {id}, or --force):\n  {list}"));
        }
        eprintln!("kigae: warning, applying anyway:\n  {list}");
    }
    let layout_js = to.layout_js().map_err(|e| e.to_string())?;
    let from_m = from.map(|r| c.effective(&r.manifest));
    let opts = Opts { mood: mood.map(Into::into), unset: c.policy, ..Opts::default() };
    let target = Target { manifest: &to_m, layout_js };
    let label = format!("{} → {id}", from.map(Rice::id).unwrap_or("?"));
    if dry_run {
        return print_plan(real, &c.paths, from_m.as_ref(), target, opts, &label);
    }
    report(&label, &run_switch(real, &c.paths, from_m.as_ref(), target, opts)?);
    login_hint(to);
    Ok(())
}

/// Switch for real: one at a time, each step timed with `KIGAE_TRACE=1`.
fn run_switch(mut real: RealSys, paths: &Paths, from: Option<&Manifest>, to: Target<'_>, opts: Opts) -> Res<engine::Outcome> {
    let _lock = lock()?;
    if std::env::var_os("KIGAE_TRACE").is_some() {
        engine::apply(&mut Traced::new(real), paths, from, to, opts)
    } else {
        engine::apply(&mut real, paths, from, to, opts)
    }
    .map_err(|e| e.to_string())
}

/// `--dry-run`: print what the switch would change, change nothing.
fn print_plan(real: RealSys, paths: &Paths, from: Option<&Manifest>, to: Target<'_>, opts: Opts, label: &str) -> Res {
    let mut sys = DryRun::new(real);
    let out = engine::apply(&mut sys, paths, from, to, Opts { no_journal: true, ..opts }).map_err(|e| e.to_string())?;
    println!("{label}{}:", mood_label(out.mood));
    for a in &sys.actions {
        println!("  {a}");
    }
    if sys.actions.is_empty() {
        println!("  (nothing to change)");
    }
    Ok(())
}

fn report(label: &str, out: &engine::Outcome) {
    println!("{label}{}: {} changes", mood_label(out.mood), out.journal.entries.len());
    for w in &out.warnings {
        eprintln!("kigae: warning: {w}");
    }
}

/// Switch to `id` for the overlay and `shoot`: no dry run, no `--force`, the rice on screen as
/// `from`. The rice already on screen only changes its day/night look, like `kigae mood`.
fn switch(id: &str, mood: Option<MoodKind>) -> Res<engine::Outcome> {
    let c = ctx()?;
    let mut real = RealSys::new();
    let to = c.rice(id)?;
    let to_m = c.effective(&to.manifest);
    let problems = engine::doctor(&mut real, &c.paths, &to_m);
    if !problems.is_empty() {
        return Err(format!("{} isn't ready: {}", to.manifest.name, problems.join("; ")));
    }
    let from = c.current(&mut real).map(|(r, _)| r);
    let only_mood = from.is_some_and(|r| r.id() == to.id());
    let from_m = from.map(|r| c.effective(&r.manifest));
    let layout_js = if only_mood { None } else { to.layout_js().map_err(|e| e.to_string())? };
    let opts = Opts { mood, only_mood, unset: c.policy, ..Opts::default() };
    run_switch(real, &c.paths, from_m.as_ref(), Target { manifest: &to_m, layout_js }, opts)
}

/// The sun just rose or set: put up the active rice's look for now, if it has two (SPEC.md §6).
#[cfg(feature = "overlay")]
fn follow_sun() -> Res<Option<String>> {
    let c = ctx()?;
    let mut real = RealSys::new();
    let Some((rice, _)) = c.current(&mut real) else { return Ok(None) };
    if rice.manifest.daynight.is_none() {
        return Ok(None);
    }
    let m = c.effective(&rice.manifest);
    let opts = Opts { only_mood: true, unset: c.policy, ..Opts::default() };
    let out = run_switch(real, &c.paths, Some(&m), Target { manifest: &m, layout_js: None }, opts)?;
    Ok(Some(format!("{}{}", rice.id(), mood_label(out.mood))))
}

/// Wear each rice (each look of a day/night rice), hide the windows and screenshot it for its
/// card; then put back the rice and look that were on.
fn shoot(ids: &[String]) -> Res {
    let c = ctx()?;
    let mut real = RealSys::new();
    let (start, _) = c.current(&mut real).ok_or("can't tell which rice is on screen; apply one first")?;
    let start_id = start.id().to_string();
    let last = Journal::list(&c.paths.undo_dir()).ok().and_then(|j| j.into_iter().last());
    let start_mood = last.filter(|j| j.to == start_id).and_then(|j| j.mood).and_then(|m| MoodKind::parse(&m));
    let mut rices: Vec<&Rice> =
        if ids.is_empty() { c.rices.iter().collect() } else { ids.iter().map(|i| c.rice(i)).collect::<Res<_>>()? };
    // the rice on screen first, and last of its looks the one it's wearing: fewer switches
    rices.sort_by_key(|r| r.id() != start_id);
    let dir = card::shots_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut failed = vec![];
    for r in rices {
        let mut moods = if r.manifest.daynight.is_some() { vec![MoodKind::Light, MoodKind::Dark] } else { vec![] };
        if r.id() == start_id && start_mood == Some(MoodKind::Light) {
            moods.reverse();
        }
        let moods: Vec<Option<MoodKind>> = if moods.is_empty() { vec![None] } else { moods.into_iter().map(Some).collect() };
        for mood in moods {
            let label = format!("{}{}", r.id(), mood_label(mood));
            let path = dir.join(card::shot_name(r.id(), mood));
            match switch(r.id(), mood).and_then(|_| take_shot(&mut real, &path)) {
                Ok(()) => println!("{label}: {}", path.display()),
                Err(e) => {
                    eprintln!("kigae: {label}: {e}");
                    failed.push(label);
                }
            }
        }
    }
    let back = switch(&start_id, start_mood).map_err(|e| format!("putting {start_id} back: {e} (kigae apply {start_id})"));
    println!("back on {start_id}{}", mood_label(start_mood));
    back?;
    if failed.is_empty() { Ok(()) } else { Err(format!("no shot of {}", failed.join(", "))) }
}

/// Let the switch settle, hide every window, screenshot the screen, bring the windows back.
fn take_shot(sys: &mut RealSys, path: &std::path::Path) -> Res {
    use std::time::Duration;
    // plasmashell's new panels, the wallpaper crossfade and KWin's effects
    std::thread::sleep(Duration::from_millis(2500));
    sys.act(&Action::ShowDesktop(true)).map_err(|e| e.to_string())?;
    std::thread::sleep(Duration::from_millis(900));
    let _ = std::fs::remove_file(path);
    let shot = std::process::Command::new("spectacle")
        .args(["--background", "--nonotify", "--fullscreen", "--output"])
        .arg(path)
        .status()
        .map_err(|e| format!("spectacle: {e}"));
    // spectacle may return before the file is written
    for _ in 0..50 {
        if path.is_file() {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let _ = sys.act(&Action::ShowDesktop(false));
    match shot? {
        s if !s.success() => Err(format!("spectacle exited with {s}")),
        _ if !path.is_file() => Err("spectacle saved nothing".into()),
        _ => Ok(()),
    }
}

fn mood_label(m: Option<MoodKind>) -> String {
    m.map(|m| format!(" ({})", m.name())).unwrap_or_default()
}

fn mood_cmd(mood: Option<Mood>, dry_run: bool) -> Res {
    let c = ctx()?;
    let mut real = RealSys::new();
    let (rice, _) = c.current(&mut real).ok_or("can't tell which rice is on screen")?;
    if rice.manifest.daynight.is_none() {
        println!("{} has no day/night looks", rice.id());
        return Ok(());
    }
    let m = c.effective(&rice.manifest);
    let opts = Opts { mood: mood.map(Into::into), only_mood: true, unset: c.policy, ..Opts::default() };
    let target = Target { manifest: &m, layout_js: None };
    if dry_run {
        return print_plan(real, &c.paths, Some(&m), target, opts, rice.id());
    }
    report(rice.id(), &run_switch(real, &c.paths, Some(&m), target, opts)?);
    Ok(())
}

/// Undo a remembered switch: the one at `ts`, else the last one that changed something.
fn undo_switch(ts: Option<u64>) -> Res<(Journal, Vec<String>)> {
    let paths = Paths::from_env().ok_or("HOME is not set")?;
    let journals = Journal::list(&paths.undo_dir()).map_err(|e| e.to_string())?;
    let mut j = match ts {
        Some(ts) => journals.into_iter().find(|j| j.ts == ts).ok_or(format!("no switch at {ts}"))?,
        None => journals
            .into_iter()
            .rev()
            .find(|j| matches!(j.status, Status::Done | Status::InProgress) && !j.entries.is_empty())
            .ok_or("nothing to undo")?,
    };
    let _lock = lock()?;
    let errors = engine::undo(&mut RealSys::new(), &paths, &mut j).map_err(|e| e.to_string())?;
    Ok((j, errors))
}

fn undo(list: bool, ts: Option<u64>) -> Res {
    if list {
        let paths = Paths::from_env().ok_or("HOME is not set")?;
        for j in &Journal::list(&paths.undo_dir()).map_err(|e| e.to_string())? {
            let status = match j.status {
                Status::Done => "",
                Status::InProgress => "  (interrupted)",
                Status::RolledBack => "  (rolled back)",
                Status::Undone => "  (undone)",
            };
            let from = j.from.as_deref().unwrap_or("?");
            let mood = mood_label(j.mood.as_deref().and_then(MoodKind::parse));
            println!("{}  {from} → {}{mood}  {} changes{status}", j.ts, j.to, j.entries.len());
        }
        return Ok(());
    }
    let (j, errors) = undo_switch(ts)?;
    println!("undid {} → {} ({} changes)", j.from.as_deref().unwrap_or("?"), j.to, j.entries.len());
    if errors.is_empty() {
        Ok(())
    } else {
        Err(format!("some things couldn't be put back:\n  {}", errors.join("\n  ")))
    }
}

fn install_cmd(id: &str, dry_run: bool, force: bool) -> Res {
    let c = ctx()?;
    let rice = c.rice(id)?;
    let items = install::plan(rice, &c.paths).map_err(|e| e.to_string())?;
    let count = |s| items.iter().filter(|i| i.state == s).count();
    let (new, differs, same) = (count(install::State::New), count(install::State::Differs), count(install::State::Same));
    if dry_run {
        for i in items.iter().filter(|i| i.state != install::State::Same) {
            let what = if i.state == install::State::New { "new" } else { "differs" };
            println!("  {what:<8} {}", i.dest.display());
        }
        println!("{id}: {new} new, {differs} differ, {same} already there");
        return Ok(());
    }
    let written = install::run(&items, force, &c.paths, id)?;
    println!("{id}: wrote {} files, {same} were already there", written.len());
    Ok(())
}

fn uninstall(id: &str) -> Res {
    let paths = Paths::from_env().ok_or("HOME is not set")?;
    let removed = install::uninstall(&paths, id)?;
    println!("{id}: removed {} files", removed.len());
    Ok(())
}

fn doctor(ids: &[String]) -> Res {
    let c = ctx()?;
    let mut sys = RealSys::new();
    let rices: Vec<&Rice> = if ids.is_empty() { c.rices.iter().collect() } else { ids.iter().map(|i| c.rice(i)).collect::<Res<_>>()? };
    let mut bad = false;
    for r in rices {
        let mut problems = engine::doctor(&mut sys, &c.paths, &c.effective(&r.manifest));
        if let Err(e) = r.layout_js() {
            problems.push(format!("layout script: {e}"));
        }
        if problems.is_empty() {
            println!("{}: ok", r.id());
        } else {
            bad = true;
            println!("{}:\n  {}", r.id(), problems.join("\n  "));
        }
    }
    if bad { Err("problems found".into()) } else { Ok(()) }
}

fn capture_cmd(info: capture::Info, dry_run: bool, force: bool) -> Res {
    // the id has to be one a manifest accepts
    Manifest::parse(&format!("schema = 1\nid = {:?}\nname = \"x\"", info.id)).map_err(|e| e.to_string())?;
    let c = ctx()?;
    if let Ok(r) = c.rice(&info.id)
        && r.local_dir.is_none()
    {
        return Err(format!("{} already has a rice.toml on {}; capture under another id", info.id, r.branch.as_deref().unwrap_or("?")));
    }
    let toml_path = c.manifests.join(format!("{}.toml", info.id));
    let layout_path = c.manifests.join(&info.id).join("layout.js");
    if !dry_run && !force && toml_path.exists() {
        return Err(format!("{} exists (use --force to overwrite)", toml_path.display()));
    }
    let mut sys = RealSys::new();
    let out = capture::capture(&mut sys, &c.paths, &info, c.base.as_ref()).map_err(|e| e.to_string())?;
    let m = Manifest::parse(&out.manifest).map_err(|e| format!("captured manifest doesn't parse (a bug): {e}"))?;
    if dry_run {
        println!("── {}\n{}", toml_path.display(), out.manifest);
        println!("── {}\n{}", layout_path.display(), out.layout);
    } else {
        let write = |p: &std::path::Path, text: &str| -> Res {
            std::fs::create_dir_all(p.parent().expect("has parent")).map_err(|e| e.to_string())?;
            std::fs::write(p, text).map_err(|e| format!("{}: {e}", p.display()))
        };
        write(&toml_path, &out.manifest)?;
        write(&layout_path, &out.layout)?;
        println!("captured {} → {}", info.id, toml_path.display());
    }
    let problems = engine::doctor(&mut sys, &c.paths, &m);
    for p in &problems {
        println!("  problem: {p}");
    }
    for n in &out.notes {
        println!("  note: {n}");
    }
    Ok(())
}


/// After a switch: say so when the login screen still belongs to another rice (the switch sets
/// it only once `kigae setup-login` has run).
fn login_hint(r: &Rice) {
    if let Some(t) = &r.manifest.deep.sddm {
        let now = deep_switch::login_theme();
        if now.as_deref() != Some(t.as_str()) {
            let follow = if std::path::Path::new(deep_switch::LOGIN_HELPER).exists() {
                ""
            } else {
                ", or `kigae setup-login` once makes every switch do it"
            };
            eprintln!("kigae: the login screen is still {}; `kigae deep {}` switches it{follow}",
                      now.as_deref().unwrap_or("the default"), r.id());
        }
    }
}

/// Install (or remove) the login helper and its polkit action, behind one pkexec prompt.
fn setup_login(remove: bool) -> Res {
    let mut cmd = std::process::Command::new("pkexec");
    cmd.args(["/bin/sh", "-c", deep_switch::SETUP_SCRIPT, "sh"]);
    if remove {
        cmd.arg("--remove");
    } else {
        cmd.args([deep_switch::LOGIN_SCRIPT, deep_switch::LOGIN_POLICY_XML]);
    }
    match cmd.status().map_err(|e| format!("pkexec: {e}"))?.code() {
        Some(0) => {
            if !remove {
                println!("every switch now sets the login screen of rices with [deep] sddm (no password)");
            }
            Ok(())
        }
        Some(126) | Some(127) => Err("not authorised (pkexec was cancelled or denied)".into()),
        c => Err(format!("setup failed (exit {})", c.map_or("?".into(), |c| c.to_string()))),
    }
}

/// SDDM and GRUB for a rice, behind one pkexec prompt (SPEC.md §11).
fn deep(id: &str, dry_run: bool, grub_again: bool) -> Res {
    let c = ctx()?;
    let r = c.rice(id)?;
    let d = &r.manifest.deep;
    if d.sddm.is_none() && d.grub.is_none() {
        return Err(format!("{id} has no [deep] parts (login screen, GRUB)"));
    }
    let mut theme = String::new();
    if let Some(t) = &d.sddm {
        if !deep_switch::sddm_installed(t) {
            return Err(format!("SDDM theme {t:?} is not installed in {}: install it with the rice's own installer first", deep_switch::SDDM_THEMES));
        }
        if deep_switch::login_theme().as_deref() == Some(t.as_str()) {
            println!("login: already {t}");
        } else {
            theme = t.clone();
        }
    }
    let mut grub = String::new();
    let mut scratch = None;
    if let Some(g) = &d.grub {
        if deep_switch::grub_shows(id) && !grub_again {
            println!("grub: already {id} (--grub runs its script again)");
        } else {
            let (script, dir) = grub_script(r, g)?;
            grub = script.to_string_lossy().into_owned();
            scratch = dir;
        }
    }
    if theme.is_empty() && grub.is_empty() {
        return Ok(());
    }
    if dry_run {
        if !theme.is_empty() {
            println!("would set the login to {theme} ({} [Theme] Current)", deep_switch::SDDM_CONF);
        }
        if !grub.is_empty() {
            println!("would run {grub} as root");
        }
        if let Some(dir) = scratch {
            let _ = std::fs::remove_dir_all(dir);
        }
        return Ok(());
    }
    let status = std::process::Command::new("pkexec")
        .args(["/bin/sh", "-c", deep_switch::ROOT_SCRIPT, "sh", deep_switch::LOGIN_SCRIPT, &theme, &grub])
        .status()
        .map_err(|e| format!("pkexec: {e}"));
    if let Some(dir) = scratch {
        let _ = std::fs::remove_dir_all(dir);
    }
    match status?.code() {
        Some(0) => Ok(()),
        Some(126) | Some(127) => Err("not authorised (pkexec was cancelled or denied)".into()),
        c => Err(format!("the root part failed (exit {})", c.map_or("?".into(), |c| c.to_string()))),
    }
}

/// The rice's GRUB script on disk: next to a local manifest, or its folder taken out of the
/// rice's branch (git archive) into the runtime dir, since nothing is ever checked out.
fn grub_script(r: &Rice, path: &str) -> Res<(PathBuf, Option<PathBuf>)> {
    if let (Some(rest), Some(dir)) = (path.strip_prefix("./"), &r.local_dir) {
        let p = dir.join(rest);
        return if p.is_file() { Ok((p, None)) } else { Err(format!("{}: no such GRUB script", p.display())) };
    }
    let branch = r.branch.as_ref().ok_or_else(|| format!("{}: GRUB script {path:?} needs a branch", r.id()))?;
    let folder = std::path::Path::new(path).parent().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
    let run = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    let out = run.join(format!("kigae-deep-{}", r.id()));
    let _ = std::fs::remove_dir_all(&out);
    std::fs::create_dir_all(&out).map_err(|e| format!("{}: {e}", out.display()))?;
    let tar = std::process::Command::new("sh")
        .args(["-c", r#"git -C "$1" archive "$2" -- "$3" | tar -x -C "$4""#, "sh"])
        .arg(&r.repo).arg(branch).arg(if folder.is_empty() { "." } else { &folder }).arg(&out)
        .status()
        .map_err(|e| e.to_string())?;
    let script = out.join(path);
    if !tar.success() || !script.is_file() {
        let _ = std::fs::remove_dir_all(&out);
        return Err(format!("couldn't take {path} out of {branch}"));
    }
    Ok((script, Some(out)))
}
