// SPDX-License-Identifier: MIT OR Apache-2.0

//! User configuration in KDL v2 (https://kdl.dev). Optional: kiwa works with zero config.
//!
//! ```kdl
//! roots "~/dev" "D:/DEV"
//! depth 4
//! ignore "archive" "tmp"
//! accent "amber"
//! mouse #false
//! alias "t" "api" "test"          // `kw t` runs task `test` of project `api`
//! alias "w" "web" "dev" "--port" "4000"
//! hide "*:lint:fix" "legacy/*"   // tasks hidden from lists, as `project:task` patterns
//! show-git-only #false           // hide repositories that have no tasks
//! risky "deploy" "drop" "prod"    // words that make a task confirmed before it runs (replaces the defaults)
//! workspace "work" {             // kw -W work
//!     roots "~/work"
//!     depth 3
//! }
//! ```
//!
//! The file carries a format version (`version 1`). kiwa refuses a file written for a newer
//! format instead of misreading it, and keeps a `.bak` copy before each rewrite.
//!
//! Edits made by kiwa (`kw config set`, `kw alias add`, `kw init`) go through the `kdl`
//! document model, so the user's comments and layout are preserved.

use anyhow::{Context, Result, anyhow, bail};
use kdl::{KdlDocument, KdlEntry, KdlNode, KdlValue};
use std::path::{Path, PathBuf};

/// Accent colours, default first. None of them is green, yellow or red: those are kept for
/// task results. `none` draws the accent in bold only.
pub const ACCENTS: &[&str] = &[
    "cyan", "azure", "indigo", "violet", "rose", "amber", "steel", "none",
];

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Alias {
    pub name: String,
    pub project: String,
    pub task: String,
    pub args: Vec<String>,
}

/// Version of the config format this kiwa reads and writes.
pub const CONFIG_VERSION: i128 = 1;

/// A named set of roots, selected with `kw -W <name>`.
#[derive(Debug, Default, Clone, PartialEq, serde::Serialize)]
pub struct Workspace {
    pub name: String,
    pub roots: Vec<PathBuf>,
    pub depth: Option<usize>,
    pub ignore: Vec<String>,
}

#[derive(Debug, Default, Clone)]
pub struct Config {
    /// Format version found in the file (`None` when absent: version 1 is assumed).
    pub version: Option<i128>,
    /// `project:task` patterns (`*` and `?` wildcards) hidden from lists and the picker.
    pub hide: Vec<String>,
    /// Words that make a task name risky (`risky`); `None` keeps [`crate::model::RISKY`].
    pub risky: Option<Vec<String>>,
    /// `Some(false)` hides git repositories in which no task was detected.
    pub show_git_only: Option<bool>,
    pub workspaces: Vec<Workspace>,
    pub path: Option<PathBuf>,
    pub roots: Vec<PathBuf>,
    pub depth: Option<usize>,
    pub ignore: Vec<String>,
    /// TUI accent colour name, one of [`ACCENTS`].
    pub accent: Option<String>,
    /// `Some(false)` turns mouse capture off in the TUI.
    pub mouse: Option<bool>,
    pub aliases: Vec<Alias>,
    /// Unknown nodes are reported, not fatal: forward compatibility.
    pub warnings: Vec<String>,
}

/// `$KIWA_CONFIG`, else `<config dir>/kiwa/config.kdl`.
pub fn config_path() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("KIWA_CONFIG") {
        return Some(PathBuf::from(p));
    }
    dirs::config_dir().map(|d| d.join("kiwa").join("config.kdl"))
}

pub fn load() -> Result<Config> {
    let Some(path) = config_path() else {
        return Ok(Config::default());
    };
    if !path.is_file() {
        return Ok(Config {
            path: Some(path),
            ..Default::default()
        });
    }
    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("cannot read {}", path.display()))?;
    let mut cfg = parse(&text).map_err(|e| anyhow!("{}: {e}", path.display()))?;
    cfg.path = Some(path);
    Ok(cfg)
}

fn positional(node: &KdlNode) -> Vec<&KdlValue> {
    node.entries()
        .iter()
        .filter(|e| e.name().is_none())
        .map(KdlEntry::value)
        .collect()
}

fn strip_bom(text: &str) -> &str {
    text.strip_prefix('\u{feff}').unwrap_or(text)
}

fn usize_of(v: Option<&&KdlValue>) -> Option<usize> {
    v.and_then(|v| v.as_integer())
        .and_then(|i| usize::try_from(i).ok())
}

