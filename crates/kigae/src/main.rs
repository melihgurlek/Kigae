//! `kigae`: the CLI (SPEC.md §9).

use std::fs::File;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand, ValueEnum};
use kigae_core::config::{Config, load_base};
use kigae_core::discover::{Rice, discover};
use kigae_core::engine::{self, Opts, Paths, Target};
use kigae_core::journal::{Journal, Status};
use kigae_core::manifest::{Manifest, MoodKind, UnsetPolicy};
use kigae_core::sys::{DryRun, FileState, RealSys, Sys};
use kigae_core::install;
use kigae_core::capture;

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
    Deep { id: String },
    /// Run the D-Bus service and overlay
    Daemon,
    /// Ask the daemon to show the overlay (bound to Meta+Shift+T)
    Overlay,
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
    let (rices, warnings) = discover(&paths.expand(&cfg.repo), &cfg.branches, &paths.expand(&cfg.manifests));
    for w in warnings {
        eprintln!("kigae: warning: {w}");
    }
    let manifests = paths.expand(&cfg.manifests);
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
        Command::Daemon | Command::Overlay => Err("not built yet (phase 2 in PLAN.md)".into()),
        Command::Shoot { .. } => Err("not built yet (phase 3 in PLAN.md)".into()),
        Command::Deep { .. } => Err("not built yet (phase 4 in PLAN.md)".into()),
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
    let problems = engine::doctor(&mut real, &c.paths, &to.manifest);
    if !problems.is_empty() {
        let list = problems.join("\n  ");
        if !force {
            return Err(format!("{id} isn't ready (kigae install {id}, or --force):\n  {list}"));
        }
        eprintln!("kigae: warning, applying anyway:\n  {list}");
    }
    let layout_js = to.layout_js().map_err(|e| e.to_string())?;
    let from_m = from.map(|r| c.effective(&r.manifest));
    let to_m = c.effective(&to.manifest);
    let opts = Opts { mood: mood.map(Into::into), unset: c.policy, no_journal: dry_run, ..Opts::default() };
    let target = Target { manifest: &to_m, layout_js };
    let from_label = from.map(Rice::id).unwrap_or("?");
    if dry_run {
        let mut sys = DryRun::new(real);
        let out = engine::apply(&mut sys, &c.paths, from_m.as_ref(), target, opts).map_err(|e| e.to_string())?;
        println!("{from_label} → {id}{}:", mood_label(out.mood));
        for a in &sys.actions {
            println!("  {a}");
        }
        if sys.actions.is_empty() {
            println!("  (nothing to change)");
        }
        return Ok(());
    }
    let _lock = lock()?;
    let out = engine::apply(&mut real, &c.paths, from_m.as_ref(), target, opts).map_err(|e| e.to_string())?;
    println!("{from_label} → {id}{}: {} changes", mood_label(out.mood), out.journal.entries.len());
    for w in &out.warnings {
        eprintln!("kigae: warning: {w}");
    }
    Ok(())
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
    let opts = Opts { mood: mood.map(Into::into), only_mood: true, unset: c.policy, no_journal: dry_run };
    let target = Target { manifest: &m, layout_js: None };
    if dry_run {
        let mut sys = DryRun::new(real);
        let out = engine::apply(&mut sys, &c.paths, Some(&m), target, opts).map_err(|e| e.to_string())?;
        println!("{}{}:", rice.id(), mood_label(out.mood));
        for a in &sys.actions {
            println!("  {a}");
        }
        return Ok(());
    }
    let _lock = lock()?;
    let out = engine::apply(&mut real, &c.paths, Some(&m), target, opts).map_err(|e| e.to_string())?;
    println!("{}{}: {} changes", rice.id(), mood_label(out.mood), out.journal.entries.len());
    for w in &out.warnings {
        eprintln!("kigae: warning: {w}");
    }
    Ok(())
}

fn undo(list: bool, ts: Option<u64>) -> Res {
    let paths = Paths::from_env().ok_or("HOME is not set")?;
    let journals = Journal::list(&paths.undo_dir()).map_err(|e| e.to_string())?;
    if list {
        for j in &journals {
            let status = match j.status {
                Status::Done => "",
                Status::InProgress => "  (interrupted)",
                Status::RolledBack => "  (rolled back)",
                Status::Undone => "  (undone)",
            };
            let from = j.from.as_deref().unwrap_or("?");
            println!("{}  {from} → {}{}  {} changes{status}", j.ts, j.to, mood_label(j.mood.as_deref().map(|m| if m == "light" { MoodKind::Light } else { MoodKind::Dark })), j.entries.len());
        }
        return Ok(());
    }
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
