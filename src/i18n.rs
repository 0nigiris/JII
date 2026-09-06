// SPDX-FileCopyrightText: 2026 0nigiris
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Localization (i18n).
//!
//! User-facing strings live in `locales/<lang>.toml`, **not** in Rust code — the code holds
//! only *keys* (ADR-0050). Locale files are embedded at build time (`include_str!`) so the
//! binary stays self-contained. English is the source of truth and the fallback; a missing
//! key degrades English → the raw key, never a panic.
//!
//! Keys are namespaced by dotted path mirroring the TOML nesting, e.g. `error.unknown_source`
//! or `install.searching`. Look up with the [`t!`](crate::t) macro:
//!
//! ```ignore
//! renderer.info(&t!("install.searching"));
//! renderer.error(&t!("install.not_found", names = missing.join(", ")));
//! ```
//!
//! Language is resolved once at startup by [`init`]: `--lang` › config `[ui] locale` (unless
//! `"auto"`) › `$LC_ALL`/`$LC_MESSAGES`/`$LANG` › English.

use std::collections::HashMap;
use std::sync::OnceLock;

/// Embedded locale sources. English is the source of truth (and fallback).
const EN: &str = include_str!("../locales/en.toml");
const RU: &str = include_str!("../locales/ru.toml");

/// The resolved active language's code and lookup tables (primary + English fallback).
struct Active {
    lang: &'static str,
    primary: HashMap<String, String>,
    english: HashMap<String, String>,
}

static ACTIVE: OnceLock<Active> = OnceLock::new();

/// Resolve and install the active language. Call once, early in `main`, after config load.
/// Idempotent-ish: a second call is ignored (the first resolution wins).
pub fn init(flag: Option<&str>, config_locale: &str) {
    let lang = resolve_lang(flag, config_locale);
    let primary = match lang {
        "ru" => flatten(RU),
        _ => flatten(EN),
    };
    let _ = ACTIVE.set(Active { lang, primary, english: flatten(EN) });
}

/// The active tables, defaulting to English if [`init`] was never called (e.g. in tests).
fn active() -> &'static Active {
    ACTIVE.get_or_init(|| {
        let english = flatten(EN);
        Active { lang: "en", primary: english.clone(), english }
    })
}

/// The active language code (`"en"` / `"ru"`). For content that carries its own
/// translations rather than living in `locales/*.toml` — see [`crate::changelog`].
pub fn lang() -> &'static str {
    active().lang
}

/// Look up a key: active language → English fallback → the key itself (never panics).
pub fn tr(key: &str) -> String {
    let a = active();
    a.primary
        .get(key)
        .or_else(|| a.english.get(key))
        .cloned()
        .unwrap_or_else(|| key.to_string())
}

/// Look up a key and substitute `{name}` placeholders from `args`.
pub fn tr_args(key: &str, args: &[(&str, String)]) -> String {
    let mut s = tr(key);
    for (name, val) in args {
        s = s.replace(&format!("{{{name}}}"), val);
    }
    s
}

/// Which grammatical form a count takes in the active language.
///
/// English needs two (one / other), Russian three: `1` (but not 11), `2–4` (but not 12–14),
/// and everything else. Getting this wrong is the single most obvious sign that software was
/// translated rather than written — "Найдено 3 вариантов" reads as a machine talking. So a
/// counted phrase is three keys (`<stem>.one` / `.few` / `.many`), and a language with fewer
/// forms simply repeats one of them.
fn plural_form(lang: &str, count: u64) -> &'static str {
    match lang {
        "ru" => {
            let (n10, n100) = (count % 10, count % 100);
            if n10 == 1 && n100 != 11 {
                "one"
            } else if (2..=4).contains(&n10) && !(12..=14).contains(&n100) {
                "few"
            } else {
                "many"
            }
        }
        // English and the fallback: singular or not.
        _ => {
            if count == 1 {
                "one"
            } else {
                "many"
            }
        }
    }
}

/// Look up a **counted** phrase: `stem` names a group of `.one`/`.few`/`.many` keys, and the
/// active language's rule for `count` picks one. `{count}` is substituted automatically, on
/// top of any `args` given. See [`plural_form`].
pub fn tr_count(stem: &str, count: u64, args: &[(&str, String)]) -> String {
    let key = format!("{stem}.{}", plural_form(active().lang, count));
    let mut all: Vec<(&str, String)> = vec![("count", count.to_string())];
    all.extend(args.iter().map(|(k, v)| (*k, v.clone())));
    tr_args(&key, &all)
}

