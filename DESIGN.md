# Kigae (着替え)

*Kigae* means "changing clothes". This is a rice switcher for the Plasma desktop on vortex: hold **Meta+Shift**, tap **T**, and a strip of rices slides up over the screen. Each card is a screenshot of one rice. Tap T again to move to the next card, then let go to switch. The whole desktop changes in a couple of seconds, with no logout.

Status: design, nothing built yet (2026-11-01). The build spec is `SPEC.md` (it wins where the two disagree); the build order is `PLAN.md`.

## Why build it

Nothing ready-made does this on KDE:

- **Konsave** saves and applies whole Plasma setups as profiles, but it does that by copying config files. Plasma only picks those up after you log out, and plasmashell can overwrite them when it exits.
- **Tinty / flavours** (base16) swap colours across apps, but only colours: no panel layout, no wallpaper, no Plasma style, no icons.
- **HyDE, ML4W** have the theme picker people show off (a wallpaper carousel on a hotkey), but they only work on Hyprland.

The way rices switch today is `git checkout` → `install.sh` → log out. `ukiyo/revert.sh` shows the hidden costs: plasmashell has to be stopped first so it doesn't write its config back, files are rsynced over `$HOME`, and the whole thing takes minutes.

## The key idea: install everything, then switch by pointing

Every part of a rice is a file in a folder of its own: a colour scheme in `color-schemes/`, a Plasma style in `desktoptheme/`, icons in `icons/`, a kitty theme file. These don't conflict with each other, so **all rices can be installed side by side**. Switching then never copies files. It tells each program which of the installed ones to use, through the tools Plasma already has for applying changes live:

