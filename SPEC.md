# Kigae design spec

This spec turns `DESIGN.md` (the idea) into something to build. Where the two disagree, this file wins. The build order is in `PLAN.md`.

Status: spec, nothing built yet (2026-11-01). Target: Plasma 6.7 on Wayland, Fedora 44.

## 0. Language

- **The engine, CLI and daemon are Rust.** Switching the desktop is a transaction on live state. Serde with `deny_unknown_fields` catches manifest mistakes before anything is touched. `Result` forces every failed step to be handled, and the undo journal depends on that. `zbus` talks to KWin, PlasmaShell, systemd and NightTime natively. The result is one binary that doesn't break when Fedora upgrades Python or PySide.
- **The overlay is QML, hosted inside the daemon through cxx-qt.** QML is what gets you Plasma's layer-shell (`org.kde.layershell`), KWin blur and the native look. The bridge between Rust and QML stays thin: one list model, a few invokable methods, a few signals. It sits behind the Cargo feature `overlay`, so the engine builds and tests without Qt.
- Not chosen: PySide6 (DESIGN.md's first pick) works, but it's loosely typed and depends on Fedora's Python/PySide upgrades. Slint has no KWin blur or layer-shell integration. GTK4 looks foreign on Plasma.
- `qdbus` doesn't exist on Fedora (it's named `qdbus-qt6`). Kigae calls D-Bus through `zbus` and never shells out to it.

## 1. Architecture

```
kigae (one binary)
├─ CLI ─────────────┐
├─ daemon (zbus) ───┼──> kigae-core: discover → plan → journal → run parts
└─ overlay (QML) ───┘          │
                               ├─ shell-out: plasma-apply-*, plasma-changeicons, kwriteconfig6, git
                               └─ D-Bus: KWin.reconfigure, PlasmaShell.evaluateScript,
                                         systemd1.Manager, org.kde.NightTime
```

- Crates: `crates/kigae-core` (library, no Qt) and `crates/kigae` (the binary, with features `daemon` and `overlay`).
- When the daemon is running, the CLI sends `apply` through it so that the overlay and the CLI never apply at the same moment. If the daemon isn't running, the CLI does the work itself and holds a lock file (`$XDG_RUNTIME_DIR/kigae.lock`).
- Everything runs in sequence. A switch is a short list of steps, and the bulk of it is about 15 process spawns.
- All side effects go through a `Runner` trait: the real one runs commands and D-Bus calls, and a recording one powers `--dry-run` and the tests.

## 2. Where rices come from

- `~/.config/kigae/config.toml`:
  ```toml
  repo = "~/Code/dotfiles"
  branches = ["rice/*", "backup/og"]
  manifests = "~/Code/Kigae/manifests"
  ```
- Lookup order for each rice: `git show <branch>:rice.toml`, then `<manifests>/<id>.toml`. Local manifests are for branches that must stay untouched: `backup/og` is a restore point, so og's manifest lives here as `manifests/og.toml` with `branch = "backup/og"`.
- The rice id is the last part of the branch name (`rice/ukiyo` → `ukiyo`), with `og` for `backup/og`.
- Previews are read with `git show <branch>:<preview>` and cached at `~/.cache/kigae/previews/<id>-<blob-sha>.jpg`. Nothing is ever checked out, so the repo can stay on any branch.

## 3. Manifest (`rice.toml`)

`schema = 1`. Unknown keys are rejected. Every section is optional, and a missing part means "leave it as it is". The one exception: the previous rice's services are always stopped.

