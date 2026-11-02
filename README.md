# Kigae (着替え)

A live rice switcher for KDE Plasma 6. All your rices stay installed side by side, and switching just points Plasma, KWin, kitty, VS Code and the rest at another one. No logout, a few seconds.

```
$ kigae list
  og           Scratchy & Itchy 原  (backup/og)  day/night
* ukiyo        Ukiyo 浮世  (rice/ukiyo)
$ kigae apply og
ukiyo → og (dark): 29 changes
$ kigae undo
undid ukiyo → og (29 changes)
```

- `DESIGN.md`: the idea
- `SPEC.md`: how it works: manifest format, apply order, undo, layout contract
- `PLAN.md`: build phases (the Meta+Shift+T overlay is phase 2)

## Getting started

```
cargo install --path crates/kigae
kigae setup
```

The overlay needs Qt 6 and layer-shell-qt headers to build (`sudo dnf install qt6-qtbase-devel qt6-qtdeclarative-devel layer-shell-qt-devel`); `cargo install --path crates/kigae --no-default-features` builds the CLI alone.

`kigae setup` installs a systemd user service that starts the daemon at login (it keeps day/night in step with the sun and opens the fitting room), D-Bus activation for it, and binds Meta+Shift+T. It won't take the shortcut if something else has it. Run it again after each `cargo install`; `kigae setup --remove` takes it all back. Logs: `journalctl --user -t kigae`.

Kigae keeps its files in `~/.config/kigae`:

```
~/.config/kigae/
├── config.toml      where your rices are
├── base.toml        what applies under every rice (fonts, say), optional
└── manifests/       rices described here instead of on their own branch
```

A minimal `config.toml`:

```toml
repo = "~/dotfiles"      # one branch per rice (the default)
branches = ["rice/*"]    # which branches are rices (the default)
```

Then either give a rice branch a `rice.toml`, or put the rice on screen and run `kigae capture <id>`.

**Tip:** make `~/.config/kigae` a branch of your dotfiles, so it's versioned with everything else and readable whichever rice branch is checked out:

```
git -C ~/dotfiles worktree add --orphan -b kigae ~/.config/kigae
```

## A rice

A rice is a branch in your dotfiles repo with a `rice.toml` at its root (`SPEC.md` §3), or a manifest in `~/.config/kigae/manifests/` for branches you'd rather not touch. Kigae reads both from git, so nothing gets checked out.

The shared base files each point at one link that Kigae owns:

- `~/.config/kitty/kitty.conf` ends with `include kigae-current.conf`
- `~/.zshrc` sources `~/.config/kigae/p10k.zsh` (falling back to `~/.p10k.zsh`)
- Neovim loads `~/.config/nvim/lua/plugins/kigae.lua` when it exists

## Any rice, without writing a manifest

Put the rice on screen once, then:

```
kigae capture catppuccin --name "Catppuccin Mocha"
```

Kigae writes `manifests/catppuccin.toml` and `manifests/catppuccin/layout.js` from what's on screen. Add `[services]` or `[hooks]` by hand if the rice has background jobs.

## When a rice doesn't theme an app

Pick per section in `~/.config/kigae/config.toml`:

```toml
[unset]
default = "base"   # base: your ~/.config/kigae/base.toml, then the app's stock look
vscode  = "keep"   # stock: delete what the last rice set · keep: leave it
```