| Part | Live switch |
|---|---|
| Colour scheme | `plasma-apply-colorscheme <name>` |
| Plasma style (panels, popups) | `plasma-apply-desktoptheme <name>` |
| Icons | `/usr/libexec/plasma-changeicons <name>` |
| Cursor | `plasma-apply-cursortheme <name>` |
| Wallpaper | `plasma-apply-wallpaperimage <file>`, or the rice's own timer (Ukiyo's `ukiyo-sky`) |
| Window decoration, KWin effects | `kwriteconfig6 --file kwinrc …` then `org.kde.KWin /KWin reconfigure` over D-Bus (Fedora's tool is `qdbus-qt6`; Kigae uses zbus) |
| Panel layout | the rice's `layout.js` run through `org.kde.PlasmaShell.evaluateScript` (Ukiyo already has one) |
| Desktop widgets | the same script (it adds or removes `org.ukiyo.clock` and so on) |
| GTK apps | follow automatically (KDE's gtkconfig module rewrites GTK colours when the scheme changes) |
| kitty | point `current-rice.conf` at the rice's theme file, then `kill -USR1` every kitty (kitty reloads its config on SIGUSR1) |
| p10k prompt | point `~/.p10k.zsh` at the rice's copy; new shells pick it up, open ones on `exec zsh` |
| VS Code | edit `workbench.colorTheme` (+ customizations) in `settings.json`; VS Code reloads it live |
| Neovim | `vim.g.rice` read from a state file at startup |
| Kate/KWrite, Konsole | `kwriteconfig6` the theme/profile; new windows only |
| Splash | `ksplashrc`, shown at the next login (it can't show any earlier) |
| Login screen, GRUB | need root: separate **deep switch** (`pkexec`), never on the hotkey |

Each rice also turns its own background services on or off. For example, `ukiyo-sky.timer` runs only while Ukiyo is active; otherwise it would put Ukiyo wallpapers on top of another rice.

## A rice is a manifest

Each rice branch gets a `rice.toml` at the root that names its parts:

```toml
name = "Ukiyo"
kanji = "浮世"
preview = "ukiyo/screens/desktop.jpg"

[plasma]
colors = "Ukiyo"           # day/night is the rice's own business (ukiyo-sky)
style = "ukiyo"
icons = "Colloid-Yellow-Everforest-Dark"
cursor = "breeze_cursors"
decoration = { library = "org.kde.breeze", theme = "Breeze", borders = "None" }
layout = "ukiyo/layout.js"

[kwin]
"Plugins/fadeEnabled" = true
"Plugins/scaleEnabled" = false
"Effect-blur/BlurStrength" = 9

[apps]
kitty = "ukiyo.conf"
p10k = "~/.config/ukiyo/p10k.zsh"
vscode = { theme = "Kanagawa" }
konsole = "Ukiyo"
kate = "Ukiyo"

[services]
enable = ["ukiyo-sky.timer"]

[deep]                      # pkexec, only on `kigae deep`
sddm = "ukiyo"
grub = "ukiyo/grub/install-grub-theme.sh"
```

Kigae reads manifests straight from git (`git show rice/ukiyo:rice.toml`) without checking anything out, so the repo can stay on whatever branch it's on. `kigae install <branch>` copies a rice's asset folders (themes, icons, wallpapers, plasmoids) into `$HOME` once. After that, switching only points at them.

Rices to start with: **Ukiyo** (`rice/ukiyo`), **your own design** (`backup/og`: Scratchy/Itchy with its automatic day/night, which Kigae takes over through `[daynight]` so `AutomaticLookAndFeel` can stay off; og's manifest lives in `Kigae/manifests/` because `backup/og` is a restore point), and **Catppuccin Mocha** (`rice/catppuccin-mocha`). The two older branches need their manifests written by reading their `home/` configs.

## The overlay

- A frameless full-screen Qt Quick window: a blurred, dimmed copy of the current desktop, with a row of cards along the bottom third. Each card shows the rice's preview, its name and kanji, and a short line saying what it is.
- It works like Alt-Tab. **Meta+Shift+T** opens it with the next rice selected, each tap of T moves one card along, Shift+T moves back, and **letting go applies**. Esc cancels. Clicking a card works too.
- Applying: the overlay fades to the new rice's preview, applies everything behind it, then fades out to the live desktop. You see one smooth change instead of panels flickering one at a time.
- Launched by a global shortcut: a `.desktop` file with `X-KDE-Shortcuts`. The overlay stays resident after first use, so it opens instantly.
- Written in Rust (engine, CLI, daemon) with the overlay in QML hosted through cxx-qt; see `SPEC.md` §0 for why. The engine is a library with a CLI (`kigae apply ukiyo`, `kigae list`, `kigae current`, `kigae undo`), so the overlay is only a front end.

## Safety

- Before every switch, Kigae saves every config key it's about to change into `~/.local/state/kigae/undo/<time>.json`. `kigae undo` puts them back.
- Panels are rebuilt through the scripting API, never by editing `plasma-org.kde.plasma.desktop-appletsrc` while plasmashell is running, because that's the file plasmashell writes back on exit. Every `layout.js` has to start from any layout and keep the dock's launchers (Ukiyo's already carries the launcher list over).
- Never `plasma-apply-lookandfeel`: it resets far more than it should (layout, splash, widget style), and `AutomaticLookAndFeel` stays off.
- The deep switch (login screen, GRUB) uses the existing install/revert scripts, behind one pkexec prompt, and only when asked.

## Known limits

- Some windows keep their old look until you reopen them: Konsole, Kate, Obsidian, and already-running GTK2 apps. Kitty, VS Code, Dolphin and everything Qt or KDE switch live.
- The splash screen and SDDM can only show up at the next login.
- A Plasma style changes panel padding, so a rice's `layout.js` has to set its own panel heights (Ukiyo's 56px dock and tray island).

## Build order

1. **Engine + CLI** with manifests for Ukiyo and `backup/og`, switching both ways live. This is the part that proves the idea; the rest is presentation.
2. **Overlay** (Meta+Shift+T, cards, apply on release, crossfade).
3. **Catppuccin manifest**, plus a preview screenshot for every rice (`kigae shoot` applies each rice in turn and takes the screenshot).
4. **Deep switch** for SDDM and GRUB.
5. Optional: fold Ukiyo's day/night into Kigae as two "moods" of one rice, so the overlay can show both.

Code lives in `~/Code/Kigae`; manifests and assets stay with their rices in `~/Code/dotfiles`.