```toml
schema = 1
id = "ukiyo"
name = "Ukiyo"
kanji = "浮世"
tagline = "ink & paper, floating islands"
branch = "rice/ukiyo"            # only in local manifests
preview = "ukiyo/screens/desktop.jpg"

[plasma]
colors = "Ukiyo"
style = "ukiyo"
icons = "Colloid-Yellow-Everforest-Dark"
cursor = "breeze_cursors"
decoration = { library = "org.kde.breeze", theme = "Breeze", borders = "None" }
layout = "ukiyo/kigae-layout.js"
wallpaper = "~/Pictures/Wallpapers/ukiyo/day.jpg"   # omit when a service owns the wallpaper
splash = "org.ukiyo.desktop"

[kwin]                            # "Group/Key" = value, written to kwinrc
"Plugins/blurEnabled" = true

[daynight]                        # optional: Kigae switches these at NightTime transitions
light = { colors = "Itchy", style = "Itchy" }
dark  = { colors = "Scratchy", style = "Scratchy" }

[apps]
kitty = "~/.config/kitty/themes/ukiyo.conf"
p10k = "~/.config/ukiyo/p10k.zsh"
vscode = { theme = "Kanagawa", customizations = "ukiyo/vscode.json" }
konsole = "Ukiyo"
kate = "Ukiyo"

[services]
enable = ["ukiyo-sky.timer"]

[install]                         # branch path → $HOME path, copied once by `kigae install`
"home/.local/share/plasma/desktoptheme/ukiyo" = ".local/share/plasma/desktoptheme/ukiyo"

[deep]                            # root only, `kigae deep`
sddm = "ukiyo"
grub = "ukiyo/grub/install-grub-theme.sh"
```

Paths starting with `~` are in `$HOME`. Other relative paths (`layout`, `preview`, `vscode.customizations`) are inside the rice's branch.

## 4. Apply pipeline

1. **Resolve**: load the target manifest and the current one (from `~/.local/state/kigae/current`).
2. **Preflight**, the same check as `kigae doctor`: every named colour scheme, Plasma style, icon set, cursor, plasmoid and file has to exist where it's installed. If anything is missing, stop before touching the desktop and suggest `kigae install <id>`.
3. **Snapshot**: each part reads the keys it is about to change. They're written to `~/.local/state/kigae/undo/<unix-ts>-<from>-<to>.json`, together with the full panel layout dumped through `evaluateScript` (`panels()` → JSON).
4. **Run** the parts in this order:

   | # | Part | How | Critical |
   |---|---|---|---|
   | 1 | services-stop | systemd `StopUnit` + `DisableUnitFiles` for the old rice's units | yes |
   | 2 | kwin | `kwriteconfig6 --file kwinrc`, then `org.kde.KWin /KWin reconfigure` | yes |
   | 3 | colors | `plasma-apply-colorscheme` | yes |
   | 4 | style | `plasma-apply-desktoptheme` | yes |
   | 5 | icons | `/usr/libexec/plasma-changeicons` | yes |
   | 6 | cursor | `plasma-apply-cursortheme` | yes |
   | 7 | layout | prelude + rice script through `org.kde.PlasmaShell.evaluateScript` | yes |
   | 8 | wallpaper | `plasma-apply-wallpaperimage` | no |
   | 9 | apps | see below | no |
   | 10 | splash | `kwriteconfig6 --file ksplashrc` | no |
   | 11 | services-start | `EnableUnitFiles` + `StartUnit` | no |
   | 12 | state | write `current` | — |

5. **Failure policy**: if a critical part fails, Kigae rolls back the parts it already applied, using the snapshot, and returns an error. A non-critical part that fails is logged, and the switch counts as done with warnings.
6. Every part implements `snapshot`, `apply` and `describe`. `--dry-run` prints the commands from `describe`.

**Apps**
- **kitty**: atomically switch the symlink `~/.config/kitty/current-rice.conf`, then send `SIGUSR1` to every `kitty` process. The base `kitty.conf` has `include current-rice.conf`.
- **p10k**: symlink `~/.p10k.zsh` to the rice's copy. New shells pick it up, open ones on `exec zsh`.
- **vscode**: change `workbench.colorTheme` (and `workbench.colorCustomizations`) in `~/.config/Code/User/settings.json`. The file is JSONC, so the edit has to keep comments and formatting.
- **nvim**: reads the rice id from `~/.local/state/kigae/current` into `vim.g.rice` at startup.
- **konsole/kate**: `kwriteconfig6` for the default profile/theme. Only new windows pick it up.

## 5. Layout contract

Kigae runs `data/prelude.js` and then the rice's layout file, as one `evaluateScript` call.

- The prelude reads the launcher list from every `org.kde.plasma.icontasks` widget into `kigae.launchers`. Then it removes **every** panel and every desktop widget, on all containments.
- The rice script builds its panels from nothing and uses `kigae.launchers` for the dock.
- Panel heights are set in the script, never inherited (a Plasma style changes panel padding). The script must run cleanly from any starting state.
- Kigae never edits `plasma-org.kde.plasma.desktop-appletsrc` while plasmashell is running, because that's the file plasmashell writes back on exit.

