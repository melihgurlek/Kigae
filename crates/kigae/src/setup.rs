//! `kigae setup`: install the systemd user unit, D-Bus activation and the Meta+Shift+T shortcut.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::Res;

const UNIT: &str = include_str!("../../../data/kigae.service");
const DBUS: &str = include_str!("../../../data/dev.kigae.Kigae.service");
const DESKTOP: &str = include_str!("../../../data/kigae-switch.desktop");

const COMPONENT: &str = "kigae-switch.desktop";
const ACTION: &str = "_launch";
/// Qt::META | Qt::SHIFT | Qt::Key_T, as kglobalaccel stores a key.
const META_SHIFT_T: i32 = 0x1000_0000 | 0x0200_0000 | 0x54;

struct Files {
    unit: PathBuf,
    dbus: PathBuf,
    desktop: PathBuf,
}

fn files() -> Res<Files> {
    let home = PathBuf::from(std::env::var_os("HOME").ok_or("HOME is not set")?);
    let xdg = |var, default: &str| std::env::var_os(var).map(PathBuf::from).unwrap_or_else(|| home.join(default));
    let config = xdg("XDG_CONFIG_HOME", ".config");
    let data = xdg("XDG_DATA_HOME", ".local/share");
    Ok(Files {
        unit: config.join("systemd/user/kigae.service"),
        dbus: data.join("dbus-1/services/dev.kigae.Kigae.service"),
        desktop: data.join("applications").join(COMPONENT),
    })
}

fn write(path: &Path, template: &str, exe: &str) -> Res {
    std::fs::create_dir_all(path.parent().expect("has parent")).map_err(|e| e.to_string())?;
    std::fs::write(path, template.replace("@EXE@", exe)).map_err(|e| format!("{}: {e}", path.display()))?;
    println!("  wrote {}", path.display());
    Ok(())
}

fn systemctl(args: &[&str]) -> Res {
    let out = Command::new("systemctl").arg("--user").args(args).output().map_err(|e| format!("systemctl: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!("systemctl --user {}: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim()))
    }
}

fn kglobalaccel() -> Res<zbus::blocking::Proxy<'static>> {
    let conn = zbus::blocking::Connection::session().map_err(|e| e.to_string())?;
    zbus::blocking::Proxy::new(&conn, "org.kde.kglobalaccel", "/kglobalaccel", "org.kde.KGlobalAccel")
        .map_err(|e| format!("kglobalaccel: {e}"))
}

/// KGlobalShortcutInfo: action id, action name, component id, component name, context id,
/// context name, keys, default keys.
type ShortcutInfo = (String, String, String, String, String, String, Vec<i32>, Vec<i32>);

/// Bind Meta+Shift+T to the desktop file, unless something else holds it. kglobalaccel usually
/// picks up the desktop file's `X-KDE-Shortcuts` by itself; registering covers when it doesn't.
fn bind_shortcut() -> Res {
    let kga = kglobalaccel()?;
    let holders: Vec<ShortcutInfo> =
        kga.call("getGlobalShortcutsByKey", &(META_SHIFT_T,)).map_err(|e| format!("kglobalaccel: {e}"))?;
    if let Some(h) = holders.iter().find(|h| h.2 != COMPONENT) {
        return Err(format!(
            "Meta+Shift+T already belongs to \"{}\" ({}); free it in System Settings → Shortcuts and run setup again",
            h.1, h.3
        ));
    }
    if !holders.is_empty() {
        println!("  Meta+Shift+T is bound");
        return Ok(());
    }
    let id: &[&str] = &[COMPONENT, ACTION, "Kigae", "Open the fitting room"];
    kga.call::<_, _, ()>("doRegister", &(id,)).map_err(|e| format!("kglobalaccel: {e}"))?;
    kga.call::<_, _, ()>("setForeignShortcut", &(id, vec![META_SHIFT_T])).map_err(|e| format!("kglobalaccel: {e}"))?;
    println!("  bound Meta+Shift+T");
    Ok(())
}

pub fn install() -> Res {
    let exe = std::env::current_exe().and_then(|p| p.canonicalize()).map_err(|e| format!("can't find my own binary: {e}"))?;
    if exe.components().any(|c| c.as_os_str() == "target") {
        eprintln!(
            "kigae: warning: {} is a build in target/; `cargo install --path crates/kigae` and run setup from there, \
             or the service breaks on the next `cargo clean`",
            exe.display()
        );
    }
    let exe = exe.to_str().ok_or("binary path isn't UTF-8")?;
    let f = files()?;
    write(&f.unit, UNIT, exe)?;
    write(&f.dbus, DBUS, exe)?;
    write(&f.desktop, DESKTOP, exe)?;
    systemctl(&["daemon-reload"])?;
    systemctl(&["enable", "kigae.service"])?;
    // restart, not start: a re-run after `cargo install` should pick up the new binary
    systemctl(&["restart", "kigae.service"]).map_err(|e| format!("{e}\n(is a `kigae daemon` running by hand? stop it and run setup again)"))?;
    println!("  kigae.service enabled and running (logs: journalctl --user -t kigae)");
    bind_shortcut()
}

pub fn remove() -> Res {
    let f = files()?;
    let _ = systemctl(&["disable", "--now", "kigae.service"]);
    if let Ok(kga) = kglobalaccel() {
        let _ = kga.call::<_, _, bool>("unregister", &(COMPONENT, ACTION));
    }
    for p in [&f.unit, &f.dbus, &f.desktop] {
        match std::fs::remove_file(p) {
            Ok(()) => println!("  removed {}", p.display()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(format!("{}: {e}", p.display())),
        }
    }
    systemctl(&["daemon-reload"])?;
    println!("  service stopped, Meta+Shift+T released");
    Ok(())
}
