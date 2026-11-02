//! What the overlay shows for one rice: names, wallpaper, palette from its colour scheme and a
//! cached preview screenshot (SPEC.md §2–3). Nothing here fails; missing pieces are `None`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::discover::{git, Rice};
use crate::engine::Paths;
use crate::manifest::{Look, Manifest, MoodKind};

#[derive(Debug, Clone, Serialize)]
pub struct Card {
    pub id: String,
    pub name: String,
    pub kanji: Option<String>,
    pub tagline: Option<String>,
    pub daynight: bool,
    /// Cached screenshot (`<cache>/previews/<id>-<key>.<ext>`).
    pub preview: Option<PathBuf>,
    /// Without `[daynight]`, light and dark are both the plain look.
    pub light: LookCard,
    pub dark: LookCard,
}

#[derive(Debug, Clone, Serialize)]
pub struct LookCard {
    /// Expanded path, `None` if unset or the file doesn't exist.
    pub wallpaper: Option<PathBuf>,
    /// Colour scheme name.
    pub colors: Option<String>,
    pub palette: Option<Palette>,
}

/// Colours of a scheme as `#rrggbb`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Palette {
    /// `[Colors:Window]` BackgroundNormal.
    pub background: String,
    /// `[Colors:View]` BackgroundNormal.
    pub surface: String,
    /// `[Colors:View]` ForegroundNormal.
    pub text: String,
    /// `[General]` AccentColor, else `[Colors:Selection]` BackgroundNormal.
    pub accent: String,
    /// 4 to 6 distinct colours for a strip of dots.
    pub swatches: Vec<String>,
}

/// `m` is the effective manifest (`rice.manifest.with_base(..)`).
pub fn card(rice: &Rice, m: &Manifest, paths: &Paths, cache_dir: &Path) -> Card {
    let dirs = scheme_dirs();
    let daynight = m.daynight.is_some();
    let look = |mood| look_card(&m.look(mood), paths, &dirs);
    let (light, dark) = if daynight {
        (look(Some(MoodKind::Light)), look(Some(MoodKind::Dark)))
    } else {
        (look(None), look(None))
    };
    Card {
        id: m.id.clone(),
        name: m.name.clone(),
        kanji: m.kanji.clone(),
        tagline: m.tagline.clone(),
        daynight,
        preview: m.preview.as_deref().and_then(|p| cache_preview(rice, &m.id, p, cache_dir)),
        light,
        dark,
    }
}

fn look_card(look: &Look, paths: &Paths, dirs: &[PathBuf]) -> LookCard {
    LookCard {
        wallpaper: look.wallpaper.as_deref().map(|w| paths.expand(w)).filter(|p| p.is_file()),
        palette: look.colors.as_deref().and_then(|c| scheme_palette(c, dirs)),
        colors: look.colors.clone(),
    }
}

/// Where KDE looks for `<name>.colors`, most specific first.
fn scheme_dirs() -> Vec<PathBuf> {
    let env = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty());
    let data_home = env("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| env("HOME").map(|h| PathBuf::from(h).join(".local/share")));
    let data_dirs = env("XDG_DATA_DIRS")
        .map(|v| std::env::split_paths(&v).collect())
        .unwrap_or_else(|| vec![PathBuf::from("/usr/local/share"), PathBuf::from("/usr/share")]);
    data_home
        .into_iter()
        .chain(data_dirs)
        .map(|d| d.join("color-schemes"))
        .collect()
}

/// Palette of the first `<name>.colors` found in `dirs`.
fn scheme_palette(name: &str, dirs: &[PathBuf]) -> Option<Palette> {
    if name.is_empty() || name.contains('/') {
        return None;
    }
    let file = format!("{name}.colors");
    let text = dirs.iter().find_map(|d| std::fs::read_to_string(d.join(&file)).ok())?;
    Some(parse_palette(&text))
}

type Rgb = [u8; 3];

