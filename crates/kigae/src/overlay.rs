//! `kigae daemon`: the D-Bus service and the QML overlay, the "fitting room" (SPEC.md §7–8).
//!
//! The bridge is thin on purpose: QML gets the whole state as one JSON string and calls back
//! with `apply` / `undo`. Slow work (git, doctor, the switch itself) runs on a thread and is
//! queued back onto the Qt thread.

use std::pin::Pin;
use std::sync::Mutex;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use cxx_qt::{CxxQtThread, Threading};
use cxx_qt_lib::{QGuiApplication, QQmlApplicationEngine, QString, QUrl};
use kigae_core::card;
use kigae_core::engine;
use kigae_core::journal::{Journal, Status};
use kigae_core::manifest::MoodKind;
use kigae_core::sys::{RealSys, Sys};
use serde_json::json;

use crate::{Res, ctx};

const BUS: &str = "dev.kigae.Kigae";
const PATH: &str = "/dev/kigae/Kigae";

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
    }

    #[auto_cxx_name]
    extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qproperty(QString, state)]
        #[qproperty(bool, busy)]
        type Kigae = super::KigaeRust;

        /// Called once from QML: from now on D-Bus `Show()` reaches this object.
        #[qinvokable]
        fn attach(self: Pin<&mut Kigae>);
        /// Re-read rices, the current one and history in the background.
        #[qinvokable]
        fn reload(self: Pin<&mut Kigae>);
        /// `mood` is "light", "dark" or "" (NightTime decides).
        #[qinvokable]
        fn apply(self: Pin<&mut Kigae>, id: &QString, mood: &QString);
        #[qinvokable]
        fn undo(self: Pin<&mut Kigae>);

        #[qsignal]
        fn show_requested(self: Pin<&mut Kigae>);
        #[qsignal]
        fn finished(self: Pin<&mut Kigae>, ok: bool, message: QString);
    }

    impl cxx_qt::Threading for Kigae {}
}

#[derive(Default)]
pub struct KigaeRust {
    state: QString,
    busy: bool,
}

static QT: Mutex<Option<CxxQtThread<qobject::Kigae>>> = Mutex::new(None);
/// A `Show()` that arrived before QML attached (D-Bus activation starts us on the hotkey).
static PENDING: AtomicBool = AtomicBool::new(false);
/// Tickets for state reads, handed out as each read starts; a reply older than the newest one
/// already shown is dropped, so a slow `reload` can't overwrite what a switch just produced.
static TICKET: AtomicU64 = AtomicU64::new(0);
static SHOWN: AtomicU64 = AtomicU64::new(0);

/// On the Qt thread: show `s` unless a read that started later has already been shown.
fn fresh(ticket: u64) -> bool {
    SHOWN.fetch_max(ticket, Ordering::SeqCst) <= ticket
}

fn request_show() {
    let qt = QT.lock().expect("lock");
    match qt.as_ref() {
        Some(t) => {
            let _ = t.queue(|k| k.show_requested());
        }
        None => PENDING.store(true, Ordering::SeqCst),
    }
}

impl qobject::Kigae {
    fn attach(mut self: Pin<&mut Self>) {
        let mut qt = QT.lock().expect("lock");
        *qt = Some(self.qt_thread());
        drop(qt);
        self.as_mut().reload();
        if PENDING.swap(false, Ordering::SeqCst) {
            self.show_requested();
        }
    }

    fn reload(self: Pin<&mut Self>) {
        let qt = self.qt_thread();
        std::thread::spawn(move || {
            let ticket = TICKET.fetch_add(1, Ordering::SeqCst) + 1;
            let s = catch_unwind(state)
                .unwrap_or_else(|_| Err("reading the rices panicked".into()))
                .unwrap_or_else(|e| json!({ "error": e }).to_string());
            let _ = qt.queue(move |k| {
                let s = QString::from(&s);
                if fresh(ticket) && *k.state() != s {
                    k.set_state(s);
                }
            });
        });
    }

    fn apply(self: Pin<&mut Self>, id: &QString, mood: &QString) {
        let id = String::from(id);
        let mood = match String::from(mood).as_str() {
            "light" => Some(MoodKind::Light),
            "dark" => Some(MoodKind::Dark),
            _ => None,
        };
        self.run(move || {
            let out = crate::switch(&id, mood)?;
            Ok(out.warnings.join("\n"))
        });
    }

    fn undo(self: Pin<&mut Self>) {
        self.run(|| {
            let paths = engine::Paths::from_env().ok_or("HOME is not set")?;
            let journals = Journal::list(&paths.undo_dir()).map_err(|e| e.to_string())?;
            let mut j = journals
                .into_iter()
                .rev()
                .find(|j| matches!(j.status, Status::Done | Status::InProgress) && !j.entries.is_empty())
                .ok_or("nothing to undo")?;
            let _lock = crate::lock()?;
            let errors = engine::undo(&mut RealSys::new(), &paths, &mut j).map_err(|e| e.to_string())?;
            if errors.is_empty() { Ok(String::new()) } else { Err(errors.join("\n")) }
        });
    }

