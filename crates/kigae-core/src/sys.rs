//! Everything Kigae reads from or does to the desktop goes through [`Sys`], so the engine can be
//! run for real, recorded for `--dry-run`, or faked in tests (SPEC.md §1).

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};

pub type Result<T> = std::result::Result<T, SysError>;

#[derive(Debug, thiserror::Error)]
pub enum SysError {
    #[error("{cmd}: {msg}")]
    Command { cmd: String, msg: String },
    #[error("{path}: {err}")]
    Io { path: PathBuf, err: io::Error },
    #[error("D-Bus {call}: {err}")]
    DBus { call: &'static str, err: String },
}

/// What a path holds, for the journal: enough to put it back exactly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FileState {
    Missing,
    Link(PathBuf),
    Content(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnitState {
    pub enabled: bool,
    pub active: bool,
}

/// One change to the desktop. `Display` is what `--dry-run` prints.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    /// A program and its arguments.
    Exec(Vec<String>),
    /// `sh -c` (manifest hooks).
    Shell(String),
    /// `kwriteconfig6`; `value: None` deletes the key.
    KWrite { file: String, groups: Vec<String>, key: String, value: Option<String>, boolean: bool },
    KWinReconfigure,
    /// KWin's Show Desktop: hide every window (`kigae shoot`) or bring them back.
    ShowDesktop(bool),
    /// A plasmashell script; `delay_ms` lets panels created by the previous script settle.
    EvaluateScript { what: String, script: String, delay_ms: u64 },
    WriteFile { path: PathBuf, contents: String },
    Symlink { link: PathBuf, target: PathBuf },
    Remove(PathBuf),
    Unit { unit: String, enable: Option<bool>, active: Option<bool> },
    ReloadKitty,
}

impl fmt::Display for Action {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Action::Exec(argv) => write!(f, "{}", argv.join(" ")),
            Action::Shell(cmd) => write!(f, "sh -c {cmd:?}"),
            Action::KWrite { file, groups, key, value, .. } => {
                let path = format!("{file} [{}] {key}", groups.join("]["));
                match value {
                    Some(v) => write!(f, "kconfig {path} = {v:?}"),
                    None => write!(f, "kconfig {path} (delete)"),
                }
            }
            Action::KWinReconfigure => write!(f, "KWin reconfigure"),
            Action::ShowDesktop(on) => write!(f, "KWin show desktop {}", if *on { "on" } else { "off" }),
            Action::EvaluateScript { what, .. } => write!(f, "plasmashell script: {what}"),
            Action::WriteFile { path, contents } => {
                write!(f, "write {} ({} bytes)", path.display(), contents.len())
            }
            Action::Symlink { link, target } => {
                write!(f, "link {} -> {}", link.display(), target.display())
            }
            Action::Remove(p) => write!(f, "remove {}", p.display()),
            Action::Unit { unit, enable, active } => {
                let mut parts = vec![];
                if let Some(e) = enable {
                    parts.push(if *e { "enable" } else { "disable" });
                }
                if let Some(a) = active {
                    parts.push(if *a { "start" } else { "stop" });
                }
                write!(f, "systemctl --user {} {unit}", parts.join("+"))
            }
            Action::ReloadKitty => write!(f, "reload kitty (SIGUSR1)"),
        }
    }
}

pub trait Sys {
    fn kread(&mut self, file: &str, groups: &[String], key: &str) -> Result<Option<String>>;
    fn file_state(&mut self, path: &Path) -> Result<FileState>;
    fn exists(&mut self, path: &Path) -> bool;
    fn unit_state(&mut self, unit: &str) -> Result<UnitState>;
    fn unit_exists(&mut self, unit: &str) -> bool;
    /// `org.kde.PlasmaShell.dumpCurrentLayoutJS`: a script that rebuilds today's panels.
    fn dump_layout(&mut self) -> Result<String>;
    /// What the layout dump leaves out (tray settings, desktop widget positions), as JSON for
    /// `kigae.restore`.
    fn capture_layout(&mut self) -> Result<String>;
    /// The first desktop's wallpaper image (a `file://` URL), if it uses `org.kde.image`.
    fn wallpaper(&mut self) -> Result<Option<String>>;
    /// Is the sun up, by Plasma's NightTime schedule.
    fn is_day(&mut self) -> Result<bool>;
    /// The SDDM theme the login screen uses.
    fn login_theme(&mut self) -> Option<String>;
    /// Do it; returns what a plasmashell script printed (empty otherwise).
    fn act(&mut self, action: &Action) -> Result<String>;
}

