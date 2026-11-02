# Kigae design spec

This spec turns `DESIGN.md` (the idea) into something to build. Where the two disagree, this file wins. The build order is in `PLAN.md`.

Status: phase 1 (engine + CLI) built and switching Ukiyo ⇄ og live (2026-11-01). Target: Plasma 6.7 on Wayland, Fedora 44.

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
                               ├─ shell-out: plasma-apply-*, plasma-changeicons, k*config6, systemctl, git
                               └─ D-Bus: KWin.reconfigure, PlasmaShell.evaluateScript/dumpCurrentLayoutJS,
                                         org.kde.NightTime
```

- Crates: `crates/kigae-core` (library, no Qt) and `crates/kigae` (the binary, with features `daemon` and `overlay`).
- When the daemon is running, the CLI sends `apply` through it so that the overlay and the CLI never apply at the same moment. If the daemon isn't running, the CLI does the work itself and holds a lock file (`$XDG_RUNTIME_DIR/kigae.lock`).
- Everything runs in sequence. A switch is a short list of steps, and the bulk of it is about 15 process spawns.
- All reads and side effects go through the `Sys` trait: `RealSys` talks to the session, `DryRun` reads live but records actions, `FakeSys` is an in-memory desktop for the tests.

## 2. Where rices come from

- `~/.config/kigae/config.toml` (all keys optional; these are the defaults):
  ```toml
  repo = "~/dotfiles"      # one branch per rice
  branches = ["rice/*"]    # a trailing * matches any rest
  # manifests = "~/.config/kigae/manifests"
  ```
- Lookup order for each rice: `git show <branch>:rice.toml`, then the manifests folder (for rices whose branch you'd rather not touch, or that have no branch). A local manifest names its branch with `branch = "…"`, and its `./…` paths resolve next to it.
- Previews are read with `git show <branch>:<preview>` and cached at `~/.cache/kigae/previews/<id>-<blob-sha>.jpg`. Nothing is ever checked out, so the repo can stay on any branch.
- `~/.config/kigae` can itself be a branch of the dotfiles repo, checked out as a git worktree. Then `config.toml`, `base.toml` and the manifests are versioned like any other dotfile and readable whatever rice branch is checked out (vortex does this: branch `kigae`, with og's manifest, since `backup/og` stays untouched).

## 3. Manifest (`rice.toml`)

`schema = 1`. Unknown keys are rejected. Every section is optional, and a missing plasma part means "leave it as it is".

**Ownership.** Keys a rice sets in `[kconfig]`, `[links]` and `[vscode]` (in either mood) are *owned* by it. When you switch away, what happens to owned keys the next rice doesn't set depends on `[unset]` (§3.1). The previous rice's services are always stopped.

```toml
schema = 1
id = "ukiyo"
name = "Ukiyo"
kanji = "浮世"
tagline = "ink & paper, floating islands"
branch = "rice/ukiyo"            # only in local manifests
preview = "ukiyo/screens/desktop.jpg"

[plasma]                          # applied with Plasma's own tools
colors = "Ukiyo"
style = "ukiyo"
icons = "Colloid-Yellow-Everforest-Dark"
cursor = "breeze_cursors"
layout = "ukiyo/kigae-layout.js"
wallpaper = "~/Pictures/Wallpapers/ukiyo/night-wave.jpg"

[kconfig.kwinrc]                  # any KConfig file: "Group/Key" (nested: "A/B/Key") = value
"Plugins/fadeEnabled" = true
"org.kde.kdecoration2/library" = "org.kde.breeze"
[kconfig.konsolerc]
"Desktop Entry/DefaultProfile" = "Ukiyo.profile"
[kconfig.ksplashrc]
"KSplash/Theme" = "org.ukiyo.desktop"

[links]                           # Kigae-owned symlink → the rice's file (both under ~)
"~/.config/kitty/kigae-current.conf" = "~/.config/kitty/ukiyo.conf"
"~/.config/kigae/p10k.zsh" = "~/.config/ukiyo/p10k.zsh"
"~/.config/nvim/lua/plugins/kigae.lua" = "~/.config/ukiyo/nvim.lua"

[vscode]                          # settings.json keys; tables become JSON objects
"workbench.colorTheme" = "Kanagawa"

