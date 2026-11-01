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

## Build

```
cargo build --release && cp target/release/kigae ~/.local/bin/
```

## A rice

A rice is a branch in your dotfiles repo with a `rice.toml` at its root (`SPEC.md` §3), or a manifest in `manifests/` for branches that stay untouched. Kigae reads both from git, so nothing gets checked out.

The shared base files each point at one link that Kigae owns:

- `~/.config/kitty/kitty.conf` ends with `include kigae-current.conf`
- `~/.zshrc` sources `~/.config/kigae/p10k.zsh` (falling back to `~/.p10k.zsh`)
- Neovim loads `~/.config/nvim/lua/plugins/kigae.lua` when it exists

## When a rice doesn't theme an app

Pick per section in `~/.config/kigae/config.toml`:

```toml
[unset]
default = "base"   # base: your ~/.config/kigae/base.toml, then the app's stock look
vscode  = "keep"   # stock: delete what the last rice set · keep: leave it
```