pub fn parse(text: &str) -> Result<Config> {
    let doc: KdlDocument = strip_bom(text)
        .parse()
        .map_err(|e| anyhow!("invalid KDL: {e}"))?;
    let mut cfg = Config::default();
    for node in doc.nodes() {
        let values = positional(node);
        let strings: Vec<String> = values
            .iter()
            .filter_map(|v| v.as_string().map(str::to_owned))
            .collect();
        match node.name().value() {
            "roots" | "root" => cfg.roots.extend(strings.iter().map(|s| expand_home(s))),
            "ignore" => cfg.ignore.extend(strings),
            "accent" => cfg.accent = strings.into_iter().next(),
            "mouse" => cfg.mouse = values.first().and_then(|v| v.as_bool()),
            "depth" => {
                cfg.depth = usize_of(values.first());
            }
            "version" => {
                let v = values.first().and_then(|v| v.as_integer());
                match v {
                    Some(v) if v > CONFIG_VERSION => bail!(
                        "this file uses config format {v}; this kiwa reads up to {CONFIG_VERSION}. Upgrade kiwa"
                    ),
                    Some(v) if v >= 1 => cfg.version = Some(v),
                    _ => bail!("`version` must be a positive integer"),
                }
            }
            "hide" => cfg.hide.extend(strings),
            "risky" => cfg.risky.get_or_insert_with(Vec::new).extend(strings),
            "show-git-only" => cfg.show_git_only = values.first().and_then(|v| v.as_bool()),
            "workspace" => {
                let Some(name) = strings.first().cloned() else {
                    cfg.warnings.push("`workspace` needs a name".to_owned());
                    continue;
                };
                let mut ws = Workspace {
                    name,
                    ..Default::default()
                };
                for child in node.children().map(|d| d.nodes()).unwrap_or_default() {
                    let cv = positional(child);
                    let cs = cv.iter().filter_map(|v| v.as_string().map(str::to_owned));
                    match child.name().value() {
                        "roots" | "root" => ws.roots.extend(cs.map(|s| expand_home(&s))),
                        "ignore" => ws.ignore.extend(cs),
                        "depth" => ws.depth = usize_of(cv.first()),
                        other => cfg.warnings.push(format!(
                            "unknown node `{other}` in workspace `{}` ignored",
                            ws.name
                        )),
                    }
                }
                if ws.roots.is_empty() {
                    cfg.warnings
                        .push(format!("workspace `{}` has no roots", ws.name));
                }
                cfg.workspaces.push(ws);
            }
            "alias" => match strings.as_slice() {
                [name, project, task, args @ ..] => cfg.aliases.push(Alias {
                    name: name.clone(),
                    project: project.clone(),
                    task: task.clone(),
                    args: args.to_vec(),
                }),
                _ => cfg
                    .warnings
                    .push("`alias` needs a name, a project and a task".to_owned()),
            },
            other => cfg
                .warnings
                .push(format!("unknown config node `{other}` ignored")),
        }
    }
    Ok(cfg)
}

/// A folder as people paste or type it, made usable: see [`user_path`].
pub fn expand_home(s: &str) -> PathBuf {
    user_path(s, dirs::home_dir().as_deref(), cfg!(windows))
}

/// Turns pasted or typed text into a path, without a shell. Handles what copying a folder
/// from somewhere else tends to produce:
/// - surrounding whitespace and one pair of matching quotes;
/// - a `file://` URI (dragged from a browser or some terminals), percent-escapes decoded;
/// - outside Windows, backslash-escaped characters (`my\ dir`); on Windows a backslash is
///   a separator and stays;
/// - the home folder written as `~`, `$HOME`, `${HOME}` or `%USERPROFILE%`.
pub fn user_path(text: &str, home: Option<&Path>, windows: bool) -> PathBuf {
    let text = text.trim();
    let text = ['"', '\'']
        .iter()
        .find_map(|&q| text.strip_prefix(q)?.strip_suffix(q))
        .unwrap_or(text);
    let text = match text.strip_prefix("file://") {
        Some(uri) => {
            let path = percent_decoded(uri);
            // `file:///C:/x` names `C:/x`; `file:///home/x` names `/home/x`.
            let drive = path.len() > 2 && path.as_bytes()[2] == b':';
            if drive { path[1..].to_owned() } else { path }
        }
        None if windows => text.to_owned(),
        None => unescaped(text),
    };
    let Some(home) = home else {
        return PathBuf::from(text);
    };
    for name in ["~", "${HOME}", "$HOME", "%USERPROFILE%"] {
        if let Some(rest) = text.strip_prefix(name) {
            if rest.is_empty() {
                return home.to_path_buf();
            }
            if let Some(rest) = rest.strip_prefix(['/', '\\']) {
                return home.join(rest);
            }
        }
    }
    PathBuf::from(text)
}