/// Translate a key in a *specific* shipped language, bypassing the globally active one
/// (which is fixed for the process by [`init`]). `jii lang <code>` uses this to confirm in
/// the language just chosen. `code` accepts `auto` — it resolves against the environment.
pub fn tr_in(code: &str, key: &str, args: &[(&str, String)]) -> String {
    let lang = if code == "auto" { resolve_lang(None, "auto") } else { normalize(code) };
    let table = if lang == "ru" { flatten(RU) } else { flatten(EN) };
    let english = flatten(EN);
    let mut s = table
        .get(key)
        .or_else(|| english.get(key))
        .cloned()
        .unwrap_or_else(|| key.to_string());
    for (name, val) in args {
        s = s.replace(&format!("{{{name}}}"), val);
    }
    s
}

/// Resolve the language code (`"en"`/`"ru"`) from, in order: an explicit `--lang` flag, the
/// config `[ui] locale` (unless `"auto"`/empty), then `$LC_ALL`/`$LC_MESSAGES`/`$LANG`, then
/// English. Only the languages we ship are honoured; anything else falls back to English.
fn resolve_lang(flag: Option<&str>, config_locale: &str) -> &'static str {
    if let Some(code) = flag.map(normalize).filter(|c| !c.is_empty()) {
        return code;
    }
    if !config_locale.is_empty() && config_locale != "auto" {
        return normalize(config_locale);
    }
    for var in ["LC_ALL", "LC_MESSAGES", "LANG"] {
        if let Ok(v) = std::env::var(var)
            && !v.is_empty()
        {
            return normalize(&v);
        }
    }
    "en"
}

/// Normalise a locale string (`ru_RU.UTF-8`, `en_US`, `C`, `ru`) to a shipped code. Unknown
/// or C/POSIX locales map to English.
fn normalize(raw: &str) -> &'static str {
    let code = raw
        .split(['_', '.', '@', '-'])
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    match code.as_str() {
        "ru" => "ru",
        _ => "en",
    }
}

/// Flatten a locale TOML into `dotted.key → value`, mirroring the table nesting. Non-string
/// leaves are ignored; a malformed file yields an empty map (a broken locale must not crash).
fn flatten(src: &str) -> HashMap<String, String> {
    let value: toml::Value = toml::from_str(src).unwrap_or_else(|_| toml::Value::Table(Default::default()));
    let mut map = HashMap::new();
    flatten_into(&value, String::new(), &mut map);
    map
}

fn flatten_into(value: &toml::Value, prefix: String, map: &mut HashMap<String, String>) {
    match value {
        toml::Value::Table(table) => {
            for (key, val) in table {
                let path = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{prefix}.{key}")
                };
                flatten_into(val, path, map);
            }
        }
        toml::Value::String(s) => {
            map.insert(prefix, s.clone());
        }
        _ => {}
    }
}

/// Translate a key, with optional `name = value` interpolation arguments.
///
/// `t!("common.aborted")` · `t!("install.not_found", names = list)`
#[macro_export]
macro_rules! t {
    ($key:expr $(,)?) => {
        $crate::i18n::tr($key)
    };
    ($key:expr, $($name:ident = $val:expr),+ $(,)?) => {
        $crate::i18n::tr_args($key, &[$((stringify!($name), ($val).to_string())),+])
    };
}