## 6. Day/night

- If the active rice has `[daynight]`, the daemon subscribes to `org.kde.NightTime`'s schedule. At each transition it re-applies only the `light` or `dark` parts: no layout, no services.
- When the daemon isn't running, `kigae apply` picks the right half for the current time from the same schedule.
- `AutomaticLookAndFeel` is forced to `false`, and that change is part of the first snapshot. Plasma's own look-and-feel switch resets too much.

## 7. Overlay

- **Window**: a full-screen layer-shell surface (`org.kde.layershell`, layer `Overlay`, keyboard `Exclusive`). It's translucent with a dim layer and KWin blur behind it (a small C++ shim around `KWindowEffects::enableBlurBehind`). Cards sit in a `ListView` in the bottom third, each showing preview, name, kanji and tagline. The current rice is marked.
- **Shortcut**: `kigae-switch.desktop` with `X-KDE-Shortcuts=Meta+Shift+T` and `Exec=kigae overlay`. That sends `Show()` to the daemon.
- **Opening**: `Show()` opens the overlay with the *next* rice selected. A `Show()` while it's already open means `Next`, because KWin may take the T taps as shortcut presses instead of key events. Duplicates are dropped.
- **Keys**: T or →/Tab moves to the next card, Shift+T or ←/Shift+Tab moves back, Esc cancels, Enter or a click applies.
- **Applying on release**: if the keyboard-enter event shows Meta still held, letting go of Meta applies. If Meta was already released when the window got focus (a quick tap), the overlay stays open in **browse mode** and Enter or a click applies. A quick tap never switches the whole desktop by accident.
- **Apply animation**: fade to the target's preview at full opacity (200 ms), apply in the daemon, wait for the result plus 400 ms for things to settle, then fade out (300 ms). If the switch fails, show an error card with "Undo".
- The window is created on first use and hidden afterwards, never destroyed, so it opens instantly.

## 8. D-Bus API

Bus name `dev.kigae.Kigae`, object `/dev/kigae/Kigae`, interface `dev.kigae.Kigae`.

| Member | Signature |
|---|---|
| `List()` | `→ a(ssss)` (id, name, kanji, preview path) |
| `Current()` | `→ s` |
| `Apply(s id)` | `→ (b ok, as warnings)` |
| `Undo()` | `→ b` |
| `Show()` | `→ ()` |
| signal `Switched` | `(s from, s to)` |

The daemon runs as the systemd user unit `kigae.service`.

## 9. CLI

```
kigae list                       rices found, current one marked
kigae current
kigae apply <id> [--dry-run]
kigae undo [--list | <ts>]
kigae install <id> [--dry-run]   copy [install] assets into $HOME once
kigae uninstall <id>             remove exactly what install copied
kigae doctor [<id>...]           preflight check without applying
kigae shoot [<id>...]            apply each rice in turn and take its preview screenshot
kigae deep <id>                  SDDM + GRUB, one pkexec prompt
kigae daemon
kigae overlay                    ask the daemon to show the overlay
```

## 10. State and files

| Path | What |
|---|---|
| `~/.config/kigae/config.toml` | repo, branches, local manifests |
| `~/.local/state/kigae/current` | active rice id (Neovim reads this) |
| `~/.local/state/kigae/undo/*.json` | one journal per switch |
| `~/.local/state/kigae/installed/<id>.json` | every file `install` copied |
| `~/.local/state/kigae/kigae.log` | apply log |
| `~/.cache/kigae/previews/` | preview images taken from git |

## 11. Safety rules (non-negotiable)

- Never run `plasma-apply-lookandfeel`.
- Never edit appletsrc while plasmashell is running. Panels change only through the scripting API.
- Never copy over shared config files (`kdeglobals`, `kwinrc`, `plasmarc`, and so on). Those are changed key by key, and every key is journaled first.
- Every switch is journaled before it starts. `kigae undo` puts the previous state back exactly.
- Anything that needs root is limited to `kigae deep` and never happens on the hotkey.

## 12. Known limits

- Konsole, Kate, Obsidian and GTK2 apps that are already running keep their old look until reopened.
- The splash screen and SDDM only show at the next login.