/// `%20` → ` `; malformed escapes are kept as written.
fn percent_decoded(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = (bytes[i] == b'%')
            .then(|| s.get(i + 1..i + 3))
            .flatten()
            .and_then(|h| u8::from_str_radix(h, 16).ok());
        match hex {
            Some(b) => {
                out.push(b);
                i += 3;
            }
            None => {
                out.push(bytes[i]);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Drops the backslash of `\x` escapes, as a POSIX shell would.
fn unescaped(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        out.push(if c == '\\' {
            chars.next().unwrap_or(c)
        } else {
            c
        });
    }
    out
}

// ------------------------------------------------------------------ editing

/// A config file opened for editing. Saving keeps comments and unrelated nodes.
pub struct Editor {
    path: PathBuf,
    doc: KdlDocument,
}

impl Editor {
    pub fn open(path: &Path) -> Result<Self> {
        let doc = if path.is_file() {
            let text = std::fs::read_to_string(path)?;
            // Refuse to edit a file written for a newer format.
            parse(&text).map_err(|e| anyhow!("{}: {e}", path.display()))?;
            strip_bom(&text)
                .parse()
                .map_err(|e| anyhow!("{}: invalid KDL: {e}", path.display()))?
        } else {
            KdlDocument::new()
        };
        Ok(Self {
            path: path.to_path_buf(),
            doc,
        })
    }

    pub fn open_default() -> Result<Self> {
        let path =
            config_path().ok_or_else(|| anyhow!("no configuration directory on this system"))?;
        Self::open(&path)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Replace every node called `name` by one node with `values`.
    pub fn set(&mut self, name: &str, values: Vec<KdlValue>) {
        let nodes = self.doc.nodes_mut();
        let at = nodes.iter().position(|n| n.name().value() == name);
        nodes.retain(|n| n.name().value() != name);
        let node = make_node(name, values);
        match at {
            Some(i) => nodes.insert(i.min(nodes.len()), node),
            None => nodes.push(node),
        }
    }

    /// Remove nodes called `name` whose first argument is `first` (or all if `None`).
    pub fn remove(&mut self, name: &str, first: Option<&str>) -> usize {
        let nodes = self.doc.nodes_mut();
        let before = nodes.len();
        nodes.retain(|n| {
            n.name().value() != name
                || first
                    .is_some_and(|f| positional(n).first().and_then(|v| v.as_string()) != Some(f))
        });
        before - nodes.len()
    }

    pub fn push(&mut self, name: &str, values: Vec<KdlValue>) {
        self.doc.nodes_mut().push(make_node(name, values));
    }

    /// Add `workspace "name" { roots "…" }`, replacing a workspace of that name.
    pub fn set_workspace(&mut self, name: &str, roots: &[PathBuf]) {
        self.remove("workspace", Some(name));
        let mut node = make_node("workspace", vec![name.into()]);
        let mut children = KdlDocument::new();
        children.nodes_mut().push(make_node(
            "roots",
            roots
                .iter()
                .map(|r| r.display().to_string().into())
                .collect(),
        ));
        node.set_children(children);
        node.autoformat();
        self.doc.nodes_mut().push(node);
    }

    /// Write the file: stamps the format version, validates the result, and keeps the
    /// previous file as `<name>.bak` so a bad edit can always be undone by hand.
    pub fn save(&mut self) -> Result<()> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        if !self
            .doc
            .nodes()
            .iter()
            .any(|n| n.name().value() == "version")
        {
            self.doc.nodes_mut().insert(
                0,
                make_node("version", vec![KdlValue::Integer(CONFIG_VERSION)]),
            );
        }
        let mut text = self.doc.to_string();
        if !text.ends_with('\n') {
            text.push('\n');
        }
        // Validate before writing: never leave a config kiwa itself cannot read.
        parse(&text)?;
        if self.path.is_file() {
            let bak = backup_path(&self.path);
            std::fs::copy(&self.path, &bak)
                .with_context(|| format!("cannot back up to {}", bak.display()))?;
        }
        std::fs::write(&self.path, text)
            .with_context(|| format!("cannot write {}", self.path.display()))
    }
}

/// `config.kdl` → `config.kdl.bak`.
pub fn backup_path(path: &Path) -> PathBuf {
    let mut p = path.as_os_str().to_owned();
    p.push(".bak");
    PathBuf::from(p)
}

/// `*` matches any run of characters, `?` one character; case-insensitive.
pub fn glob(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.to_lowercase().chars().collect();
    let t: Vec<char> = text.to_lowercase().chars().collect();
    let (mut pi, mut ti, mut star, mut mark) = (0, 0, None, 0);
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = ti;
            pi += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|c| *c == '*')
}

impl Config {
    /// Words that make a task name risky: the config's `risky` list, or the defaults.
    pub fn risky_words(&self) -> Vec<&str> {
        match &self.risky {
            Some(v) => v.iter().map(String::as_str).collect(),
            None => crate::model::RISKY.to_vec(),
        }
    }

    /// Whether `project:task` (or `runner:task`) is hidden by a `hide` pattern.
    pub fn hides(&self, project_id: &str, runner: &str, task: &str) -> bool {
        self.hide.iter().any(|h| {
            glob(h, &format!("{project_id}:{task}")) || glob(h, &format!("{runner}:{task}"))
        })
    }

    pub fn workspace(&self, name: &str) -> Option<&Workspace> {
        self.workspaces.iter().find(|w| w.name == name)
    }

    /// The current value of a setting, as text (for `kw config get`).
    pub fn get(&self, key: &str) -> Result<serde_json::Value> {
        use serde_json::json;
        Ok(match key {
            "roots" => json!(self.roots),
            "depth" => json!(self.depth.unwrap_or(4)),
            "ignore" => json!(self.ignore),
            "accent" => json!(self.accent.as_deref().unwrap_or("cyan")),
            "mouse" => json!(self.mouse.unwrap_or(true)),
            "hide" => json!(self.hide),
            "risky" => json!(self.risky_words()),
            "show-git-only" => json!(self.show_git_only.unwrap_or(true)),
            "version" => json!(self.version.unwrap_or(CONFIG_VERSION)),
            "workspaces" => json!(self.workspaces),
            "aliases" => json!(self.aliases),
            _ => bail!(
                "unknown setting `{key}` (roots, depth, ignore, accent, mouse, hide, risky, show-git-only, version, workspaces, aliases)"
            ),
        })
    }
}

fn make_node(name: &str, values: Vec<KdlValue>) -> KdlNode {
    let mut node = KdlNode::new(name);
    for v in values {
        node.push(KdlEntry::new(v));
    }
    node
}

/// Validate and convert a `kw config set <key> <values...>` request.
pub fn setting(key: &str, values: &[String]) -> Result<Vec<KdlValue>> {
    let one = || -> Result<&String> {
        match values {
            [v] => Ok(v),
            _ => bail!("`{key}` takes exactly one value"),
        }
    };
    Ok(match key {
        "roots" => {
            if values.is_empty() {
                bail!("`roots` needs at least one folder");
            }
            values.iter().map(|v| KdlValue::String(v.clone())).collect()
        }
        "ignore" => values.iter().map(|v| KdlValue::String(v.clone())).collect(),
        "depth" => {
            let n: i128 = one()?
                .parse()
                .map_err(|_| anyhow!("`depth` must be a number"))?;
            if !(0..=32).contains(&n) {
                bail!("`depth` must be between 0 and 32");
            }
            vec![KdlValue::Integer(n)]
        }
        "accent" => {
            let v = one()?;
            if !ACCENTS.contains(&v.as_str()) {
                bail!("`accent` must be one of: {}", ACCENTS.join(", "));
            }
            vec![KdlValue::String(v.clone())]
        }
        "mouse" => {
            let b = match one()?.as_str() {
                "on" | "true" | "yes" => true,
                "off" | "false" | "no" => false,
                _ => bail!("`mouse` must be on or off"),
            };
            vec![KdlValue::Bool(b)]
        }
        "hide" | "risky" => values.iter().map(|v| KdlValue::String(v.clone())).collect(),
        "show-git-only" => {
            let b = match one()?.as_str() {
                "on" | "true" | "yes" => true,
                "off" | "false" | "no" => false,
                _ => bail!("`show-git-only` must be on or off"),
            };
            vec![KdlValue::Bool(b)]
        }
        _ => bail!(
            "unknown setting `{key}` (roots, depth, ignore, accent, mouse, hide, risky, show-git-only)"
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn editor_adds_a_workspace_that_reads_back() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("config.kdl");
        std::fs::write(&path, "// mine\nroots \"/a\"\n").unwrap();
        let mut e = Editor::open(&path).unwrap();
        e.set_workspace("work", &[PathBuf::from("/w/one")]);
        e.set_workspace("work", &[PathBuf::from("/w/two")]);
        e.save().unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("// mine"), "{text}");
        let c = parse(&text).unwrap();
        assert_eq!(c.workspaces.len(), 1, "replaced, not duplicated: {text}");
        assert_eq!(c.workspaces[0].roots, [PathBuf::from("/w/two")]);
    }

    #[test]
    fn parses_kdl() {
        let c = parse(
            "roots \"/a\" \"/b\"\ndepth 3\nignore \"tmp\"\ncolour \"x\"\nmouse #false\nalias \"t\" \"api\" \"test\" \"-v\"\n",
        )
        .unwrap();
        assert_eq!(c.roots.len(), 2);
        assert_eq!(c.depth, Some(3));
        assert_eq!(c.ignore, ["tmp"]);
        assert_eq!(c.mouse, Some(false));
        assert_eq!(c.aliases[0].args, ["-v"]);
        assert_eq!(c.warnings.len(), 1);
    }

    #[test]
    fn invalid_alias_warns() {
        let c = parse("alias \"t\" \"api\"\n").unwrap();
        assert!(c.aliases.is_empty());
        assert_eq!(c.warnings.len(), 1);
    }

    #[test]
    fn invalid_kdl_is_an_error() {
        assert!(parse("roots \"unclosed\n").is_err());
    }

    #[test]
    fn editor_preserves_comments_and_replaces_values() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.kdl");
        std::fs::write(&path, "// my roots\nroots \"/a\"\naccent \"red\" // fav\n").unwrap();
        let mut e = Editor::open(&path).unwrap();
        e.set("accent", setting("accent", &["amber".into()]).unwrap());
        e.set("mouse", setting("mouse", &["off".into()]).unwrap());
        e.push("alias", vec!["t".into(), "api".into(), "test".into()]);
        e.save().unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("// my roots"), "{text}");
        let c = parse(&text).unwrap();
        assert_eq!(c.accent.as_deref(), Some("amber"));
        assert_eq!(c.mouse, Some(false));
        assert_eq!(c.aliases.len(), 1);
        let mut e = Editor::open(&path).unwrap();
        assert_eq!(e.remove("alias", Some("t")), 1);
        e.save().unwrap();
        assert!(
            parse(&std::fs::read_to_string(&path).unwrap())
                .unwrap()
                .aliases
                .is_empty()
        );
    }

    #[test]
    fn settings_are_validated() {
        assert!(
            setting("accent", &["green".into()]).is_err(),
            "status colours are not accents"
        );
        assert!(setting("depth", &["99".into()]).is_err());
        assert!(setting("mouse", &["maybe".into()]).is_err());
        assert!(setting("nope", &["x".into()]).is_err());
        assert!(setting("roots", &[]).is_err());
    }

    #[test]
    fn pasted_paths_are_cleaned_up() {
        let home = Path::new("/home/me");
        let unix = |s: &str| user_path(s, Some(home), false);
        let win = |s: &str| user_path(s, Some(home), true);
        assert_eq!(unix("  \"/x y\" "), PathBuf::from("/x y"));
        assert_eq!(unix("'/x'"), PathBuf::from("/x"));
        assert_eq!(unix("\"/x'"), PathBuf::from("\"/x'"), "quotes must match");
        assert_eq!(unix("/my\\ dir"), PathBuf::from("/my dir"));
        assert_eq!(
            win("C:\\my dir"),
            PathBuf::from("C:\\my dir"),
            "a separator on Windows"
        );
        assert_eq!(unix("file:///srv/my%20code"), PathBuf::from("/srv/my code"));
        assert_eq!(win("file:///C:/dev/a%2"), PathBuf::from("C:/dev/a%2"));
        for typed in ["~/dev", "$HOME/dev", "${HOME}/dev"] {
            assert_eq!(unix(typed), home.join("dev"), "{typed}");
        }
        assert_eq!(win("%USERPROFILE%\\dev"), home.join("dev"));
        assert_eq!(unix("~"), home);
        assert_eq!(
            unix("~bob/x"),
            PathBuf::from("~bob/x"),
            "other users are not guessed"
        );
        assert_eq!(user_path("~/x", None, false), PathBuf::from("~/x"));
    }
}