/// Counted sibling of [`t!`]: `tn!("search.found", n)` picks `search.found.one` /
/// `.few` / `.many` by the active language's plural rule and fills `{count}`.
#[macro_export]
macro_rules! tn {
    ($stem:expr, $count:expr $(,)?) => {
        $crate::i18n::tr_count($stem, $count as u64, &[])
    };
    ($stem:expr, $count:expr, $($name:ident = $val:expr),+ $(,)?) => {
        $crate::i18n::tr_count($stem, $count as u64, &[$((stringify!($name), ($val).to_string())),+])
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn en_and_ru_have_identical_key_sets() {
        // The core guarantee: every key exists in *both* locales, so no translation is
        // silently missing. Adding a key to one file without the other fails here.
        let en = flatten(EN);
        let ru = flatten(RU);
        let mut en_keys: Vec<&String> = en.keys().collect();
        let mut ru_keys: Vec<&String> = ru.keys().collect();
        en_keys.sort();
        ru_keys.sort();
        assert_eq!(en_keys, ru_keys, "en and ru locale keys must match exactly");
        assert!(!en.is_empty(), "locale files must not be empty");
    }

    /// A string with `{placeholders}` must never be looked up without arguments.
    ///
    /// `t!("setup.gh_why_not_rc")` shipped exactly that way: users read "Deliberately not
    /// `export {env}`", braces and all, on every distro the tester tried. The single-argument
    /// arm of `t!` substitutes nothing, so the mistake is invisible until someone reads the
    /// output. Scans the source for that shape and names the offender.
    #[test]
    fn a_string_with_placeholders_is_never_looked_up_without_arguments() {
        let en = flatten(EN);
        let with_placeholder: std::collections::HashSet<&String> = en
            .iter()
            .filter(|(_, v)| v.contains('{') && v.contains('}'))
            .map(|(k, _)| k)
            .collect();

        let mut offenders: Vec<String> = Vec::new();
        for file in source_files(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src")) {
            // Comment lines are dropped first — including this test's own doc comment, which
            // quotes the offending call as prose and would otherwise report itself.
            let text: String = std::fs::read_to_string(&file)
                .unwrap_or_default()
                .lines()
                .filter(|l| !l.trim_start().starts_with("//"))
                .collect::<Vec<_>>()
                .join("\n");
            // Every `t!("some.key")` whose closing paren follows the string directly — i.e.
            // the no-arguments arm of the macro.
            for (at, _) in text.match_indices("t!(\"") {
                let rest = &text[at + 4..];
                let Some(end) = rest.find('"') else { continue };
                let key = &rest[..end];
                if rest[end + 1..].trim_start().starts_with(')') && with_placeholder.contains(&key.to_string()) {
                    offenders.push(format!("{}: {key}", file.display()));
                }
            }
        }
        assert!(
            offenders.is_empty(),
            "these keys carry {{placeholders}} but are looked up with no arguments:\n  {}",
            offenders.join("\n  ")
        );
    }

    /// Every `.rs` file under `dir`, recursively.
    fn source_files(dir: std::path::PathBuf) -> Vec<std::path::PathBuf> {
        let mut out = Vec::new();
        let Ok(entries) = std::fs::read_dir(&dir) else {
            return out;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                out.extend(source_files(path));
            } else if path.extension().is_some_and(|e| e == "rs") {
                out.push(path);
            }
        }
        out
    }

    #[test]
    fn every_boss_and_ending_has_its_text() {
        // Adding a fight means adding a row to BOSSES, badges to the catalog *and* text here.
        // Miss the text and the badge renders as its own key — so check the whole shape.
        // The parity test above then guarantees ru has everything en does.
        let en = flatten(EN);
        let mut want: Vec<String> = Vec::new();
        for boss in crate::achievements::BOSSES {
            want.push(format!("achieve.{}.title", boss.id));
            want.push(format!("achieve.{}.desc", boss.id));
            for ending in boss.endings {
                want.push(format!("achieve.{}.toast-{ending}", boss.id));
                want.push(format!("achieve.{}-{ending}.title", boss.id));
                want.push(format!("achieve.{}-{ending}.desc", boss.id));
            }
            if !boss.endings.is_empty() {
                want.push(format!("achieve.{}-both.title", boss.id));
                want.push(format!("achieve.{}-both.desc", boss.id));
            }
        }
        let missing: Vec<&String> = want.iter().filter(|k| !en.contains_key(*k)).collect();
        assert!(missing.is_empty(), "locale keys missing for a boss fight: {missing:?}");
    }

    #[test]
    fn locales_parse_and_flatten_dotted_keys() {
        let en = flatten(EN);
        // A representative migrated key must resolve (guards against a broken TOML shape).
        assert!(en.contains_key("common.aborted"));
    }

    #[test]
    fn normalize_maps_locale_variants() {
        assert_eq!(normalize("ru_RU.UTF-8"), "ru");
        assert_eq!(normalize("ru"), "ru");
        assert_eq!(normalize("en_US"), "en");
        assert_eq!(normalize("C"), "en");
        assert_eq!(normalize("POSIX"), "en");
        assert_eq!(normalize("de_DE"), "en"); // unshipped → English
    }

    #[test]
    fn resolve_prefers_flag_then_config_then_default() {
        assert_eq!(resolve_lang(Some("ru"), "auto"), "ru"); // flag wins
        assert_eq!(resolve_lang(None, "ru_RU.UTF-8"), "ru"); // config next
        assert_eq!(resolve_lang(Some("en"), "ru"), "en"); // flag over config
    }

    #[test]
    fn tr_args_substitutes_placeholders() {
        // Uses the English fallback catalog (init not called in this unit test).
        let s = tr_args("install.not_found", &[("names", "vlc, htop".to_string())]);
        assert!(s.contains("vlc, htop"), "placeholder not substituted: {s}");
    }

    #[test]
    fn missing_key_returns_itself() {
        assert_eq!(tr("no.such.key.exists"), "no.such.key.exists");
    }
}
