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

## Phase 2: daemon + overlay

- `kigae daemon` (zbus, `dev.kigae.Kigae`), `data/kigae.service`
- The cxx-qt QML overlay: layer-shell, exclusive keyboard, KWin blur shim
- `data/kigae-switch.desktop` (Meta+Shift+T → `kigae overlay`)
- Spike: when the overlay has focus, do T taps arrive as key events or as shortcut presses?

## Phase 3: Catppuccin + previews

- Catppuccin manifest (`rice/catppuccin-mocha`)
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