[daynight.light]                  # optional: colors/style/icons/cursor/wallpaper/kconfig
colors = "Itchy"
kconfig.kwinrc."org.kde.kdecoration2/theme" = "__aurorae__svg__Itchy"
[daynight.dark]
colors = "Scratchy"
kconfig.kwinrc."org.kde.kdecoration2/theme" = "__aurorae__svg__Scratchy"

[services]
enable = ["ukiyo-sky.timer"]

[hooks]
after = ["~/.local/bin/ukiyo-sky --force"]   # sh -c, after everything else

[install]                         # source → $HOME path, copied once by `kigae install`
"home/.local/share/plasma/desktoptheme/ukiyo" = ".local/share/plasma/desktoptheme/ukiyo"
"./og/kitty.conf" = ".config/kitty/og.conf"   # ./ = next to a local manifest

[deep]                            # root only, `kigae deep`
sddm = "ukiyo"
grub = "ukiyo/grub/install-grub-theme.sh"
```

Paths starting with `~` are in `$HOME`. `layout`, `preview` and `[install]` sources are paths inside the rice's branch, or `./…` next to a local manifest.

The shared base files each point at one Kigae-owned link and never change per rice: `kitty.conf` ends with `include kigae-current.conf`, `.zshrc` sources `~/.config/kigae/p10k.zsh` (falling back to `~/.p10k.zsh`), and Neovim loads `plugins/kigae.lua` when it exists.

### 3.1 When a rice doesn't theme an app

You choose this in `config.toml`, per section:

```toml
[unset]
default = "base"   # base | stock | keep
vscode  = "keep"   # overrides: kconfig, links, vscode
```

| Mode | The app gets |
|---|---|
| `base` (default) | your `~/.config/kigae/base.toml` (written like a rice), then the app's stock look for what it doesn't cover. With no base file this works the same as `stock`. |
| `stock` | the app's stock look: the previous rice's owned keys and links are deleted |
| `keep` | whatever the previous rice left |

Plasma parts a rice leaves out also come from `base.toml` if it sets them. A key set in a rice's mood counts as set by the rice, so the base never overrides day/night.

### 3.2 Capturing a rice

A rice doesn't need a hand-written manifest. Put it on screen once (the old way, or it's already there) and run `kigae capture <id>`. Kigae reads:

- the Plasma parts (colours, style, icons, cursor, wallpaper)
- the look keys of KConfig files: window decoration, KWin effects, fonts, accent colour, widget style, splash, Konsole/Kate themes, cursor size
- the panels: plasmashell's layout dump, rewritten to run after the prelude, with the dock taking `kigae.launchers` and `kigae.tray()` / `kigae.desktop()` for what the dump leaves out
- the Kigae links that exist, and VS Code's theme keys (with the icon theme's own settings)

Anything equal to `base.toml` is left out, so the manifest only holds what's specific to the rice. Kigae can't see services, hooks or a day/night pair; the command says so, and you add those by hand. It writes nothing over an existing manifest without `--force`, and runs `doctor` on the result.

## 4. Apply pipeline

1. **Resolve**: load the target manifest and the current one (from `~/.local/state/kigae/current`).
2. **Preflight**, the same check as `kigae doctor`: every named colour scheme, Plasma style, icon set, cursor, wallpaper, link target and service has to be installed (in either mood). If anything is missing, stop before touching the desktop and suggest `kigae install <id>`.
3. **Snapshot**: before each change, the old value goes into `~/.local/state/kigae/undo/<unix-ts>-<from>-<to>.json` (saved after every entry, so a crash leaves a usable journal). For the layout that's plasmashell's own `dumpCurrentLayoutJS`, which undo replays after the prelude. Parts whose value is already right are skipped and not journaled.
4. **Run** the parts in this order:

   | # | Part | How | Critical |
   |---|---|---|---|
   | 1 | services-stop | `systemctl --user disable` + `stop` for the old rice's units | yes |
   | 2 | kconfig | `kwriteconfig6 --notify` per owned key (and `AutomaticLookAndFeel=false`), then `org.kde.KWin /KWin reconfigure` if kwinrc changed | yes |
   | 3 | colors | `plasma-apply-colorscheme` | yes |
   | 4 | style | `plasma-apply-desktoptheme` | yes |
   | 5 | icons | `/usr/libexec/plasma-changeicons` | yes |
   | 6 | cursor | `plasma-apply-cursortheme` | yes |
   | 7 | layout | prelude + rice script through `org.kde.PlasmaShell.evaluateScript` | yes |
   | 8 | wallpaper | `plasma-apply-wallpaperimage` | no |
   | 9 | links | atomic symlink swap; `SIGUSR1` to kitty if a kitty link changed | no |
   | 10 | vscode | edit `settings.json` in place, comments and order kept | no |
   | 11 | services-start | `systemctl --user enable` + `start` | no |
   | 12 | hooks | `[hooks] after`, `sh -c` | no |
   | 13 | state | write `current` | no |

5. **Failure policy**: if a critical part fails, Kigae rolls back the parts it already applied, using the snapshot, and returns an error. A non-critical part that fails is logged, and the switch counts as done with warnings.
6. `--dry-run` reads the live desktop but records the actions instead of running them, and prints them.

**How apps follow**: kitty, p10k and Neovim through `[links]` (kitty reloads on `SIGUSR1`, open shells on `exec zsh`, Neovim on restart). VS Code through `[vscode]` (it reloads `settings.json` live). Konsole, Kate and the splash through `[kconfig]`; Konsole and Kate change in new windows only.

## 5. Layout contract

Kigae runs its prelude (`crates/kigae-core/src/prelude.js`) and then the rice's layout file, as one `evaluateScript` call.

- The prelude reads the launcher list of the first task manager into `kigae.launchers`. Then it removes **every** panel and every desktop widget, on all containments.
- The rice script builds its panels from nothing and uses `kigae.launchers` for the dock.
- Two things can't be done inside that script, so the rice declares them and Kigae applies them in a second call about 600 ms later:
  - `kigae.tray(tray, {General: {hiddenItems: [...]}})`: Plasma drops tray settings written while the tray is being created.
  - `kigae.desktop(type, x, y, w, h, config)`: the folder view only honours a position when the spot is free, and the prelude's removals don't finish within the same call.
- Undo replays plasmashell's layout dump, plus a capture of what the dump leaves out (tray settings, desktop widgets with their positions and settings), applied through the same second pass.
- Panel heights are set in the script, never inherited (a Plasma style changes panel padding). The script must run cleanly from any starting state.
- Any number of panels works: the prelude clears them all, the script builds as many as it wants. `new Panel` lands on screen 0; a rice for several monitors sets `panel.screen` itself.
- Kigae never edits `plasma-org.kde.plasma.desktop-appletsrc` while plasmashell is running, because that's the file plasmashell writes back on exit.

## 6. Day/night

- If the active rice has `[daynight]`, the daemon checks `org.kde.NightTime`'s schedule every minute. When day turns to night or back, it re-applies only the `light` or `dark` parts (no layout, no services; the rice's hooks do run, so a timer like ukiyo-sky can take over). A look picked by hand in the overlay (☀ or ☾ instead of auto) holds until that next turn.
- When the daemon isn't running, `kigae apply` picks the right half for the current time from the same schedule, and `kigae mood [light|dark]` re-applies just the mood parts of the active rice.
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
kigae apply <id> [--dry-run] [--mood light|dark] [--from <id>] [--force]
kigae mood [light|dark] [--dry-run]   re-apply the active rice's day/night parts
kigae undo [--list | <ts>]
kigae install <id> [--dry-run] [--force]   copy [install] assets into $HOME once
kigae uninstall <id>             remove exactly what install copied
kigae capture <id> [--name --kanji --tagline --branch] [--dry-run] [--force]
                                 read the rice on screen into manifests/<id>.toml + <id>/layout.js
kigae doctor [<id>...]           preflight check without applying
kigae shoot [<id>...]            apply each rice in turn and take its preview screenshot
kigae deep <id>                  SDDM + GRUB, one pkexec prompt
kigae daemon
kigae overlay                    ask the daemon to show the overlay
```

## 10. State and files

| Path | What |
|---|---|
| `~/.config/kigae/config.toml` | repo, branches, local manifests, `[unset]` |
| `~/.config/kigae/base.toml` | your defaults for what a rice leaves out |
| `~/.config/kigae/p10k.zsh` | link to the active rice's prompt |
| `~/.local/state/kigae/current` | active rice id. If it's missing, Kigae guesses from the Plasma style |
| `~/.local/state/kigae/undo/*.json` | one journal per switch |
| `~/.local/state/kigae/installed/<id>.json` | every file `install` copied |
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