/// `Group → key → colour` for the groups a palette needs.
fn parse_scheme(text: &str) -> HashMap<(String, String), Rgb> {
    let mut out = HashMap::new();
    let mut group = String::new();
    for line in text.lines().map(str::trim) {
        if let Some(g) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            group = g.to_string();
        } else if let Some((k, v)) = line.split_once('=')
            && let Some(rgb) = parse_rgb(v)
        {
            out.insert((group.clone(), k.trim().to_string()), rgb);
        }
    }
    out
}

/// `r,g,b` or `r,g,b,a` (alpha ignored).
fn parse_rgb(v: &str) -> Option<Rgb> {
    let mut it = v.split(',').map(|p| p.trim().parse::<u8>());
    let rgb = [it.next()?.ok()?, it.next()?.ok()?, it.next()?.ok()?];
    let alpha_ok = it.next().is_none_or(|a| a.is_ok());
    (alpha_ok && it.next().is_none()).then_some(rgb)
}

fn hex([r, g, b]: Rgb) -> String {
    format!("#{r:02x}{g:02x}{b:02x}")
}

fn dist(a: Rgb, b: Rgb) -> u32 {
    a.iter().zip(b).map(|(&x, y)| (x as i32 - y as i32).unsigned_abs()).sum()
}

fn chroma([r, g, b]: Rgb) -> u8 {
    r.max(g).max(b) - r.min(g).min(b)
}

/// Colours closer than this (sum of channel differences) count as the same dot.
const NEAR: u32 = 36;

/// Breeze Light, for keys a scheme leaves out.
const DEFAULT_BG: Rgb = [239, 240, 241];
const DEFAULT_SURFACE: Rgb = [252, 252, 252];
const DEFAULT_TEXT: Rgb = [35, 38, 39];
const DEFAULT_ACCENT: Rgb = [61, 174, 233];

fn parse_palette(text: &str) -> Palette {
    let s = parse_scheme(text);
    let get = |g: &str, k: &str| s.get(&(g.to_string(), k.to_string())).copied();
    let background = get("Colors:Window", "BackgroundNormal")
        .or_else(|| get("Colors:View", "BackgroundNormal"))
        .unwrap_or(DEFAULT_BG);
    let surface = get("Colors:View", "BackgroundNormal").unwrap_or(DEFAULT_SURFACE);
    let text = get("Colors:View", "ForegroundNormal")
        .or_else(|| get("Colors:Window", "ForegroundNormal"))
        .unwrap_or(DEFAULT_TEXT);
    let accent = get("General", "AccentColor")
        .or_else(|| get("Colors:Selection", "BackgroundNormal"))
        .unwrap_or(DEFAULT_ACCENT);

    let mut swatches: Vec<Rgb> = vec![];
    let push = |c: Rgb, swatches: &mut Vec<Rgb>| {
        if swatches.iter().all(|&o| dist(o, c) >= NEAR) {
            swatches.push(c);
        }
    };
    for c in [background, surface, accent, text] {
        push(c, &mut swatches);
    }
    let mut extras: Vec<Rgb> = ["ForegroundLink", "ForegroundNegative", "ForegroundPositive", "ForegroundNeutral", "ForegroundActive"]
        .iter()
        .filter_map(|k| get("Colors:View", k))
        .collect();
    extras.sort_by_key(|&c| std::cmp::Reverse(chroma(c)));
    for c in extras {
        if swatches.len() >= 6 {
            break;
        }
        push(c, &mut swatches);
    }
    Palette {
        background: hex(background),
        surface: hex(surface),
        text: hex(text),
        accent: hex(accent),
        swatches: swatches.into_iter().map(hex).collect(),
    }
}