// ───────────────────────────── real ─────────────────────────────

/// Talks to the live session.
#[derive(Default)]
pub struct RealSys {
    bus: Option<zbus::blocking::Connection>,
    /// Where KConfig files live, for in-process reads (`kconfig::dirs`).
    config_dirs: Option<Vec<std::path::PathBuf>>,
}

/// kreadconfig6 can't tell "unset" from "empty"; ask for this default to find out.
const UNSET: &str = "\u{1}kigae-unset";

impl RealSys {
    pub fn new() -> Self {
        Self::default()
    }

    fn bus(&mut self, call: &'static str) -> Result<&zbus::blocking::Connection> {
        if self.bus.is_none() {
            let c = zbus::blocking::Connection::session()
                .map_err(|e| SysError::DBus { call, err: e.to_string() })?;
            self.bus = Some(c);
        }
        Ok(self.bus.as_ref().expect("just set"))
    }

    fn call<B, R>(&mut self, call: &'static str, dest: &str, path: &str, iface: &str, method: &str, body: &B) -> Result<R>
    where
        B: serde::Serialize + zbus::zvariant::DynamicType,
        R: for<'d> zbus::zvariant::DynamicDeserialize<'d>,
    {
        let bus = self.bus(call)?;
        let err = |e: zbus::Error| SysError::DBus { call, err: e.to_string() };
        let reply = bus.call_method(Some(dest), path, Some(iface), method, body).map_err(err)?;
        reply.body().deserialize::<R>().map_err(err)
    }

    fn kwin_noreply<B>(&mut self, call: &'static str, method: &'static str, body: &B) -> Result<()>
    where
        B: serde::Serialize + zbus::zvariant::DynamicType,
    {
        self.noreply(call, "/KWin", "org.kde.KWin", "org.kde.KWin", method, body)
    }

    /// Send a method call without waiting for the answer.
    fn noreply<B>(&mut self, call: &'static str, path: &'static str, dest: &'static str, iface: &'static str, method: &'static str, body: &B) -> Result<()>
    where
        B: serde::Serialize + zbus::zvariant::DynamicType,
    {
        let bus = self.bus(call)?;
        let err = |e: zbus::Error| SysError::DBus { call, err: e.to_string() };
        let msg = zbus::message::Message::method_call(path, method)
            .map_err(err)?
            .destination(dest)
            .map_err(err)?
            .interface(iface)
            .map_err(err)?
            .with_flags(zbus::message::Flags::NoReplyExpected)
            .map_err(err)?
            .build(body)
            .map_err(err)?;
        bus.send(&msg).map_err(err)
    }

    /// A call on the user systemd manager.
    fn systemd<B, R>(&mut self, method: &'static str, body: &B) -> Result<R>
    where
        B: serde::Serialize + zbus::zvariant::DynamicType,
        R: for<'d> zbus::zvariant::DynamicDeserialize<'d>,
    {
        self.call(method, "org.freedesktop.systemd1", "/org/freedesktop/systemd1", "org.freedesktop.systemd1.Manager", method, body)
    }

    fn unit_file_state(&mut self, unit: &str) -> Result<String> {
        self.systemd("GetUnitFileState", &(unit,))
    }

    fn evaluate(&mut self, script: &str) -> Result<String> {
        self.call(
            "evaluateScript",
            "org.kde.plasmashell",
            "/PlasmaShell",
            "org.kde.PlasmaShell",
            "evaluateScript",
            &(script,),
        )
    }
}

pub fn run(argv: &[&str]) -> Result<String> {
    let cmd = argv.join(" ");
    let out = Command::new(argv[0])
        .args(&argv[1..])
        .stdin(Stdio::null())
        .output()
        .map_err(|e| SysError::Command { cmd: cmd.clone(), msg: e.to_string() })?;
    if !out.status.success() {
        let mut msg = String::from_utf8_lossy(&out.stderr).trim().to_string();
        if msg.is_empty() {
            msg = String::from_utf8_lossy(&out.stdout).trim().to_string();
        }
        if msg.is_empty() {
            msg = format!("exited with {}", out.status);
        }
        return Err(SysError::Command { cmd, msg });
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// How long a manifest hook may run before the switch gives up on it.
const HOOK_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// Run a manifest hook via `sh -c`. No pipes (a hook that backgrounds something would keep them
/// open and hang us): stdin and stdout are null, stderr goes to a temp file for the error message.
fn run_shell(cmd: &str, timeout: std::time::Duration) -> Result<()> {
    let fail = |msg: String| SysError::Command { cmd: format!("sh -c {cmd:?}"), msg };
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(0);
    let errfile = std::env::temp_dir().join(format!("kigae-hook-{}-{nanos}.err", std::process::id()));
    let stderr = fs::File::create(&errfile).map_err(|e| fail(e.to_string()))?;
    let result = (|| {
        let mut child = Command::new("sh")
            .args(["-c", cmd])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(stderr)
            .spawn()
            .map_err(|e| fail(e.to_string()))?;
        let start = std::time::Instant::now();
        let status = loop {
            if let Some(s) = child.try_wait().map_err(|e| fail(e.to_string()))? {
                break s;
            }
            if start.elapsed() >= timeout {
                let _ = child.kill();
                let _ = child.wait();
                return Err(fail(format!("timed out after {}s", timeout.as_secs_f32())));
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        };
        if status.success() {
            return Ok(());
        }
        let msg = fs::read_to_string(&errfile).unwrap_or_default().trim().to_string();
        Err(fail(if msg.is_empty() { format!("exited with {status}") } else { msg }))
    })();
    let _ = fs::remove_file(&errfile);
    result
}

fn io_err(path: &Path) -> impl FnOnce(io::Error) -> SysError + '_ {
    move |err| SysError::Io { path: path.to_path_buf(), err }
}

fn group_args(groups: &[String]) -> Vec<&str> {
    groups.iter().flat_map(|g| ["--group", g.as_str()]).collect()
}

impl Sys for RealSys {
    fn kread(&mut self, file: &str, groups: &[String], key: &str) -> Result<Option<String>> {
        let dirs = self.config_dirs.get_or_insert_with(crate::kconfig::dirs);
        if let Some(v) = crate::kconfig::read(dirs, file, groups, key) {
            return Ok(v);
        }
        let mut argv = vec!["kreadconfig6", "--file", file];
        argv.extend(group_args(groups));
        argv.extend(["--key", key, "--default", UNSET]);
        let out = run(&argv)?;
        let v = out.strip_suffix('\n').unwrap_or(&out);
        Ok((v != UNSET).then(|| v.to_string()))
    }

    fn file_state(&mut self, path: &Path) -> Result<FileState> {
        match fs::symlink_metadata(path) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(FileState::Missing),
            Err(e) => Err(io_err(path)(e)),
            Ok(m) if m.file_type().is_symlink() => {
                Ok(FileState::Link(fs::read_link(path).map_err(io_err(path))?))
            }
            Ok(_) => Ok(FileState::Content(fs::read_to_string(path).map_err(io_err(path))?)),
        }
    }

    fn exists(&mut self, path: &Path) -> bool {
        path.exists()
    }

    fn unit_state(&mut self, unit: &str) -> Result<UnitState> {
        // like `systemctl --user is-enabled` / `is-active`: no unit file or not loaded is "no"
        let enabled = self.unit_file_state(unit).is_ok_and(|s| s == "enabled");
        let unit_path: Result<zbus::zvariant::OwnedObjectPath> = self.systemd("GetUnit", &(unit,));
        let active = match unit_path {
            Ok(path) => {
                let v: zbus::zvariant::OwnedValue = self.call(
                    "systemd ActiveState",
                    "org.freedesktop.systemd1",
                    path.as_str(),
                    "org.freedesktop.DBus.Properties",
                    "Get",
                    &("org.freedesktop.systemd1.Unit", "ActiveState"),
                )?;
                String::try_from(v).is_ok_and(|s| s == "active")
            }
            Err(_) => false,
        };
        Ok(UnitState { enabled, active })
    }

    fn unit_exists(&mut self, unit: &str) -> bool {
        self.unit_file_state(unit).is_ok()
    }

    fn dump_layout(&mut self) -> Result<String> {
        let bytes: Vec<u8> = self.call(
            "dumpCurrentLayoutJS",
            "org.kde.plasmashell",
            "/PlasmaShell",
            "org.kde.PlasmaShell",
            "dumpCurrentLayoutJS",
            &(),
        )?;
        String::from_utf8(bytes).map_err(|e| SysError::DBus { call: "dumpCurrentLayoutJS", err: e.to_string() })
    }

    fn capture_layout(&mut self) -> Result<String> {
        let out = self.evaluate(&format!("{}\nkigae.capture();", crate::engine::HELPERS))?;
        crate::engine::kigae_state(&out)
            .map(str::to_string)
            .ok_or_else(|| SysError::Command { cmd: "kigae.capture".into(), msg: out.trim().into() })
    }

    fn wallpaper(&mut self) -> Result<Option<String>> {
        let out = self.evaluate(
            "var d = desktops()[0];\
             if (d.wallpaperPlugin == 'org.kde.image') {\
               d.currentConfigGroup = ['Wallpaper', 'org.kde.image', 'General'];\
               print(d.readConfig('Image'));\
             }",
        )?;
        let v = out.trim();
        Ok((!v.is_empty()).then(|| v.to_string()))
    }

    fn login_theme(&mut self) -> Option<String> {
        crate::deep::login_theme()
    }

    fn is_day(&mut self) -> Result<bool> {
        use std::collections::HashMap;
        use zbus::zvariant::{OwnedValue, Value};
        const CALL: &str = "NightTime";
        let opts: HashMap<&str, Value> = HashMap::new();
        let reply: HashMap<String, OwnedValue> = self.call(
            CALL,
            "org.kde.NightTime",
            "/org/kde/NightTime/Manager",
            "org.kde.NightTime.Manager",
            "Subscribe",
            &(opts,),
        )?;
        let bad = |what: &str| SysError::DBus { call: CALL, err: format!("unexpected reply: {what}") };
        if let Some(cookie) = reply.get("Cookie").and_then(|c| u32::try_from(c).ok()) {
            // best effort: the answer is already in hand
            let _: Result<()> = self.call(
                CALL,
                "org.kde.NightTime",
                "/org/kde/NightTime/Manager",
                "org.kde.NightTime.Manager",
                "Unsubscribe",
                &(cookie,),
            );
        }
        // Schedule = (kind, v) where v is a(xxxxx): per cycle noon, morning start/end,
        // evening start/end, in ms since the epoch
        let schedule = reply.get("Schedule").ok_or_else(|| bad("no Schedule"))?;
        let Value::Structure(st) = &**schedule else { return Err(bad("Schedule is not a struct")) };
        let inner = match st.fields().get(1) {
            Some(Value::Value(v)) => &**v,
            Some(v) => v,
            None => return Err(bad("Schedule has no cycles")),
        };
        let Value::Array(arr) = inner else { return Err(bad("cycles are not an array")) };
        let mut cycles = vec![];
        for item in arr.iter() {
            let Value::Structure(c) = item else { return Err(bad("cycle is not a struct")) };
            let x: Vec<i64> = c.fields().iter().filter_map(|f| if let Value::I64(x) = f { Some(*x) } else { None }).collect();
            let [a, b, c, d, e] = x[..] else { return Err(bad("cycle is not 5 times")) };
            cycles.push((a, b, c, d, e));
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or_default();
        Ok(crate::daynight::is_day(now, &cycles))
    }

    fn act(&mut self, action: &Action) -> Result<String> {
        if let Action::EvaluateScript { script, delay_ms, .. } = action {
            std::thread::sleep(std::time::Duration::from_millis(*delay_ms));
            let out = self.evaluate(script)?;
            // plasmashell reports script errors in the returned text, not as a D-Bus error
            if out.contains("Error:") || out.contains("TypeError") || out.contains("ReferenceError") {
                return Err(SysError::Command { cmd: "evaluateScript".into(), msg: out.trim().into() });
            }
            return Ok(out);
        }
        self.act_quiet(action).map(|()| String::new())
    }
}

impl RealSys {
    fn act_quiet(&mut self, action: &Action) -> Result<()> {
        match action {
            Action::Exec(argv) => {
                let argv: Vec<&str> = argv.iter().map(String::as_str).collect();
                run(&argv).map(drop)
            }
            Action::Shell(cmd) => run_shell(cmd, HOOK_TIMEOUT),
            Action::KWrite { file, groups, key, value, boolean } => {
                let mut argv = vec!["kwriteconfig6", "--notify", "--file", file.as_str()];
                argv.extend(group_args(groups));
                argv.extend(["--key", key.as_str()]);
                match value {
                    Some(v) => {
                        if *boolean {
                            argv.extend(["--type", "bool"]);
                        }
                        // `--` so a value like "-1" isn't read as an option
                        argv.extend(["--", v.as_str()]);
                    }
                    None => argv.push("--delete"),
                }
                run(&argv).map(drop)
            }
            // both are no-reply: send without waiting for an answer
            Action::KWinReconfigure => self.kwin_noreply("KWin reconfigure", "reconfigure", &()),
            Action::ShowDesktop(on) => self.kwin_noreply("KWin showDesktop", "showDesktop", &(*on,)),
            Action::EvaluateScript { .. } => unreachable!("handled in act"),
            Action::WriteFile { path, contents } => {
                if let Some(dir) = path.parent() {
                    fs::create_dir_all(dir).map_err(io_err(dir))?;
                }
                let tmp = path.with_extension("kigae-tmp");
                fs::write(&tmp, contents).map_err(io_err(&tmp))?;
                fs::rename(&tmp, path).map_err(io_err(path))
            }
            Action::Symlink { link, target } => {
                if let Some(dir) = link.parent() {
                    fs::create_dir_all(dir).map_err(io_err(dir))?;
                }
                // atomic swap: make the new link beside it, then rename over
                let tmp = link.with_extension("kigae-tmp");
                let _ = fs::remove_file(&tmp);
                std::os::unix::fs::symlink(target, &tmp).map_err(io_err(&tmp))?;
                fs::rename(&tmp, link).map_err(io_err(link))
            }
            Action::Remove(path) => match fs::remove_file(path) {
                Err(e) if e.kind() != io::ErrorKind::NotFound => Err(io_err(path)(e)),
                _ => Ok(()),
            },
            // Over D-Bus, without waiting for the daemon-reload `systemctl enable` does
            // (~700 ms): start/stop act right away, and the reload, sent last, catches systemd up
            // on the [Install] links in the background.
            Action::Unit { unit, enable, active } => {
                match enable {
                    Some(true) => {
                        let _: (bool, Vec<(String, String, String)>) =
                            self.systemd("EnableUnitFiles", &(vec![unit.as_str()], false, true))?;
                    }
                    Some(false) => {
                        let _: Vec<(String, String, String)> =
                            self.systemd("DisableUnitFiles", &(vec![unit.as_str()], false))?;
                    }
                    None => {}
                }
                if let Some(a) = active {
                    let method = if *a { "StartUnit" } else { "StopUnit" };
                    let _: zbus::zvariant::OwnedObjectPath = self.systemd(method, &(unit.as_str(), "replace"))?;
                }
                if enable.is_some() {
                    self.noreply("systemd Reload", "/org/freedesktop/systemd1", "org.freedesktop.systemd1", "org.freedesktop.systemd1.Manager", "Reload", &())?;
                }
                Ok(())
            }
            Action::ReloadKitty => {
                // exits 1 when no kitty is running, which is fine
                let _ = Command::new("pkill").args(["-USR1", "-x", "kitty"]).status();
                Ok(())
            }
        }
    }
}

// ───────────────────────────── fake ─────────────────────────────

/// An in-memory desktop for tests and `--dry-run`: reads come from maps, actions are recorded
/// and also applied to the maps, so later reads (and undo) see them.
#[derive(Debug, Default, Clone)]
pub struct FakeSys {
    /// `(file, "Group/Sub", key)` → value
    pub kconfig: BTreeMap<(String, String, String), String>,
    pub files: BTreeMap<PathBuf, FileState>,
    /// Paths that exist for `exists` (themes, wallpapers, …) besides `files`.
    pub present: Vec<PathBuf>,
    pub units: BTreeMap<String, UnitState>,
    pub layout: String,
    /// What `capture_layout` returns.
    pub layout_state: String,
    /// What the next scripts print, in order (then nothing).
    pub script_output: Vec<String>,
    /// Every script run, in order.
    pub scripts: Vec<String>,
    pub wallpaper: Option<String>,
    pub day: bool,
    /// The SDDM theme; `pkexec kigae-login <theme>` actions change it.
    pub login: Option<String>,
    pub actions: Vec<Action>,
    /// Make the action whose `Display` contains this fail.
    pub fail_on: Option<String>,
}

impl FakeSys {
    fn key(file: &str, groups: &[String], key: &str) -> (String, String, String) {
        (file.to_string(), groups.join("/"), key.to_string())
    }

    pub fn set_kconfig(&mut self, file: &str, groups: &str, key: &str, value: &str) {
        self.kconfig.insert((file.into(), groups.into(), key.into()), value.into());
    }

    pub fn get_kconfig(&self, file: &str, groups: &str, key: &str) -> Option<&str> {
        self.kconfig.get(&(file.into(), groups.into(), key.into())).map(String::as_str)
    }

    /// The actions as `--dry-run` prints them.
    pub fn log(&self) -> Vec<String> {
        self.actions.iter().map(ToString::to_string).collect()
    }
}

impl Sys for FakeSys {
    fn kread(&mut self, file: &str, groups: &[String], key: &str) -> Result<Option<String>> {
        Ok(self.kconfig.get(&Self::key(file, groups, key)).cloned())
    }

    fn file_state(&mut self, path: &Path) -> Result<FileState> {
        Ok(self.files.get(path).cloned().unwrap_or(FileState::Missing))
    }

    fn exists(&mut self, path: &Path) -> bool {
        self.present.iter().any(|p| p == path)
            || !matches!(self.files.get(path), None | Some(FileState::Missing))
    }

    fn unit_state(&mut self, unit: &str) -> Result<UnitState> {
        Ok(self.units.get(unit).copied().unwrap_or(UnitState { enabled: false, active: false }))
    }

    fn unit_exists(&mut self, unit: &str) -> bool {
        self.units.contains_key(unit)
    }

    fn dump_layout(&mut self) -> Result<String> {
        Ok(self.layout.clone())
    }

    fn capture_layout(&mut self) -> Result<String> {
        Ok(self.layout_state.clone())
    }

    fn wallpaper(&mut self) -> Result<Option<String>> {
        Ok(self.wallpaper.clone())
    }

    fn is_day(&mut self) -> Result<bool> {
        Ok(self.day)
    }

    fn login_theme(&mut self) -> Option<String> {
        self.login.clone()
    }

    fn act(&mut self, action: &Action) -> Result<String> {
        if let Some(pat) = &self.fail_on
            && action.to_string().contains(pat.as_str())
        {
            return Err(SysError::Command { cmd: action.to_string(), msg: "fake failure".into() });
        }
        self.actions.push(action.clone());
        match action {
            Action::KWrite { file, groups, key, value, .. } => {
                let k = Self::key(file, groups, key);
                match value {
                    Some(v) => self.kconfig.insert(k, v.clone()),
                    None => self.kconfig.remove(&k),
                };
            }
            Action::Exec(argv) => {
                let set = |s: &mut Self, f: &str, g: &str, k: &str| {
                    if let Some(v) = argv.get(1) {
                        s.set_kconfig(f, g, k, v);
                    }
                };
                match argv.first().map(String::as_str) {
                    Some("plasma-apply-colorscheme") => set(self, "kdeglobals", "General", "ColorScheme"),
                    Some("plasma-apply-desktoptheme") => set(self, "plasmarc", "Theme", "name"),
                    Some("/usr/libexec/plasma-changeicons") => set(self, "kdeglobals", "Icons", "Theme"),
                    Some("plasma-apply-cursortheme") => set(self, "kcminputrc", "Mouse", "cursorTheme"),
                    Some("plasma-apply-wallpaperimage") => {
                        self.wallpaper = argv.get(1).map(|p| format!("file://{p}"));
                    }
                    Some("pkexec") if argv.get(1).map(String::as_str) == Some(crate::deep::LOGIN_HELPER) => {
                        self.login = argv.get(2).cloned();
                    }
                    _ => {}
                }
            }
            Action::EvaluateScript { script, .. } => {
                self.scripts.push(script.clone());
                // a settle script only restores tray/desktop state, it doesn't rebuild panels
                if let Some(st) = script.split("kigae.restore(").nth(1) {
                    self.layout_state = st.trim_end().trim_end_matches(");").to_string();
                } else {
                    self.layout = script.clone();
                }
                return Ok(if self.script_output.is_empty() { String::new() } else { self.script_output.remove(0) });
            }
            Action::WriteFile { path, contents } => {
                self.files.insert(path.clone(), FileState::Content(contents.clone()));
            }
            Action::Symlink { link, target } => {
                self.files.insert(link.clone(), FileState::Link(target.clone()));
            }
            Action::Remove(p) => {
                self.files.remove(p);
            }
            Action::Unit { unit, enable, active } => {
                let s = self.units.entry(unit.clone()).or_insert(UnitState { enabled: false, active: false });
                if let Some(e) = enable {
                    s.enabled = *e;
                }
                if let Some(a) = active {
                    s.active = *a;
                }
            }
            Action::Shell(_) | Action::KWinReconfigure | Action::ShowDesktop(_) | Action::ReloadKitty => {}
        }
        Ok(String::new())
    }
}

// ───────────────────────────── dry run ─────────────────────────────

/// Reads from the live session, records actions instead of doing them (`--dry-run`).
pub struct DryRun<S: Sys> {
    pub inner: S,
    pub actions: Vec<Action>,
}

impl<S: Sys> DryRun<S> {
    pub fn new(inner: S) -> Self {
        Self { inner, actions: vec![] }
    }
}

impl<S: Sys> Sys for DryRun<S> {
    fn kread(&mut self, file: &str, groups: &[String], key: &str) -> Result<Option<String>> {
        self.inner.kread(file, groups, key)
    }
    fn file_state(&mut self, path: &Path) -> Result<FileState> {
        self.inner.file_state(path)
    }
    fn exists(&mut self, path: &Path) -> bool {
        self.inner.exists(path)
    }
    fn unit_state(&mut self, unit: &str) -> Result<UnitState> {
        self.inner.unit_state(unit)
    }
    fn unit_exists(&mut self, unit: &str) -> bool {
        self.inner.unit_exists(unit)
    }
    fn dump_layout(&mut self) -> Result<String> {
        self.inner.dump_layout()
    }
    fn capture_layout(&mut self) -> Result<String> {
        self.inner.capture_layout()
    }
    fn wallpaper(&mut self) -> Result<Option<String>> {
        self.inner.wallpaper()
    }
    fn is_day(&mut self) -> Result<bool> {
        self.inner.is_day()
    }
    fn login_theme(&mut self) -> Option<String> {
        self.inner.login_theme()
    }
    fn act(&mut self, action: &Action) -> Result<String> {
        self.actions.push(action.clone());
        Ok(String::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn hooks_report_failures_and_time_out() {
        let t = Duration::from_secs(5);
        assert!(run_shell("true", t).is_ok());
        let err = run_shell("echo nope >&2; exit 3", t).unwrap_err().to_string();
        assert!(err.contains("nope"), "{err}");
        let start = Instant::now();
        let err = run_shell("sleep 5", Duration::from_millis(100)).unwrap_err().to_string();
        assert!(err.contains("timed out"), "{err}");
        assert!(start.elapsed() < Duration::from_secs(3));
    }

    #[test]
    fn a_backgrounded_process_does_not_hold_the_hook() {
        let start = Instant::now();
        run_shell("sleep 3 &", Duration::from_secs(10)).unwrap();
        assert!(start.elapsed() < Duration::from_secs(2));
    }
}

// ───────────────────────────── timing ─────────────────────────────

/// Passes everything through and prints how long each read and action took (`KIGAE_TRACE=1`).
pub struct Traced<S: Sys> {
    pub inner: S,
    start: std::time::Instant,
}

impl<S: Sys> Traced<S> {
    pub fn new(inner: S) -> Self {
        Self { inner, start: std::time::Instant::now() }
    }

    fn time<T>(&mut self, what: impl FnOnce() -> String, f: impl FnOnce(&mut S) -> T) -> T {
        let t = std::time::Instant::now();
        let out = f(&mut self.inner);
        let ms = t.elapsed().as_secs_f64() * 1000.0;
        let at = self.start.elapsed().as_secs_f64() * 1000.0;
        eprintln!("{at:8.1} ms  {ms:7.1} ms  {}", what());
        out
    }
}

impl<S: Sys> Sys for Traced<S> {
    fn kread(&mut self, file: &str, groups: &[String], key: &str) -> Result<Option<String>> {
        self.time(|| format!("read {file} [{}] {key}", groups.join("][")), |s| s.kread(file, groups, key))
    }
    fn file_state(&mut self, path: &Path) -> Result<FileState> {
        self.time(|| format!("stat {}", path.display()), |s| s.file_state(path))
    }
    fn exists(&mut self, path: &Path) -> bool {
        self.time(|| format!("exists {}", path.display()), |s| s.exists(path))
    }
    fn unit_state(&mut self, unit: &str) -> Result<UnitState> {
        self.time(|| format!("unit state {unit}"), |s| s.unit_state(unit))
    }
    fn unit_exists(&mut self, unit: &str) -> bool {
        self.time(|| format!("unit exists {unit}"), |s| s.unit_exists(unit))
    }
    fn dump_layout(&mut self) -> Result<String> {
        self.time(|| "dump layout".into(), |s| s.dump_layout())
    }
    fn capture_layout(&mut self) -> Result<String> {
        self.time(|| "capture layout".into(), |s| s.capture_layout())
    }
    fn wallpaper(&mut self) -> Result<Option<String>> {
        self.time(|| "read wallpaper".into(), |s| s.wallpaper())
    }
    fn is_day(&mut self) -> Result<bool> {
        self.time(|| "is day".into(), |s| s.is_day())
    }
    fn login_theme(&mut self) -> Option<String> {
        self.time(|| "login theme".into(), |s| s.login_theme())
    }
    fn act(&mut self, action: &Action) -> Result<String> {
        self.time(|| action.to_string(), |s| s.act(action))
    }
}
