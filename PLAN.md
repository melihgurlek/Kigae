# Kigae build plan

The design is in `SPEC.md`. Each phase ends with something that works on the real desktop.

## Phase 0: scaffolding ✅ (2026-11-01)

- [x] `git init`, Cargo workspace (`crates/kigae-core`, `crates/kigae`)
- [x] `SPEC.md`, `PLAN.md`
- [x] Manifest types with parsing tests
- [ ] Overlay build deps (needs root, run by hand):
  `sudo dnf install qt6-qtbase-devel qt6-qtdeclarative-devel kf6-kwindowsystem-devel layer-shell-qt-devel`

## Phase 1: engine + CLI, Ukiyo ⇄ og working live ✅ (2026-11-01)

- [x] `kigae-core`: config, rice discovery (git + local `manifests/`), `Sys` trait (real / dry run / fake)
- [x] Parts in apply order, owned keys, `[unset]` policy + `base.toml`, rollback on critical failure
- [x] Undo journal; `kigae list | current | apply | mood | undo | doctor | install | uninstall`
- [x] Prelude, Ukiyo's `kigae-layout.js`, og's `manifests/og/layout.js`
- [x] Manifests: `rice.toml` on `rice/ukiyo`, `manifests/og.toml` (+ og's kitty theme)
- [x] Base files point at Kigae links: kitty `include kigae-current.conf`, `.zshrc` → `~/.config/kigae/p10k.zsh`, nvim `plugins/kigae.lua`; `ukiyo-sky --force`
- [x] Spike: 10 live switches Ukiyo ⇄ og with no plasmashell restart (3–7 s each), launchers kept, two undos walk back cleanly

Left for later: a `day/night` timer for og until the daemon exists (`kigae mood` works by hand).

## Phase 1b: `kigae capture`, so any rice can be switched ✅ (2026-11-01)

Kigae only switches rices that have a manifest, and writing one by hand (plus a panel script) is
the real barrier. `kigae capture <id>` reads the rice on screen into `manifests/<id>.toml` and
`manifests/<id>/layout.js`:

- [x] Plasma parts: colours, style, icons, cursor, wallpaper
- [x] Look keys from KConfig files: window decoration, KWin effects, fonts, accent colour, widget
      style, splash, Konsole/Kate themes, cursor size
- [x] Panels: plasmashell's layout dump, rewritten to run after the prelude (dock launchers from
      `kigae.launchers`), plus `kigae.tray()` / `kigae.desktop()` for what the dump leaves out
- [x] Kigae links that exist (kitty, p10k, nvim) and VS Code theme keys
- [x] Notes for what it can't see: services, hooks, day/night pairs, unlinked app themes
- [x] Refuses to overwrite without `--force`; the result passes `Manifest::parse` and `doctor`
- [x] Live test: captured Ukiyo as a new rice, switched og → captured: same panels, tray and clock; against the real Ukiyo only the timer, hook and layout rerun differ
- [x] Values equal to `base.toml` are left out (it found the fonts, which og and Ukiyo share, so they moved to `base.toml`)

Follow-ups:
- [ ] Notification popup position: own `plasmanotifyrc` in Ukiyo's `rice.toml`, and add it to capture's look keys
- [ ] `kigae capture <id> --mood light|dark`: fill `[daynight]` from two captures
- [ ] Unlinked app themes: when kitty's theme is a plain file, capture copies it next to the manifest and adds the link itself (today it only prints a note)
- [ ] Desktop widgets on every screen, not just the first (capture and the second layout pass)
- [ ] Small: exit quietly on a broken pipe (`kigae … | head`)

## Phase 2: daemon + overlay