/// Copy the preview into the cache (once per content version); the cached path, or `None` on
/// any error.
fn cache_preview(rice: &Rice, id: &str, src: &str, cache_dir: &Path) -> Option<PathBuf> {
    let key = if let (Some(rest), Some(dir)) = (src.strip_prefix("./"), &rice.local_dir) {
        let mtime = std::fs::metadata(dir.join(rest)).ok()?.modified().ok()?;
        mtime.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs().to_string()
    } else {
        let spec = format!("{}:{src}", rice.branch.as_ref()?);
        let out = git(&rice.repo, &["rev-parse", &spec]).ok()?;
        String::from_utf8(out).ok()?.trim().to_string()
    };
    if key.is_empty() || !key.chars().all(|c| c.is_ascii_alphanumeric()) {
        return None;
    }
    let ext = Path::new(src).extension().and_then(|e| e.to_str()).unwrap_or("img");
    let dir = cache_dir.join("previews");
    let dest = dir.join(format!("{id}-{key}.{ext}"));
    if dest.is_file() {
        return Some(dest);
    }
    let bytes = rice.read(src).ok()?;
    std::fs::create_dir_all(&dir).ok()?;
    let tmp = dir.join(format!(".{id}-{key}.{}.tmp", std::process::id()));
    std::fs::write(&tmp, bytes).ok()?;
    if std::fs::rename(&tmp, &dest).is_err() {
        let _ = std::fs::remove_file(&tmp);
        return None;
    }
    remove_older(&dir, id, &dest);
    Some(dest)
}