    /// Run a switch on a thread; `busy` while it runs, then `finished` and a reload.
    fn run(mut self: Pin<&mut Self>, work: impl FnOnce() -> Res<String> + Send + 'static) {
        if *self.busy() {
            return;
        }
        self.as_mut().set_busy(true);
        let qt = self.qt_thread();
        std::thread::spawn(move || {
            // a panic must not leave `busy` set: Enter and undo would silently do nothing
            let (ok, msg) = match catch_unwind(AssertUnwindSafe(work)) {
                Ok(Ok(w)) => (true, w),
                Ok(Err(e)) => (false, e),
                Err(_) => (false, "the switch panicked (see the daemon's log)".to_string()),
            };
            let ticket = TICKET.fetch_add(1, Ordering::SeqCst) + 1;
            let s = catch_unwind(state).ok().and_then(Result::ok).unwrap_or_default();
            let _ = qt.queue(move |mut k| {
                if !s.is_empty() && fresh(ticket) {
                    k.as_mut().set_state(QString::from(&s));
                }
                k.as_mut().set_busy(false);
                k.finished(ok, QString::from(&msg));
            });
        });
    }
}

/// Everything the overlay shows, as JSON: cards, the rice on screen, today's mood, history.
fn state() -> Res<String> {
    let c = ctx()?;
    let mut sys = RealSys::new();
    let cache = std::env::var_os("XDG_CACHE_HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| c.paths.home.join(".cache"))
        .join("kigae");
    let current = c.current(&mut sys).map(|(r, _)| r.id().to_string());
    let cards: Vec<_> = c
        .rices
        .iter()
        .map(|r| {
            let m = c.effective(&r.manifest);
            let mut problems = engine::doctor(&mut sys, &c.paths, &m);
            if let Err(e) = r.layout_js() {
                problems.push(format!("layout script: {e}"));
            }
            let mut v = serde_json::to_value(card::card(r, &m, &c.paths, &cache)).expect("card serialises");
            v["problems"] = json!(problems);
            v
        })
        .collect();
    let day = sys.is_day().unwrap_or(true);
    // Plasma's animation speed slider; 0 means "reduce motion"
    let anim: f64 = sys
        .kread("kdeglobals", &["KDE".into()], "AnimationDurationFactor")
        .ok()
        .flatten()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1.0);
    let names: std::collections::HashMap<&str, &str> =
        c.rices.iter().map(|r| (r.id(), r.manifest.name.as_str())).collect();
    let journals = Journal::list(&c.paths.undo_dir()).unwrap_or_default();
    // the look the active rice wears, when the last switch says so for sure
    let worn = journals
        .last()
        .filter(|j| j.status == Status::Done && Some(&j.to) == current.as_ref())
        .and_then(|j| j.mood.clone());
    let history: Vec<_> = journals
        .iter()
        .rev()
        .filter(|j| !j.entries.is_empty())
        .take(8)
        .map(|j| {
            let name = |id: &str| names.get(id).copied().unwrap_or(id).to_string();
            json!({
                "ts": j.when_secs(),
                "from": j.from.as_deref().map(name),
                "to": name(&j.to),
                "mood": j.mood,
                "status": format!("{:?}", j.status),
            })
        })
        .collect();
    Ok(json!({ "cards": cards, "current": current, "worn": worn, "mood": if day { "light" } else { "dark" }, "anim": anim, "history": history })
        .to_string())
}

struct Service;

#[zbus::interface(name = "dev.kigae.Kigae")]
impl Service {
    /// Open the fitting room, or move to the next rice if it's already open.
    fn show(&self) {
        request_show();
    }
}

/// `kigae daemon`: own the bus name, then run Qt until killed.
pub fn run() -> Res {
    let _conn = zbus::blocking::connection::Builder::session()
        .and_then(|b| b.name(BUS))
        .and_then(|b| b.serve_at(PATH, Service))
        .and_then(|b| b.build())
        .map_err(|e| format!("D-Bus {BUS}: {e} (is a daemon already running?)"))?;
    std::thread::spawn(follow_sun);
    cxx_qt::init_qml_module!("dev.kigae");
    let mut app = QGuiApplication::new();
    let mut engine = QQmlApplicationEngine::new();
    if let Some(engine) = engine.as_mut() {
        engine.load(&QUrl::from("qrc:/qt/qml/dev/kigae/qml/Overlay.qml"));
    }
    // Overlay.qml calls attach() when it's built; without it there's no window to show
    if QT.lock().expect("lock").is_none() {
        return Err("Overlay.qml failed to load (Qt's error is above)".into());
    }
    if let Some(app) = app.as_mut() {
        app.exec();
    }
    Ok(())
}

/// Watch NightTime; at sunrise and sunset re-apply the active rice's day or night look. A look
/// picked by hand holds until then.
fn follow_sun() {
    let mut sys = RealSys::new();
    let mut last = sys.is_day().ok();
    loop {
        std::thread::sleep(std::time::Duration::from_secs(60));
        let Ok(day) = sys.is_day() else { continue };
        match last {
            None => {
                // the first read after a failed start only records where the sun is
                last = Some(day);
                continue;
            }
            Some(l) if l == day => continue,
            Some(_) => {}
        }
        // `last` moves only once the switch went through: a busy lock retries next minute
        match crate::follow_sun() {
            Ok(Some(done)) => eprintln!("kigae: {}: {done}", if day { "sunrise" } else { "sunset" }),
            Ok(None) => {}
            Err(e) => {
                eprintln!("kigae: following the sun: {e}");
                continue;
            }
        }
        last = Some(day);
        if let Some(t) = QT.lock().expect("lock").as_ref() {
            let _ = t.queue(|k| k.reload());
        }
    }
}

/// `kigae overlay`: ask the daemon to show itself (D-Bus activation starts it if needed).
pub fn show() -> Res {
    let conn = zbus::blocking::Connection::session().map_err(|e| e.to_string())?;
    conn.call_method(Some(BUS), PATH, Some(BUS), "Show", &())
        .map_err(|e| format!("{BUS}.Show: {e} (run `kigae setup` once, or start `kigae daemon` by hand)"))?;
    Ok(())
}