**UI direction (decided 2026-11-01; replaces DESIGN.md's Alt-Tab strip and SPEC.md §7 where they differ): a wardrobe, not a task switcher.**
- *Fitting room* (Meta+Shift+T): desktop dims and blurs; rices hang in a slow horizontal carousel with depth (selected card large and centred, neighbours smaller and turned). Card: screenshot, big kanji, name + tagline, palette swatches from the rice's colour scheme, ☀/☾ toggle for `[daynight]` rices.
- *Try it on before you switch*: as the selection moves, the room crossfades to that rice's wallpaper and the overlay takes its accent colours.
- Arrows / scroll / T move, Enter or click switches, Esc closes; a quick tap leaves it open.
- *Stepping in*: the chosen card grows to fill the screen, Kigae applies everything behind it, then it dissolves into the live desktop.
- *Drawer* (Tab or ⋯): capture this desktop (name, kanji, one button), a health badge per rice (doctor), switch history as a timeline with undo.
- Motion 200–350 ms eased; honour reduced motion.


Done (2026-11-02, branch `phase2-overlay`):
- [x] `kigae daemon`: owns `dev.kigae.Kigae` (zbus, one method `Show()`), runs Qt + `qml/Overlay.qml` through cxx-qt 0.10 (feature `overlay`, on by default; `--no-default-features` = CLI only)
- [x] `kigae overlay` calls `Show()`; a second `Show()` while open moves to the next card (120 ms debounce)
- [x] `kigae-core::card`: per-rice card data (palette from the `.colors` file, light/dark looks, wallpaper, preview cached from git at `~/.cache/kigae/previews/`)
- [x] Fitting room renders on the live desktop (layer-shell Overlay, exclusive keyboard): blurred + dimmed wallpaper of the selected look, carousel with depth, hanging kanji tag in the rice's most vivid colour, band in the rice's own window/text colours, swatches, ☀/☾ pills, "wearing now" and "needs install" badges, key hints
- [x] Written but **not yet tried live**: arrows/T/scroll/click navigation, ↑↓ mood, Enter/Space/Meta-release wears, stepping-in veil (card grows to the target wallpaper, apply on a thread, dissolve), error toast with Undo, Tab drawer (history + undo, health, capture hint), reduced motion via `AnimationDurationFactor`
- Blur is QML `MultiEffect` on the wallpaper, not KWin blur behind: no C++ shim needed

Next (pick up here):
- [ ] Live test by hand: Meta+Shift+T hold → T → release wears; quick tap → browse mode; Esc changes nothing; stepping-in looks smooth (no panel flicker)
- [ ] `data/kigae.service` (systemd user), `data/dev.kigae.Kigae.service` (D-Bus activation → systemd), `data/kigae-switch.desktop` with `X-KDE-Shortcuts=Meta+Shift+T` (free on vortex), and a `kigae setup` or README step that installs them; `cargo install --path crates/kigae`
- [ ] Qt logs go to journald (`journalctl --user -t kigae`); a QML load error leaves the daemon running with no window: exit instead
- [ ] Silence the GCC 16 `-Wsfinae-incomplete` noise from Qt headers in the cxx-qt build
- [ ] Capture from the drawer (name + kanji + one button) instead of the command hint
- [ ] SPEC.md §7–8: rewrite to match (wardrobe UI, D-Bus is only `Show()`, the CLI keeps the lock file instead of routing apply through the daemon)

## Phase 3: Catppuccin + previews

- Catppuccin: apply it the old way once, then `kigae capture catppuccin-mocha`
- `kigae shoot`: apply each rice and take a screenshot with `spectacle -b -f -n -o`

## Phase 4: deep switch

- `kigae deep <id>`: SDDM + GRUB through the existing scripts, one `pkexec`

## Phase 5 (optional): moods

- Ukiyo's `ukiyo-sky` day/night becomes `[daynight]`, and the overlay shows both moods

## Verification

- **Unit** (`cargo test -p kigae-core`, no Qt): manifest parsing, rice discovery against a temporary git repo, golden `--dry-run` command lists for Ukiyo → og and og → Ukiyo, and a journal round trip.
- **Live, phase 1**:
  - `kigae doctor ukiyo og` is clean.
  - Ten rounds of `apply og` / `apply ukiyo`: panels and dock launchers survive, kitty recolours, VS Code's theme follows.
  - `ukiyo-sky.timer` runs only under Ukiyo.
  - `AutomaticLookAndFeel` stays `false`.
  - `kigae undo` brings back the previous state.
- **Live, phase 2**:
  - Meta+Shift+T, then T twice, then let go: the third rice is applied.
  - A quick tap lands in browse mode, and Esc changes nothing.
  - No panels flicker during the crossfade.