/// Drop other versions of `id`'s preview. Keys have no '-', so `<id>-<key>.<ext>` can't be
/// confused with a rice whose id starts with `<id>-`.
fn remove_older(dir: &Path, id: &str, keep: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let prefix = format!("{id}-");
    for e in entries.flatten() {
        let name = e.file_name();
        let Some(rest) = name.to_str().and_then(|n| n.strip_prefix(&prefix)) else { continue };
        if !rest.contains('-') && e.path() != keep {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("kigae-card-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    const SCHEME: &str = "\
[ColorEffects:Disabled]
BackgroundNormal=1,2,3

; comment
[Colors:View]
BackgroundNormal=252,252,252,255
ForegroundNormal=20,20,20
ForegroundLink=100,100,180
ForegroundNegative=250,5,5
ForegroundPositive=30,160,40
ForegroundNeutral=253,253,250

[Colors:Window]
BackgroundNormal=239,240,241

[Colors:Selection]
BackgroundNormal=61,174,233

[General]
AccentColor=200,50,100
";

    #[test]
    fn hex_and_rgb() {
        assert_eq!(hex([0, 15, 255]), "#000fff");
        assert_eq!(parse_rgb("1, 2,3"), Some([1, 2, 3]));
        assert_eq!(parse_rgb("1,2,3,128"), Some([1, 2, 3]));
        assert_eq!(parse_rgb("1,2"), None);
        assert_eq!(parse_rgb("a,b,c"), None);
        assert_eq!(parse_rgb("1,2,3,4,5"), None);
    }

    #[test]
    fn palette_from_scheme() {
        let p = parse_palette(SCHEME);
        assert_eq!(p.background, "#eff0f1");
        assert_eq!(p.surface, "#fcfcfc");
        assert_eq!(p.text, "#141414");
        assert_eq!(p.accent, "#c83264");
        // Neutral is a near duplicate of the surface; red is the most saturated extra.
        assert_eq!(p.swatches[..4], ["#eff0f1", "#fcfcfc", "#c83264", "#141414"]);
        assert_eq!(p.swatches[4], "#fa0505");
        assert!(p.swatches.len() <= 6 && !p.swatches.contains(&"#fdfdfa".to_string()));
    }

    #[test]
    fn accent_falls_back_to_selection() {
        let text = SCHEME.replace("AccentColor=200,50,100", "");
        assert_eq!(parse_palette(&text).accent, "#3daee9");
    }

    #[test]
    fn missing_keys_use_defaults() {
        let p = parse_palette("[Colors:Window]\nBackgroundNormal=10,10,10\n");
        assert_eq!(p.background, "#0a0a0a");
        assert_eq!(p.surface, "#fcfcfc");
        assert_eq!(p.accent, "#3daee9");
    }

    #[test]
    fn scheme_search_order() {
        let (a, b) = (tmp("sa"), tmp("sb"));
        std::fs::write(b.join("X.colors"), SCHEME).unwrap();
        let dirs = [a.clone(), b.clone()];
        assert_eq!(scheme_palette("X", &dirs).unwrap().accent, "#c83264");
        std::fs::write(a.join("X.colors"), SCHEME.replace("200,50,100", "1,2,3")).unwrap();
        assert_eq!(scheme_palette("X", &dirs).unwrap().accent, "#010203");
        assert!(scheme_palette("Nope", &dirs).is_none());
        assert!(scheme_palette("../X", &dirs).is_none());
    }

    #[test]
    fn light_and_dark_looks() {
        let m = Manifest::parse(
            r#"
schema = 1
id = "t"
name = "T"
[plasma]
colors = "Dark"
[daynight.light]
colors = "Light"
[daynight.dark]
"#,
        )
        .unwrap();
        let dir = tmp("ld");
        std::fs::write(dir.join("Light.colors"), SCHEME).unwrap();
        let paths = Paths { home: dir.clone(), state: dir.join("state") };
        let dirs = [dir.clone()];
        let light = look_card(&m.look(Some(MoodKind::Light)), &paths, &dirs);
        let dark = look_card(&m.look(Some(MoodKind::Dark)), &paths, &dirs);
        assert_eq!(light.colors.as_deref(), Some("Light"));
        assert!(light.palette.is_some());
        assert_eq!(dark.colors.as_deref(), Some("Dark"));
        assert!(dark.palette.is_none());
    }

    #[test]
    fn wallpaper_must_exist() {
        let dir = tmp("wp");
        std::fs::write(dir.join("a.jpg"), b"x").unwrap();
        let paths = Paths { home: dir.clone(), state: dir.join("state") };
        let look = Look { wallpaper: Some("~/a.jpg".into()), ..Look::default() };
        assert_eq!(look_card(&look, &paths, &[]).wallpaper, Some(dir.join("a.jpg")));
        let look = Look { wallpaper: Some("~/b.jpg".into()), ..Look::default() };
        assert_eq!(look_card(&look, &paths, &[]).wallpaper, None);
    }

    #[test]
    fn local_preview_is_cached_and_replaced() {
        let dir = tmp("pv");
        let cache = dir.join("cache");
        std::fs::write(dir.join("shot.png"), b"one").unwrap();
        let m = Manifest::parse("schema = 1\nid = \"t\"\nname = \"T\"\npreview = \"./shot.png\"\n").unwrap();
        let rice = Rice { manifest: m.clone(), branch: None, local_dir: Some(dir.clone()), repo: dir.clone() };
        let paths = Paths { home: dir.clone(), state: dir.join("state") };
        let c = card(&rice, &m, &paths, &cache);
        let p = c.preview.unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), b"one");
        assert_eq!(p.extension().unwrap(), "png");
        // A stale version and another rice's file; only the stale one goes.
        let previews = cache.join("previews");
        std::fs::write(previews.join("t-1.png"), b"old").unwrap();
        std::fs::write(previews.join("t-x-5.png"), b"other").unwrap();
        remove_older(&previews, "t", &p);
        assert!(p.is_file() && !previews.join("t-1.png").exists() && previews.join("t-x-5.png").exists());
        // Second call finds it cached.
        assert_eq!(card(&rice, &m, &paths, &cache).preview.unwrap(), p);
        // Missing source: no preview, no panic.
        std::fs::remove_file(dir.join("shot.png")).unwrap();
        assert!(card(&rice, &m, &paths, &cache).preview.is_none());
    }
}
