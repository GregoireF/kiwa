//! Interactive picker: one fuzzy search across every project *and* task.
//! Every action has a key, every key is listed in the footer, and the footer entries that act
//! on the list are clickable.
//!
//! Layout (top to bottom):
//!    kiwa   ~/dev / scope            14 projects · 56 tasks · scanned in 17 ms   Folders  Quit
//!   ─────────────────────────────────────────────────────────────────────────────────────────
//!    ❯ query                                                    ✔ api test · ok · 1.2s
//!
//!     recent ──────────────────────────────────────────── │  api-gateway
//!   ● api-gateway  test    cargo                ✔ 2 min ago ┃  test  cargo
//!     all tasks ───────────────────────────────────────── ┃
//!     billing      build   go                             │  $ cargo test
//!    ↑↓ move · ⏎ run · → args · tab buttons · esc quit
//!
//! The column between the list and the details is a scrollbar when the list overflows.
//! While browsing, the packages of a monorepo (projects nested in another project's folder) are
//! listed under their root, indented, in a group that ← folds and → unfolds:
//!   ● ▾ shop          dev    npm
//!         apps/admin  dev    npm
//!     ▸ tools +4      lint   make

use crate::exec::{self, display_argv};
use crate::theme::{Theme, time_ago, truncate_middle};
use kiwa_core::discover::natural_cmp;
use kiwa_core::state::State;
use kiwa_core::{Project, fuzzy};
use ratatui::crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, KeyModifiers,
    MouseButton, MouseEventKind,
};
use ratatui::crossterm::execute;
use ratatui::crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, ListState, Padding, Paragraph, Wrap};
use ratatui::{DefaultTerminal, Frame};
use std::collections::{HashMap, HashSet};
use std::io::{IsTerminal, Write};
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// When the picker opens, clicks only count once the pointer has moved or a key was pressed:
/// the press that opened it (or the second half of a double-click) must not act on it.
/// Terminals that never report motion get this delay instead.
const ARM_FALLBACK: Duration = Duration::from_millis(600);
/// At most this many rows in the recent group…
const RECENT_MAX: usize = 6;
/// …and only tasks run within this many seconds (14 days): old runs do not crowd the top.
const RECENT_WINDOW: u64 = 14 * 24 * 3600;
/// Most keys (typed characters + ↓) to reach any task of the polyglot test workspace.
#[cfg(test)]
const KEYSTROKE_BUDGET: usize = 5;

pub struct Options {
    pub accent: Option<String>,
    pub mouse: bool,
    /// Shown in the footer (e.g. why the mouse is off).
    pub note: Option<&'static str>,
    /// Limit the picker to one project (`kw <project>`).
    pub scope: Option<PathBuf>,
    /// Scanned roots, shown in the header.
    pub roots: Vec<PathBuf>,
    /// Workspaces offered behind `Folders`: `default` first, then the named ones. With more
    /// than one, `projects` holds all of them and the picker filters.
    pub spaces: Vec<Space>,
    /// Workspace shown first (`None`: all of them).
    pub space: Option<usize>,
    /// Home folder, for `~` in a folder typed into "add a folder".
    pub home: Option<PathBuf>,
}

/// A workspace: a name and its folders.
#[derive(Debug, Clone, PartialEq)]
pub struct Space {
    pub name: String,
    pub roots: Vec<PathBuf>,
}

/// How the picker ended.
#[derive(Debug, PartialEq)]
pub enum Outcome {
    /// Quit, with the exit code of the last task run (0 if none).
    Exit(i32),
    /// The setup was asked for (`kw init` from the switcher): run it, then open the picker again.
    Folders(i32),
    /// "Add a folder" in the switcher: save it as a workspace, then open the picker on it.
    AddFolder(PathBuf, i32),
}

/// An entry of the workspace switcher.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Switch {
    All,
    Space(usize),
    Add,
    Setup,
}

/// One task of a project, or a project without tasks (`t == None`).
struct Item {
    p: usize,
    t: Option<usize>,
    hay: String,
    /// Characters of the project id at the start of `hay` (task name starts after a space).
    id_len: usize,
}

enum Mode {
    Search,
    /// Typing arguments for the selected task.
    Args(String),
    /// The workspace switcher, with the highlighted entry.
    Spaces(usize),
    /// Typing a folder to add as a workspace.
    AddFolder(String),
    /// A shell line or a risky task, shown and waiting for `y` (project, task, arguments).
    Confirm(usize, usize, Vec<String>),
}

#[derive(Clone, Copy)]
struct LastRun {
    exit_code: Option<i32>,
    last: u64,
    count: u32,
}

/// A line of the list: a group title or a match (index into `App::matches`).
#[derive(Clone, Copy, Debug, PartialEq)]
enum Entry {
    Header(&'static str),
    Item(usize),
}

/// Anything clickable on screen.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Button {
    Folders,
    Quit,
    /// Footer: run the selected task.
    Run,
    /// Footer: run the selected task with arguments.
    Args,
    /// Footer: what Escape does (clear, leave the scope, quit).
    Back,
    PageUp,
    PageDown,
    /// The `▾`/`▸` marker of a monorepo group (root project index).
    Fold(usize),
    /// An entry of the workspace switcher (index into `App::switcher()`).
    Switch(usize),
    /// Footer: open the selected task's URL.
    Open,
}

/// Where a project sits in its monorepo. A project's parent is the root that declares it as a
/// member (`member_of`); failing that, the nearest project whose folder holds it, unless that
/// project declares its members and left this one out. The top of the chain is its root.
struct Node {
    root: usize,
    /// Number of projects above it.
    depth: usize,
    /// Path below the root (`apps/admin`), or the project id for a root.
    rel: String,
    /// Packages below it (roots only).
    members: usize,
}

/// Monorepo structure, from `member_of` and the project folders (no manifest is read again).
fn tree_of(projects: &[Project]) -> Vec<Node> {
    let index: HashMap<&std::path::Path, usize> = projects
        .iter()
        .enumerate()
        .map(|(i, p)| (p.path.as_path(), i))
        .collect();
    let declaring: HashSet<&std::path::Path> = projects
        .iter()
        .filter_map(|p| p.member_of.as_ref().map(|m| m.path.as_path()))
        .collect();
    let parent: Vec<Option<usize>> = projects
        .iter()
        .enumerate()
        .map(|(i, p)| {
            if let Some(&r) = p
                .member_of
                .as_ref()
                .and_then(|m| index.get(m.path.as_path()))
            {
                return Some(r);
            }
            let near = (0..projects.len())
                .filter(|&j| {
                    j != i && projects[j].path != p.path && p.path.starts_with(&projects[j].path)
                })
                .max_by_key(|&j| projects[j].path.components().count())?;
            (!declaring.contains(projects[near].path.as_path())).then_some(near)
        })
        .collect();
    let mut nodes: Vec<Node> = (0..projects.len())
        .map(|i| {
            let (mut root, mut depth) = (i, 0);
            // Bounded walk: a malformed chain can never loop.
            while let Some(up) = parent[root].filter(|_| depth < projects.len()) {
                root = up;
                depth += 1;
            }
            let p = &projects[i];
            let rel = match p.path.strip_prefix(&projects[root].path) {
                Ok(r) if root != i => r
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy())
                    .collect::<Vec<_>>()
                    .join("/"),
                _ => p.id.clone(),
            };
            Node {
                root,
                depth,
                rel,
                members: 0,
            }
        })
        .collect();
    for i in 0..nodes.len() {
        let r = nodes[i].root;
        if r != i {
            nodes[r].members += 1;
        }
    }
    nodes
}

/// `/`-separated paths compared folder by folder, so a folder's content follows it directly.
fn tree_path_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    let (mut x, mut y) = (a.split('/'), b.split('/'));
    loop {
        match (x.next(), y.next()) {
            (None, None) => return std::cmp::Ordering::Equal,
            (None, _) => return std::cmp::Ordering::Less,
            (_, None) => return std::cmp::Ordering::Greater,
            (Some(c), Some(d)) => match natural_cmp(c, d) {
                std::cmp::Ordering::Equal => {}
                o => return o,
            },
        }
    }
}

/// Buttons in the header, in focus order.
const TOOLBAR: [Button; 2] = [Button::Folders, Button::Quit];

/// What the last drawing put where, for mapping the mouse back.
#[derive(Default)]
struct View {
    /// Screen row → match index.
    rows: Vec<(u16, usize)>,
    hits: Vec<(Rect, Button)>,
    x: u16,
    w: u16,
}

struct App<'a> {
    projects: &'a [Project],
    items: Vec<Item>,
    matches: Vec<usize>,
    query: String,
    list: ListState,
    view: View,
    hover: Option<usize>,
    hover_button: Option<Button>,
    /// Keyboard focus on a header button (index into `TOOLBAR`).
    focus: Option<usize>,
    scan: Duration,
    status: Option<(bool, String)>,
    theme: Theme,
    scope: Option<usize>,
    mode: Mode,
    mouse: bool,
    note: Option<&'static str>,
    roots: String,
    runs: HashMap<(PathBuf, String), LastRun>,
    now: u64,
    /// Clicks are accepted (see `ARM_FALLBACK`).
    armed: bool,
    opened: Instant,
    /// Entry scroll offset.
    offset: usize,
    /// Monorepo structure, one node per project.
    tree: Vec<Node>,
    /// At least one project has packages: browsing shows the list as a tree.
    grouped: bool,
    /// Folded monorepo groups (root project indices).
    folded: HashSet<usize>,
    /// Rows hidden by each folded group.
    hidden: HashMap<usize, usize>,
    /// Matches at the top shown in the RECENT group.
    recent: usize,
    spaces: Vec<Space>,
    /// Workspace in view (`None`: all).
    space: Option<usize>,
    /// Workspaces each project belongs to (indices into `spaces`).
    tags: Vec<Vec<usize>>,
    /// Folders of each workspace that do not exist.
    missing: Vec<Vec<PathBuf>>,
    /// Projects and tasks in the workspace in view, for the header.
    counts: (usize, usize),
    home: Option<PathBuf>,
    /// Project cards, read when a project is first shown in the details.
    abouts: std::cell::RefCell<HashMap<usize, kiwa_core::about::About>>,
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

impl<'a> App<'a> {
    fn new(projects: &'a [Project], state: &State, scan: Duration, opts: &Options) -> Self {
        let mut items: Vec<(u32, Item)> = Vec::new();
        for (pi, p) in projects.iter().enumerate() {
            let id_len = p.id.chars().count();
            if p.tasks.is_empty() {
                items.push((
                    0,
                    Item {
                        p: pi,
                        t: None,
                        hay: p.id.clone(),
                        id_len,
                    },
                ));
            }
            for (ti, t) in p.tasks.iter().enumerate() {
                let hay = format!("{} {} {}", p.id, t.name, t.runner);
                items.push((
                    state.score(&p.path, &t.name),
                    Item {
                        p: pi,
                        t: Some(ti),
                        hay,
                        id_len,
                    },
                ));
            }
        }
        // Recently and frequently used first; projects without tasks last; stable otherwise.
        items.sort_by_key(|(score, i)| (std::cmp::Reverse(*score), i.t.is_none()));
        let scope = opts
            .scope
            .as_ref()
            .and_then(|s| projects.iter().position(|p| &p.path == s));
        let runs = state
            .history
            .iter()
            .map(|e| {
                let run = LastRun {
                    exit_code: e.exit_code,
                    last: e.last,
                    count: e.count,
                };
                ((e.path.clone(), e.task.clone()), run)
            })
            .collect();
        let roots = opts
            .roots
            .iter()
            .map(|r| r.display().to_string())
            .collect::<Vec<_>>()
            .join(" · ");
        let tree = tree_of(projects);
        let grouped = tree.iter().any(|n| n.members > 0);
        let tags = projects
            .iter()
            .map(|p| {
                (0..opts.spaces.len())
                    .filter(|&i| opts.spaces[i].roots.iter().any(|r| p.path.starts_with(r)))
                    .collect()
            })
            .collect();
        let missing = opts
            .spaces
            .iter()
            .map(|s| s.roots.iter().filter(|r| !r.is_dir()).cloned().collect())
            .collect();
        let mut app = Self {
            projects,
            items: items.into_iter().map(|(_, i)| i).collect(),
            matches: Vec::new(),
            query: String::new(),
            list: ListState::default(),
            view: View::default(),
            hover: None,
            hover_button: None,
            focus: None,
            scan,
            status: None,
            theme: Theme::from_env(opts.accent.as_deref()),
            scope,
            mode: Mode::Search,
            mouse: opts.mouse,
            note: opts.note,
            roots,
            runs,
            now: now(),
            armed: true,
            opened: Instant::now(),
            offset: 0,
            tree,
            grouped,
            folded: HashSet::new(),
            hidden: HashMap::new(),
            recent: 0,
            spaces: opts.spaces.clone(),
            space: opts.space.filter(|&s| s < opts.spaces.len()),
            tags,
            missing,
            counts: (0, 0),
            home: opts.home.clone(),
            abouts: Default::default(),
        };
        app.refilter();
        app.select_last_run_in_scope();
        app
    }

    /// `@name` at the start of the search: the name typed so far.
    fn at_name(&self) -> Option<&str> {
        self.query
            .strip_prefix('@')
            .map(|r| r.split(' ').next().unwrap_or(""))
    }

    /// The search without its `@name`.
    fn search(&self) -> &str {
        match self.query.strip_prefix('@') {
            Some(rest) => rest.split_once(' ').map_or("", |(_, s)| s.trim_start()),
            None => &self.query,
        }
    }

    /// Workspace in view: `@name` (exact, then a unique prefix, `@all` for every one) or the
    /// one chosen in the switcher. `Err` for an `@name` that matches no workspace.
    fn active_space(&self) -> Result<Option<usize>, ()> {
        let Some(name) = self.at_name().filter(|n| !n.is_empty()) else {
            return Ok(self.space);
        };
        let name = name.to_lowercase();
        if name == "all" {
            return Ok(None);
        }
        let lower: Vec<String> = self.spaces.iter().map(|s| s.name.to_lowercase()).collect();
        if let Some(i) = lower.iter().position(|s| *s == name) {
            return Ok(Some(i));
        }
        let starts: Vec<usize> = (0..lower.len())
            .filter(|&i| lower[i].starts_with(&name))
            .collect();
        match starts.as_slice() {
            [i] => Ok(Some(*i)),
            _ => Err(()),
        }
    }

    fn refilter(&mut self) {
        let space = self.active_space();
        let in_space = |p: usize| match space {
            Ok(None) => true,
            Ok(Some(s)) => self.tags.get(p).is_none_or(|t| t.contains(&s)),
            Err(()) => false,
        };
        let spaced: Vec<usize> = (0..self.items.len())
            .filter(|&i| in_space(self.items[i].p))
            .collect();
        let shown: HashSet<usize> = spaced.iter().map(|&i| self.items[i].p).collect();
        self.counts = (
            shown.len(),
            spaced
                .iter()
                .filter(|&&i| self.items[i].t.is_some())
                .count(),
        );
        let pool: Vec<usize> = spaced
            .into_iter()
            .filter(|&i| self.scope.is_none_or(|s| self.items[i].p == s))
            .collect();
        let hay: Vec<&str> = pool.iter().map(|&i| self.items[i].hay.as_str()).collect();
        self.matches = fuzzy::rank(self.search(), &hay)
            .into_iter()
            .map(|k| pool[k])
            .collect();
        self.recent = if self.browsing() {
            self.matches
                .iter()
                .take_while(|&&i| {
                    self.last_run(&self.items[i])
                        .is_some_and(|r| self.now.saturating_sub(r.last) <= RECENT_WINDOW)
                })
                .count()
                .min(RECENT_MAX)
        } else {
            0
        };
        self.hidden.clear();
        if self.tree_mode() {
            self.regroup();
        }
        self.list.select(if self.matches.is_empty() {
            None
        } else {
            Some(0)
        });
        self.offset = 0;
        self.hover = None;
    }

    /// No search and no project scope: the list shows groups.
    fn browsing(&self) -> bool {
        self.search().is_empty() && self.scope.is_none()
    }

    /// The switcher's entries: `all` when there are several workspaces, each workspace, then
    /// "add a folder" and the setup.
    fn switcher(&self) -> Vec<Switch> {
        let mut v = Vec::new();
        if self.spaces.len() > 1 {
            v.push(Switch::All);
        }
        v.extend((0..self.spaces.len()).map(Switch::Space));
        v.push(Switch::Add);
        v.push(Switch::Setup);
        v
    }

    /// Projects in a workspace (`None`: all).
    fn projects_in(&self, space: Option<usize>) -> usize {
        match space {
            None => self.projects.len(),
            Some(s) => self.tags.iter().filter(|t| t.contains(&s)).count(),
        }
    }

    /// Open the switcher on the workspace in view.
    fn open_switcher(&mut self) {
        let current = match self.space {
            None => Switch::All,
            Some(s) => Switch::Space(s),
        };
        let at = self
            .switcher()
            .iter()
            .position(|e| *e == current)
            .unwrap_or(0);
        self.focus = None;
        self.mode = Mode::Spaces(at);
    }

    /// Act on a switcher entry.
    fn choose(&mut self, entry: Switch) -> Option<Action> {
        match entry {
            Switch::All | Switch::Space(_) => {
                self.space = match entry {
                    Switch::Space(s) => Some(s),
                    _ => None,
                };
                // A chosen workspace replaces an `@name` typed in the search.
                if self.query.starts_with('@') {
                    self.query = self.search().to_owned();
                }
                self.scope = None;
                self.mode = Mode::Search;
                self.refilter();
                None
            }
            Switch::Add => {
                self.mode = Mode::AddFolder(String::new());
                None
            }
            Switch::Setup => Some(Action::Folders),
        }
    }

    /// Where the list comes from, for the header: the workspace and its folders.
    fn place(&self) -> String {
        if self.spaces.len() < 2 {
            return self.roots.clone();
        }
        match self.active_space() {
            Ok(Some(s)) => {
                let roots: Vec<String> = self.spaces[s]
                    .roots
                    .iter()
                    .map(|r| r.display().to_string())
                    .collect();
                format!("{} · {}", self.spaces[s].name, roots.join(" · "))
            }
            Ok(None) => "all workspaces".into(),
            Err(()) => format!("no workspace “@{}”", self.at_name().unwrap_or_default()),
        }
    }

    /// Browsing a workspace that has monorepos: the rows below RECENT form a tree.
    fn tree_mode(&self) -> bool {
        self.grouped && self.browsing()
    }

    fn root_of(&self, item: usize) -> usize {
        self.tree[self.items[item].p].root
    }

    /// Put each package right below its root (the group keeps the place of its first row),
    /// then drop the rows of folded groups but the first.
    fn regroup(&mut self) {
        let mut rest = self.matches.split_off(self.recent);
        let mut first: HashMap<usize, usize> = HashMap::new();
        for (k, &i) in rest.iter().enumerate() {
            first.entry(self.root_of(i)).or_insert(k);
        }
        rest.sort_by(|&a, &b| {
            let (pa, pb) = (self.items[a].p, self.items[b].p);
            let (na, nb) = (&self.tree[pa], &self.tree[pb]);
            first[&na.root]
                .cmp(&first[&nb.root])
                .then_with(|| (pa != na.root).cmp(&(pb != nb.root)))
                .then_with(|| tree_path_cmp(&na.rel, &nb.rel))
        });
        let mut prev = None;
        for i in rest {
            let r = self.root_of(i);
            let first_row = prev != Some(r);
            prev = Some(r);
            if !first_row && self.folded.contains(&r) {
                *self.hidden.entry(r).or_default() += 1;
            } else {
                self.matches.push(i);
            }
        }
    }

    /// Whether match `k` opens its monorepo group in the tree.
    fn opens_group(&self, k: usize) -> bool {
        k == self.recent || self.root_of(self.matches[k - 1]) != self.root_of(self.matches[k])
    }

    /// The monorepo group of the selected row, with the position of its first row, when the
    /// list is a tree.
    fn selected_group(&self) -> Option<(usize, usize)> {
        let k = self.list.selected()?;
        if !self.tree_mode() || k < self.recent {
            return None;
        }
        let root = self.root_of(self.matches[k]);
        if self.tree[root].members == 0 {
            return None;
        }
        let mut first = k;
        while !self.opens_group(first) {
            first -= 1;
        }
        Some((root, first))
    }

    fn can_fold(&self) -> bool {
        self.selected_group()
            .is_some_and(|(r, _)| !self.folded.contains(&r))
    }

    fn can_unfold(&self) -> bool {
        self.selected_group().is_some_and(|(r, first)| {
            self.folded.contains(&r) && self.list.selected() == Some(first)
        })
    }

    /// Fold or unfold a monorepo group; its first row ends up selected.
    fn toggle_fold(&mut self, root: usize) {
        let anchor = (self.recent..self.matches.len())
            .find(|&k| self.root_of(self.matches[k]) == root)
            .map(|k| self.matches[k]);
        if !self.folded.remove(&root) {
            self.folded.insert(root);
        }
        let offset = self.offset;
        self.refilter();
        if let Some(k) = anchor.and_then(|a| self.matches.iter().position(|&i| i == a)) {
            self.list.select(Some(k));
        }
        self.offset = offset;
    }

    /// The project column of match `k`: a fold marker and indentation in the tree, the name,
    /// and a count of folded rows. Outside the tree, just the project id.
    fn id_label(&self, k: usize) -> (String, String, String, Option<usize>) {
        let it = &self.items[self.matches[k]];
        let p = &self.projects[it.p];
        if !self.tree_mode() {
            return (String::new(), p.id.clone(), String::new(), None);
        }
        if k < self.recent {
            // Aligned with the tree below, past its fold markers.
            return ("  ".into(), p.id.clone(), String::new(), None);
        }
        let node = &self.tree[it.p];
        let root = node.root;
        let opens = self.opens_group(k);
        let group = self.tree[root].members > 0;
        let folded = self.folded.contains(&root);
        let marker = match (group && opens, folded) {
            (false, _) => "  ",
            (true, false) => "▾ ",
            (true, true) => "▸ ",
        };
        // A group whose root rows are all in RECENT opens on a package: show its full id.
        let (indent, name) = if opens && node.depth > 0 {
            (0, p.id.clone())
        } else {
            (node.depth, node.rel.clone())
        };
        let more = match self.hidden.get(&root) {
            Some(n) if opens && folded => format!(" +{n}"),
            _ => String::new(),
        };
        let button = (group && opens).then_some(root);
        (
            format!("{marker}{}", "  ".repeat(indent)),
            name,
            more,
            button,
        )
    }

    /// In a project-scoped picker, start on the task that was run last (as a project's script
    /// list remembers its last script).
    fn select_last_run_in_scope(&mut self) {
        if self.scope.is_none() {
            return;
        }
        let best = (0..self.matches.len())
            .filter_map(|k| {
                self.last_run(&self.items[self.matches[k]])
                    .map(|r| (r.last, k))
            })
            .max();
        if let Some((_, k)) = best {
            self.list.select(Some(k));
        }
    }

    fn selected(&self) -> Option<&Item> {
        self.list
            .selected()
            .and_then(|i| self.matches.get(i))
            .map(|&i| &self.items[i])
    }

    fn step(&mut self, delta: isize) {
        if self.matches.is_empty() {
            return;
        }
        let n = self.matches.len() as isize;
        let cur = self.list.selected().unwrap_or(0) as isize;
        self.list
            .select(Some((cur + delta).clamp(0, n - 1) as usize));
    }

    /// One row up or down, wrapping around at both ends.
    fn step_around(&mut self, delta: isize) {
        let n = self.matches.len() as isize;
        if n == 0 {
            return;
        }
        let cur = self.list.selected().unwrap_or(0) as isize;
        self.list.select(Some((cur + delta).rem_euclid(n) as usize));
    }

    fn last_run(&self, it: &Item) -> Option<LastRun> {
        let p = &self.projects[it.p];
        let t = &p.tasks[it.t?];
        self.runs.get(&(p.path.clone(), t.name.clone())).copied()
    }

    /// The list with its group titles. Groups (recent, all tasks) only when browsing:
    /// a search shows the best matches first, without titles.
    fn entries(&self) -> Vec<Entry> {
        let n = self.matches.len();
        let recent = self.recent;
        if recent == 0 || recent == n {
            let mut v: Vec<Entry> = Vec::with_capacity(n + 1);
            if recent > 0 {
                v.push(Entry::Header("recent"));
            }
            v.extend((0..n).map(Entry::Item));
            return v;
        }
        let mut v = vec![Entry::Header("recent")];
        v.extend((0..recent).map(Entry::Item));
        v.push(Entry::Header("all tasks"));
        v.extend((recent..n).map(Entry::Item));
        v
    }

    fn row_at(&self, row: u16) -> Option<usize> {
        self.view
            .rows
            .iter()
            .find(|(y, _)| *y == row)
            .map(|(_, k)| *k)
    }

    fn button_at(&self, column: u16, row: u16) -> Option<Button> {
        self.view
            .hits
            .iter()
            .find(|(r, _)| {
                column >= r.x && column < r.x + r.width && row >= r.y && row < r.y + r.height
            })
            .map(|(_, b)| *b)
    }

    fn esc_label(&self) -> &'static str {
        if !self.query.is_empty() {
            "clear"
        } else if self.scope.is_some() {
            "all projects"
        } else {
            "quit"
        }
    }
}

enum Action {
    Quit,
    Folders,
    AddFolder(PathBuf),
    /// Open a URL in the browser and stay in the picker.
    Open(String),
    Run(usize, usize, Vec<String>),
}

// ------------------------------------------------------------------ run loop

pub fn run(
    projects: &[Project],
    state: &mut State,
    scan: Duration,
    opts: Options,
) -> std::io::Result<Outcome> {
    let mut app = App::new(projects, state, scan, &opts);
    let mut last_code = 0;
    let mut again: Option<(usize, usize, Vec<String>)> = None;
    loop {
        let (pi, ti, args) = match again.take() {
            Some(sel) => sel,
            None => {
                let mut term = ratatui::try_init()?;
                if app.mouse {
                    let _ = execute!(std::io::stdout(), EnableMouseCapture);
                }
                app.armed = false;
                app.opened = Instant::now();
                let action = picker(&mut term, &mut app);
                if app.mouse {
                    let _ = execute!(std::io::stdout(), DisableMouseCapture);
                }
                ratatui::restore();
                match action? {
                    Action::Run(pi, ti, args) => (pi, ti, args),
                    Action::Quit => return Ok(Outcome::Exit(last_code)),
                    Action::Folders => return Ok(Outcome::Folders(last_code)),
                    Action::AddFolder(dir) => return Ok(Outcome::AddFolder(dir, last_code)),
                    Action::Open(_) => unreachable!("the picker loop opens URLs itself"),
                }
            }
        };
        let (p, t) = (&projects[pi], &projects[pi].tasks[ti]);
        let th = app.theme;
        let ansi = Ansi::new(&th);
        eprintln!(
            "{}{}{}   {}",
            ansi.paint(&p.id, th.bold),
            ansi.paint(" · ", th.dim),
            ansi.paint(&t.name, th.accent),
            ansi.paint(&format!("$ {}", exec::shown(t, &args)), th.dim)
        );
        let last = |path: &std::path::Path, task: &str| {
            app.runs
                .get(&(path.to_path_buf(), task.to_owned()))
                .map(|r| r.last)
        };
        for note in exec::url_notes(t, projects, last) {
            let style = if note.starts_with('⚠') {
                th.warn
            } else {
                th.accent
            };
            eprintln!("{}", ansi.paint(&note, style));
        }
        eprintln!();
        let msg = match exec::run(p, t, &args, false) {
            Ok(o) => {
                last_code = o.code;
                state.record(&p.path, &t.name, Some(o.code));
                state.save();
                let key = (p.path.clone(), t.name.clone());
                let count = app.runs.get(&key).map_or(1, |r| r.count + 1);
                app.runs.insert(
                    key,
                    LastRun {
                        exit_code: Some(o.code),
                        last: now(),
                        count,
                    },
                );
                app.now = now();
                let label = format!("{} {}", p.id, t.name);
                let (ok, line) = result_line(&label, o.code, o.duration.as_secs_f64());
                eprintln!(
                    "\n{}",
                    ansi.paint(&ruled(&line), if ok { th.ok } else { th.err })
                );
                (ok, line)
            }
            Err(f) => {
                last_code = f.code;
                eprintln!("{}", ansi.paint(&format!("✘ {}", f.message), th.err));
                for s in &f.suggestions {
                    eprintln!("  → {s}");
                }
                (false, format!("✘ {}", f.message))
            }
        };
        app.status = Some(msg);
        eprint!(
            "\n  {} {}   {} {}   {} {} ",
            ansi.paint("⏎", th.bold),
            ansi.paint("back to kiwa", th.dim),
            ansi.paint("r", th.bold),
            ansi.paint("run again", th.dim),
            ansi.paint("q", th.bold),
            ansi.paint("quit", th.dim),
        );
        match after_run()? {
            After::Back => {}
            // A shell line or a risky task is asked again, in the picker.
            After::Again if exec::confirmation(t).is_some() => {
                app.mode = Mode::Confirm(pi, ti, args);
            }
            After::Again => again = Some((pi, ti, args)),
            After::Quit => return Ok(Outcome::Exit(last_code)),
        }
    }
}

/// Outcome of a run, the same in the closing rule and in the picker afterwards:
/// `✔ api test · ok · 2.7s`, `✘ web lint · exit 2 · 0.3s`, `■ web dev · interrupted … · 9.1s`.
fn result_line(label: &str, code: i32, secs: f64) -> (bool, String) {
    let desc = exec::describe_exit(code);
    if code == 0 {
        (true, format!("✔ {label} · ok · {secs:.1}s"))
    } else if desc.contains("interrupted") || desc.contains("SIGINT") {
        (false, format!("■ {label} · {desc} · {secs:.1}s"))
    } else {
        (false, format!("✘ {label} · {desc} · {secs:.1}s"))
    }
}

/// `── text ───…` across the terminal (at most 120 columns), closing a task's output.
fn ruled(text: &str) -> String {
    let cols = ratatui::crossterm::terminal::size()
        .map_or(80, |(w, _)| w as usize)
        .min(120);
    format!(
        "── {text} {}",
        "─".repeat(cols.saturating_sub(width(text) + 4))
    )
}

/// Minimal ANSI painter for the lines printed on the normal screen around a run.
/// Colours when stderr is a terminal and `NO_COLOR` is unset, or when `FORCE_COLOR` is set
/// to anything but `0`/`false` (the convention Node.js and many CLIs follow).
struct Ansi {
    on: bool,
}

pub fn force_color() -> Option<bool> {
    std::env::var("FORCE_COLOR")
        .ok()
        .map(|v| !(v == "0" || v.eq_ignore_ascii_case("false")))
}

impl Ansi {
    fn new(th: &Theme) -> Self {
        let on = force_color().unwrap_or(th.color && std::io::stderr().is_terminal());
        Self { on }
    }

    fn paint(&self, text: &str, style: Style) -> String {
        use ratatui::style::Color;
        if !self.on {
            return text.to_owned();
        }
        let mut codes: Vec<String> = Vec::new();
        if style.add_modifier.contains(Modifier::BOLD) {
            codes.push("1".into());
        }
        if style.add_modifier.contains(Modifier::DIM) {
            codes.push("2".into());
        }
        match style.fg {
            Some(Color::Red) => codes.push("31".into()),
            Some(Color::Green) => codes.push("32".into()),
            Some(Color::Yellow) => codes.push("33".into()),
            Some(Color::Magenta) => codes.push("35".into()),
            Some(Color::Cyan) => codes.push("36".into()),
            Some(Color::LightBlue) => codes.push("94".into()),
            Some(Color::Indexed(n)) => codes.push(format!("38;5;{n}")),
            _ => {}
        }
        if codes.is_empty() {
            return text.to_owned();
        }
        format!("\x1b[{}m{text}\x1b[0m", codes.join(";"))
    }
}

enum After {
    Back,
    Again,
    Quit,
}

/// After a task: Enter returns to the picker, r runs it again, q / Esc quits.
/// Enter does not re-run on purpose: a stray Enter must never start a deploy twice.
fn after_run() -> std::io::Result<After> {
    let _ = std::io::stderr().flush();
    enable_raw_mode()?;
    let res = loop {
        if let Event::Key(k) = event::read()? {
            if k.kind != KeyEventKind::Press {
                continue;
            }
            match k.code {
                KeyCode::Enter | KeyCode::Char(' ') => break After::Back,
                KeyCode::Char('r') => break After::Again,
                KeyCode::Char('q') | KeyCode::Esc => break After::Quit,
                KeyCode::Char('c') if k.modifiers.contains(KeyModifiers::CONTROL) => {
                    break After::Quit;
                }
                _ => {}
            }
        }
    };
    disable_raw_mode()?;
    eprintln!();
    Ok(res)
}

fn picker(term: &mut DefaultTerminal, app: &mut App) -> std::io::Result<Action> {
    loop {
        term.draw(|f| draw(f, app))?;
        match handle(app, event::read()?) {
            Some(Action::Open(url)) => {
                app.status = Some(match exec::open_url(&url) {
                    Ok(()) => (true, format!("opened {url}")),
                    Err(e) => (false, format!("cannot open {url}: {e}")),
                });
            }
            Some(action) => return Ok(action),
            None => {}
        }
    }
}

// ------------------------------------------------------------------ input

fn press(app: &mut App, b: Button) -> Option<Action> {
    match b {
        Button::Folders => {
            app.open_switcher();
            None
        }
        Button::Open => open_selected(app),
        Button::Switch(k) => {
            let entry = *app.switcher().get(k)?;
            app.choose(entry)
        }
        Button::Quit => Some(Action::Quit),
        Button::Run => match std::mem::replace(&mut app.mode, Mode::Search) {
            Mode::Confirm(p, t, args) => Some(Action::Run(p, t, args)),
            other => {
                app.mode = other;
                run_selected(app)
            }
        },
        Button::Args => {
            if matches!(app.selected(), Some(Item { t: Some(_), .. })) {
                app.mode = Mode::Args(String::new());
            }
            None
        }
        Button::Back
            if matches!(
                app.mode,
                Mode::Spaces(_) | Mode::AddFolder(_) | Mode::Confirm(..)
            ) =>
        {
            app.mode = Mode::Search;
            None
        }
        Button::Back => escape(app),
        Button::PageUp => {
            app.step(-10);
            None
        }
        Button::PageDown => {
            app.step(10);
            None
        }
        Button::Fold(root) => {
            app.toggle_fold(root);
            None
        }
    }
}

/// Escape: clear the search, then leave the project scope, then quit.
fn escape(app: &mut App) -> Option<Action> {
    if !app.query.is_empty() {
        app.query.clear();
    } else if app.scope.is_some() {
        app.scope = None;
    } else {
        return Some(Action::Quit);
    }
    app.refilter();
    None
}

/// Ctrl+O and the footer's `open`: the selected task's URL in the browser.
fn open_selected(app: &mut App) -> Option<Action> {
    let url = app
        .selected()
        .and_then(|i| i.t.map(|t| &app.projects[i.p].tasks[t]))
        .and_then(|t| t.url.clone());
    if url.is_none() {
        app.status = Some((false, "this task serves no known URL".into()));
    }
    url.map(Action::Open)
}

/// Run a task, or first show it and ask when it is a shell line or looks risky.
fn start(app: &mut App, p: usize, t: usize, args: Vec<String>) -> Option<Action> {
    if exec::confirmation(&app.projects[p].tasks[t]).is_some() {
        app.mode = Mode::Confirm(p, t, args);
        return None;
    }
    Some(Action::Run(p, t, args))
}

/// Input while a run waits for confirmation: `y` or Enter runs, `n` or Esc cancels.
fn handle_confirm(app: &mut App, ev: Event) -> Option<Action> {
    let Mode::Confirm(p, t, args) = &app.mode else {
        return None;
    };
    let run = Action::Run(*p, *t, args.clone());
    match ev {
        Event::Key(k) if k.kind == KeyEventKind::Press => match k.code {
            KeyCode::Enter | KeyCode::Char('y') | KeyCode::Char('Y') => {
                app.mode = Mode::Search;
                return Some(run);
            }
            KeyCode::Char('c') if k.modifiers.contains(KeyModifiers::CONTROL) => {
                return Some(Action::Quit);
            }
            KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => app.mode = Mode::Search,
            _ => {}
        },
        Event::Mouse(m) if app.mouse => match m.kind {
            MouseEventKind::Moved => app.hover_button = app.button_at(m.column, m.row),
            MouseEventKind::Down(MouseButton::Left) => {
                if let Some(b) = app.button_at(m.column, m.row) {
                    return press(app, b);
                }
            }
            _ => {}
        },
        _ => {}
    }
    None
}

fn run_selected(app: &mut App) -> Option<Action> {
    match app.selected() {
        // A task that needs input opens the argument prompt instead of failing.
        Some(Item { p, t: Some(t), .. }) if !app.projects[*p].tasks[*t].params.is_empty() => {
            app.mode = Mode::Args(String::new());
            None
        }
        Some(&Item { p, t: Some(t), .. }) => start(app, p, t, vec![]),
        Some(Item { p, t: None, .. }) => {
            let id = &app.projects[*p].id;
            app.status = Some((
                false,
                format!("{id}: no tasks detected, see `{} path {id}`", crate::me()),
            ));
            None
        }
        None => None,
    }
}

/// Input in the workspace switcher and in its "add a folder" prompt.
fn handle_switcher(app: &mut App, ev: Event) -> Option<Action> {
    let n = app.switcher().len();
    match (&mut app.mode, ev) {
        (Mode::AddFolder(buf), Event::Key(k)) if k.kind == KeyEventKind::Press => match k.code {
            KeyCode::Esc => {
                let at = n - 2; // back on "add a folder"
                app.mode = Mode::Spaces(at);
            }
            KeyCode::Backspace => {
                buf.pop();
            }
            KeyCode::Enter => {
                let dir = kiwa_core::config::user_path(buf, app.home.as_deref(), cfg!(windows));
                if dir.is_dir() {
                    return Some(Action::AddFolder(std::path::absolute(&dir).unwrap_or(dir)));
                }
                app.status = Some((false, format!("not a folder: {}", dir.display())));
            }
            KeyCode::Char('c') if k.modifiers.contains(KeyModifiers::CONTROL) => {
                return Some(Action::Quit);
            }
            KeyCode::Char(c) => buf.push(c),
            _ => {}
        },
        (Mode::Spaces(sel), Event::Key(k)) if k.kind == KeyEventKind::Press => {
            let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
            match k.code {
                KeyCode::Up => *sel = (*sel + n - 1) % n,
                KeyCode::Down | KeyCode::Tab => *sel = (*sel + 1) % n,
                KeyCode::Home => *sel = 0,
                KeyCode::End => *sel = n - 1,
                KeyCode::Esc => app.mode = Mode::Search,
                KeyCode::Char('c') if ctrl => return Some(Action::Quit),
                KeyCode::Enter | KeyCode::Char(' ') => {
                    let at = *sel;
                    let entry = app.switcher()[at];
                    return app.choose(entry);
                }
                _ => {}
            }
        }
        (_, Event::Mouse(m)) if app.mouse => match m.kind {
            MouseEventKind::Moved => {
                app.armed = true;
                app.hover_button = app.button_at(m.column, m.row);
            }
            MouseEventKind::Down(MouseButton::Left)
                if app.armed || app.opened.elapsed() >= ARM_FALLBACK =>
            {
                if let Some(b) = app.button_at(m.column, m.row) {
                    return press(app, b);
                }
            }
            _ => {}
        },
        _ => {}
    }
    None
}

/// Apply one input event: a pure state transition, unit-testable without a terminal.
fn handle(app: &mut App, ev: Event) -> Option<Action> {
    if matches!(ev, Event::Key(_)) {
        app.armed = true;
    }
    if matches!(app.mode, Mode::Spaces(_) | Mode::AddFolder(_)) {
        return handle_switcher(app, ev);
    }
    if matches!(app.mode, Mode::Confirm(..)) {
        return handle_confirm(app, ev);
    }
    if let Mode::Args(buf) = &mut app.mode {
        if let Event::Key(k) = ev {
            if k.kind != KeyEventKind::Press {
                return None;
            }
            match k.code {
                KeyCode::Esc => app.mode = Mode::Search,
                KeyCode::Backspace => {
                    buf.pop();
                }
                KeyCode::Enter => match crate::words::split(buf) {
                    Ok(args) => {
                        if let Some(Item { p, t: Some(t), .. }) = app.selected() {
                            let (p, t) = (*p, *t);
                            let task = &app.projects[p].tasks[t];
                            if args.is_empty() && !task.params.is_empty() {
                                app.status = Some((
                                    false,
                                    format!("{} needs {}", task.name, task.params_usage()),
                                ));
                                return None;
                            }
                            app.mode = Mode::Search;
                            return start(app, p, t, args);
                        }
                    }
                    Err(e) => app.status = Some((false, e)),
                },
                KeyCode::Char(c) => buf.push(c),
                _ => {}
            }
        }
        return None;
    }
    match ev {
        Event::Key(k) if k.kind == KeyEventKind::Press => {
            let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
            if ctrl && k.code == KeyCode::Char('c') {
                return Some(Action::Quit);
            }
            // Keyboard focus on a header button.
            if let Some(f) = app.focus {
                match k.code {
                    KeyCode::Enter | KeyCode::Char(' ') => return press(app, TOOLBAR[f]),
                    KeyCode::Left => app.focus = Some(f.saturating_sub(1)),
                    KeyCode::Right => app.focus = Some((f + 1).min(TOOLBAR.len() - 1)),
                    KeyCode::Tab => app.focus = (f + 1 < TOOLBAR.len()).then_some(f + 1),
                    KeyCode::BackTab => app.focus = f.checked_sub(1),
                    KeyCode::Down | KeyCode::Esc => app.focus = None,
                    KeyCode::Char(c) if !ctrl => {
                        app.focus = None;
                        app.query.push(c);
                        app.refilter();
                    }
                    _ => {}
                }
                return None;
            }
            match k.code {
                KeyCode::Esc => return escape(app),
                KeyCode::Enter => return run_selected(app),
                KeyCode::Left => {
                    if let Some((root, _)) = app.selected_group().filter(|_| app.can_fold()) {
                        app.toggle_fold(root);
                    }
                }
                KeyCode::Right => {
                    if let Some((root, _)) = app.selected_group().filter(|_| app.can_unfold()) {
                        app.toggle_fold(root);
                    } else if matches!(app.selected(), Some(Item { t: Some(_), .. })) {
                        app.mode = Mode::Args(String::new());
                    }
                }
                KeyCode::Tab => app.focus = Some(0),
                KeyCode::BackTab => app.focus = Some(TOOLBAR.len() - 1),
                KeyCode::Up => app.step_around(-1),
                KeyCode::Char('p') if ctrl => app.step_around(-1),
                KeyCode::Down => app.step_around(1),
                KeyCode::Char('n') if ctrl => app.step_around(1),
                KeyCode::PageUp => app.step(-10),
                KeyCode::PageDown => app.step(10),
                KeyCode::Home => app.list.select((!app.matches.is_empty()).then_some(0)),
                KeyCode::End => app.step(isize::MAX / 2),
                KeyCode::Char('u') if ctrl => {
                    app.query.clear();
                    app.refilter();
                }
                KeyCode::Char('o') if ctrl => return open_selected(app),
                KeyCode::Backspace => {
                    if app.query.pop().is_none() {
                        app.scope = None; // widen back to every project
                    }
                    app.refilter();
                }
                KeyCode::Char(c) if !ctrl => {
                    app.query.push(c);
                    app.refilter();
                }
                _ => {}
            }
        }
        Event::Mouse(m) if app.mouse => {
            let armed = app.armed || app.opened.elapsed() >= ARM_FALLBACK;
            match m.kind {
                MouseEventKind::ScrollUp => {
                    app.armed = true;
                    app.step(-3);
                }
                MouseEventKind::ScrollDown => {
                    app.armed = true;
                    app.step(3);
                }
                MouseEventKind::Moved => {
                    app.armed = true;
                    app.hover = app.row_at(m.row);
                    app.hover_button = app.button_at(m.column, m.row);
                }
                // Right-click: select that row and ask for its arguments. Never runs anything.
                MouseEventKind::Down(MouseButton::Right) if armed => {
                    if let Some(k) = app.row_at(m.row) {
                        app.focus = None;
                        app.list.select(Some(k));
                        if matches!(app.selected(), Some(Item { t: Some(_), .. })) {
                            app.mode = Mode::Args(String::new());
                        }
                    }
                }
                MouseEventKind::Down(MouseButton::Left) if armed => {
                    if let Some(b) = app.button_at(m.column, m.row) {
                        return press(app, b);
                    }
                    if let Some(k) = app.row_at(m.row) {
                        app.focus = None;
                        // First click moves the selection here; clicking the already-selected row runs it.
                        if app.list.selected() == Some(k) {
                            return run_selected(app);
                        }
                        app.list.select(Some(k));
                    }
                }
                _ => {}
            }
        }
        _ => {}
    }
    None
}

// ------------------------------------------------------------------ drawing

fn is_main_branch(b: &str) -> bool {
    matches!(b, "main" | "master" | "trunk" | "develop")
}

/// Whether `b` is the project's default branch: the remote's when the clone recorded it,
/// otherwise one of the usual default names.
fn is_default_branch(p: &Project, b: &str) -> bool {
    p.default_branch
        .as_deref()
        .map_or_else(|| is_main_branch(b), |d| d == b)
}

fn width(s: &str) -> usize {
    UnicodeWidthStr::width(s)
}

/// Pad with spaces to `w` terminal cells.
fn pad_to(s: &str, w: usize) -> String {
    let n = width(s);
    if n >= w {
        s.to_owned()
    } else {
        format!("{s}{}", " ".repeat(w - n))
    }
}

/// `s` cut from the left to `w` cells, `…` included: the end of a path is what tells
/// projects apart. The cut lands on a character boundary, never inside a wide character.
fn truncate_left(s: &str, w: usize) -> String {
    if width(s) <= w {
        return s.to_owned();
    }
    let room = w.saturating_sub(1);
    let cut = s
        .char_indices()
        .rev()
        .scan(0, |cells, (at, c)| {
            *cells += c.width().unwrap_or(0);
            Some((at, *cells))
        })
        .take_while(|&(_, cells)| cells <= room)
        .last()
        .map_or(s.len(), |(at, _)| at);
    format!("…{}", &s[cut..])
}

fn draw(f: &mut Frame, app: &mut App) {
    app.view.hits.clear();
    app.view.rows.clear();
    let [header, input, body, footer] = Layout::vertical([
        Constraint::Length(2),
        Constraint::Length(2),
        Constraint::Min(3),
        Constraint::Length(1),
    ])
    .areas(f.area());
    draw_header(f, app, header);
    draw_input(f, app, input);
    if matches!(app.mode, Mode::Spaces(_) | Mode::AddFolder(_)) {
        draw_switcher(f, app, body);
    } else if body.width >= 100 {
        let [list, details] =
            Layout::horizontal([Constraint::Percentage(58), Constraint::Percentage(42)])
                .areas(body);
        draw_list(f, app, list, true);
        draw_details(f, app, details);
    } else {
        draw_list(f, app, body, false);
    }
    draw_footer(f, app, footer);
}

/// The workspace switcher: `all`, each workspace with its project count and folders (missing
/// ones flagged), "add a folder", and the setup. Every line is a button.
fn draw_switcher(f: &mut Frame, app: &mut App, area: Rect) {
    let th = app.theme;
    let sel = match app.mode {
        Mode::Spaces(s) => Some(s),
        _ => None,
    };
    let entries = app.switcher();
    let name_w = app
        .spaces
        .iter()
        .map(|s| width(&s.name))
        .max()
        .unwrap_or(0)
        .max(14);
    let mut lines = Vec::new();
    for (k, e) in entries.iter().enumerate() {
        let y = area.y + k as u16;
        if y >= area.y + area.height {
            break;
        }
        let current = match e {
            Switch::All => app.space.is_none(),
            Switch::Space(s) => app.space == Some(*s),
            _ => false,
        };
        let (name, detail, warn) = match e {
            Switch::All => (
                "all".to_owned(),
                format!(
                    "{:>4} projects   every workspace at once",
                    app.projects_in(None)
                ),
                String::new(),
            ),
            Switch::Space(s) => {
                let sp = &app.spaces[*s];
                let roots: Vec<String> = sp.roots.iter().map(|r| r.display().to_string()).collect();
                let missing = &app.missing[*s];
                let warn = match missing.len() {
                    0 => String::new(),
                    1 => format!("   ⚠ missing: {}", missing[0].display()),
                    n => format!("   ⚠ {n} folders missing"),
                };
                (
                    sp.name.clone(),
                    format!(
                        "{:>4} projects   {}",
                        app.projects_in(Some(*s)),
                        truncate_middle(&roots.join(" · "), 48)
                    ),
                    warn,
                )
            }
            Switch::Add => ("+ add a folder…".to_owned(), String::new(), String::new()),
            Switch::Setup => (
                "kw init".to_owned(),
                "choose the folders to scan again".to_owned(),
                String::new(),
            ),
        };
        let on = sel == Some(k);
        let b = Button::Switch(k);
        let base = if on { th.accent } else { Style::new() };
        let mut spans = vec![
            Span::styled(if on { "● " } else { "  " }, th.accent),
            Span::styled(pad_to(&name, name_w), button_style(app, b, base)),
            Span::raw("  "),
            Span::styled(detail, th.dim),
            Span::styled(warn, th.warn),
        ];
        if current {
            spans.push(Span::styled("   (in view)", th.dim));
        }
        app.view.hits.push((Rect::new(area.x, y, area.width, 1), b));
        lines.push(Line::from(spans));
    }
    f.render_widget(Paragraph::new(lines), area);
}

/// Focused (keyboard) buttons are drawn reversed, which survives `NO_COLOR`; hovered ones
/// take the hover background.
fn button_style(app: &App, b: Button, base: Style) -> Style {
    let focused = app.focus.is_some_and(|f| TOOLBAR[f] == b);
    if focused {
        base.add_modifier(Modifier::REVERSED)
    } else if app.hover_button == Some(b) {
        base.patch(app.theme.hover)
    } else {
        base
    }
}

/// One line at the top, then a rule:
/// ` kiwa   ~/dev / billing             14 projects · 56 tasks · scanned in 17 ms   Folders  Quit `
fn draw_header(f: &mut Frame, app: &mut App, area: Rect) {
    let th = app.theme;
    let [top, rule] = Layout::vertical([Constraint::Length(1), Constraint::Length(1)]).areas(area);
    let badge = match th.accent.fg {
        Some(c) => Style::new()
            .fg(Color::Black)
            .bg(c)
            .add_modifier(Modifier::BOLD),
        None => th.bold.add_modifier(Modifier::REVERSED),
    };
    let stats = format!(
        "{} projects · {} tasks · scanned in {} ms",
        app.counts.0,
        app.counts.1,
        app.scan.as_millis()
    );
    let tools: Vec<(String, Button)> = TOOLBAR
        .iter()
        .map(|&b| {
            let label = match b {
                Button::Folders => " Folders ",
                _ => " Quit ",
            };
            (label.to_owned(), b)
        })
        .collect();
    // One space before each button, one after the last.
    let tools_w: usize = tools.iter().map(|(l, _)| width(l) + 1).sum::<usize>() + 1;
    let scope = app.scope.map(|s| app.projects[s].id.clone());
    let scope_w = scope.as_ref().map_or(0, |s| width(s) + 3);
    let total = top.width as usize;
    let badge_w = 6 + 2;
    let stats_w = width(&stats) + 2;
    let show_stats = badge_w + scope_w + 12 + stats_w + tools_w <= total;
    let right_w = tools_w + if show_stats { stats_w } else { 0 };
    let roots = truncate_middle(
        &app.place(),
        total.saturating_sub(badge_w + scope_w + right_w + 1),
    );
    let mut spans = vec![
        Span::styled(" kiwa ", badge),
        Span::raw("  "),
        Span::styled(roots.clone(), th.dim),
    ];
    let mut left_w = badge_w + width(&roots);
    if let Some(s) = scope {
        spans.push(Span::styled(" / ", th.dim));
        left_w += 3 + width(&s);
        spans.push(Span::styled(s, th.accent));
    }
    let pad = total.saturating_sub(left_w + right_w);
    spans.push(Span::raw(" ".repeat(pad)));
    if show_stats {
        spans.push(Span::styled(stats, th.dim));
        spans.push(Span::raw("  "));
    }
    let mut x = top.x + (left_w + pad + if show_stats { stats_w } else { 0 }) as u16;
    for (label, b) in tools {
        spans.push(Span::raw(" "));
        x += 1;
        let w = width(&label) as u16;
        spans.push(Span::styled(label, button_style(app, b, th.bold)));
        app.view.hits.push((Rect::new(x, top.y, w, 1), b));
        x += w;
    }
    f.render_widget(Paragraph::new(Line::from(spans)), top);
    f.render_widget(
        Paragraph::new(Line::styled("─".repeat(rule.width as usize), th.border)),
        rule,
    );
}

/// The prompt, then on the right the outcome of the last run or the position in the list.
fn draw_input(f: &mut Frame, app: &App, area: Rect) {
    let th = app.theme;
    let area = Rect { height: 1, ..area };
    let needs = app
        .selected()
        .and_then(|i| i.t.map(|t| &app.projects[i.p].tasks[t]))
        .filter(|t| !t.params.is_empty())
        .map(|t| format!("needs {}", t.params_usage()));
    let (prefix, text, hint) = match &app.mode {
        Mode::Args(buf) => {
            let label = app
                .selected()
                .and_then(|i| i.t.map(|t| (i.p, t)))
                .map(|(p, t)| {
                    format!(
                        " {} · {}  args ❯ ",
                        app.projects[p].id, app.projects[p].tasks[t].name
                    )
                })
                .unwrap_or_else(|| " args ❯ ".into());
            let hint = needs
                .clone()
                .unwrap_or_else(|| "e.g. --port 4000 \"two words\"".into());
            (label, buf.clone(), hint)
        }
        Mode::Spaces(_) => (
            " workspaces ❯ ".to_owned(),
            String::new(),
            "choose one, or type @name in the search".to_owned(),
        ),
        Mode::AddFolder(buf) => (
            " add a folder ❯ ".to_owned(),
            buf.clone(),
            "a path: ~/work, C:\\code, or one pasted from a file manager".to_owned(),
        ),
        Mode::Confirm(p, t, args) => {
            let task = &app.projects[*p].tasks[*t];
            let why = exec::confirmation(task).unwrap_or_default();
            (
                format!(" {} · {} {why} ❯ ", app.projects[*p].id, task.name),
                exec::shown(task, args),
                String::new(),
            )
        }
        Mode::Search => {
            let label = match app.scope {
                Some(s) => format!(" {} ❯ ", app.projects[s].id),
                None => " ❯ ".to_owned(),
            };
            let hint = if app.scope.is_some() {
                "type to filter tasks · esc all projects"
            } else {
                "type to search projects and tasks, e.g. `api test`"
            };
            (label, app.query.clone(), hint.to_owned())
        }
    };
    let prefix_w = width(&prefix);
    let mut used = prefix_w + width(&text);
    let mut line = vec![Span::styled(prefix, th.accent), Span::raw(text.clone())];
    if text.is_empty() {
        used += width(&hint);
        line.push(Span::styled(hint, th.dim));
    }
    let (right, style) = match &app.status {
        Some((ok, msg)) => (msg.clone(), if *ok { th.ok } else { th.err }),
        None if !app.matches.is_empty() => (
            format!(
                "{}/{}",
                app.list.selected().map_or(0, |s| s + 1),
                app.matches.len()
            ),
            th.dim,
        ),
        None => (String::new(), th.dim),
    };
    let total = area.width as usize;
    let right = truncate_middle(&right, total.saturating_sub(used + 3));
    let pad = total.saturating_sub(used + width(&right) + 1);
    line.push(Span::raw(" ".repeat(pad)));
    line.push(Span::styled(right, style));
    f.render_widget(Paragraph::new(Line::from(line)), area);
    if app.focus.is_none() {
        f.set_cursor_position((area.x + (prefix_w + width(&text)) as u16, area.y));
    }
}

/// Spans for `text`, underlining characters whose position (offset by `start` in the
/// haystack) is in `hits`.
fn highlighted(
    text: &str,
    start: usize,
    hits: &HashSet<usize>,
    base: Style,
    matched: Style,
) -> Vec<Span<'static>> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut run = String::new();
    let mut run_hit = false;
    for (i, c) in text.chars().enumerate() {
        let hit = hits.contains(&(start + i));
        if hit != run_hit && !run.is_empty() {
            let st = if run_hit { base.patch(matched) } else { base };
            spans.push(Span::styled(std::mem::take(&mut run), st));
        }
        run_hit = hit;
        run.push(c);
    }
    if !run.is_empty() {
        spans.push(Span::styled(
            run,
            if run_hit { base.patch(matched) } else { base },
        ));
    }
    spans
}

struct Columns {
    id_w: usize,
    task_w: usize,
    runner_w: usize,
    /// 0 when the branch column does not fit.
    branch_w: usize,
    width: usize,
    x: u16,
}

/// Room the last-run tag needs on the right (`✔ 59 min ago`), kept free by the branch column.
const LAST_RUN_W: usize = 14;

fn row_line(app: &mut App, k: usize, prev: Option<usize>, cols: &Columns, y: u16) -> Line<'static> {
    let th = app.theme;
    let it = &app.items[app.matches[k]];
    let p = &app.projects[it.p];
    let selected = app.list.selected() == Some(k);
    let hovered = app.hover == Some(k);
    let hits: HashSet<usize> = fuzzy::indices(app.search(), &it.hay).into_iter().collect();

    let mut spans: Vec<Span<'static>> = vec![if selected {
        Span::styled("● ", th.accent)
    } else if hovered {
        Span::styled("○ ", th.dim)
    } else {
        Span::raw("  ")
    }];
    let (prefix, name, more, fold) = app.id_label(k);
    let shown = truncate_left(
        &name,
        cols.id_w.saturating_sub(width(&prefix) + width(&more)),
    );
    // Only the matches inside the project id: the ones of the task name start at
    // `id_len + 1` in the haystack and must not underline the id column's padding.
    let id_hits: HashSet<usize> = if prefix.is_empty() && shown.chars().count() == it.id_len {
        hits.iter().copied().filter(|&h| h < it.id_len).collect()
    } else {
        HashSet::new()
    };
    let id_hits = &id_hits;
    let id = format!("{prefix}{shown}{more}");
    let mut used = 2;
    match it.t {
        Some(ti) => {
            let t = &p.tasks[ti];
            // Rows of the same project as the row above show its name faint.
            let continues = prev.is_some_and(|pk| app.items[app.matches[pk]].p == it.p);
            let (id_style, task_style) = if selected {
                (th.accent, th.accent)
            } else if continues {
                (th.dim, th.bold)
            } else {
                (Style::new(), th.bold)
            };
            spans.extend(highlighted(
                &pad_to(&id, cols.id_w),
                0,
                id_hits,
                id_style,
                th.matched,
            ));
            spans.push(Span::raw("  "));
            spans.extend(highlighted(
                &pad_to(&t.name, cols.task_w),
                it.id_len + 1,
                &hits,
                task_style,
                th.matched,
            ));
            spans.push(Span::raw("  "));
            used += cols.id_w + 2 + cols.task_w.max(width(&t.name)) + 2;
            if cols.branch_w > 0 {
                spans.push(Span::styled(pad_to(&t.runner, cols.runner_w), th.dim));
                spans.push(Span::raw("  "));
                used += cols.runner_w.max(width(&t.runner)) + 2;
                // On the project's first row only, like its name.
                let branch = p.branch.as_deref().filter(|_| !continues);
                if let Some(b) = branch {
                    let style = if is_default_branch(p, b) {
                        th.dim
                    } else {
                        th.branch_other
                    };
                    let b = truncate_middle(b, cols.branch_w);
                    used += width(&b);
                    spans.push(Span::styled(b, style));
                }
            } else {
                spans.push(Span::styled(t.runner.clone(), th.dim));
                used += width(&t.runner);
            }
            if let Some(r) = app.last_run(it) {
                let (mark, st) = match r.exit_code {
                    Some(0) => ("✔", th.ok),
                    Some(_) => ("✘", th.err),
                    None => ("·", th.dim),
                };
                let ago = format!(" {} ", time_ago(app.now.saturating_sub(r.last)));
                let hint_w = 1 + width(&ago);
                if used + hint_w + 2 <= cols.width {
                    spans.push(Span::raw(" ".repeat(cols.width - used - hint_w)));
                    spans.push(Span::styled(mark, st));
                    spans.push(Span::styled(ago, th.dim));
                    used = cols.width;
                }
            }
        }
        None => {
            let stack: Vec<_> = p.ecosystems.iter().map(|e| e.label()).collect();
            let base = if selected { th.accent } else { th.dim };
            spans.extend(highlighted(
                &pad_to(&id, cols.id_w),
                0,
                id_hits,
                base,
                th.matched,
            ));
            let tail = format!("  no tasks · {}", stack.join(", "));
            used += cols.id_w + width(&tail);
            spans.push(Span::styled(tail, th.dim));
        }
    }
    if let Some(root) = fold {
        // The marker sits right after the selection gutter.
        app.view
            .hits
            .push((Rect::new(cols.x + 2, y, 2, 1), Button::Fold(root)));
    }
    if hovered {
        spans.push(Span::raw(" ".repeat(cols.width.saturating_sub(used))));
        return Line::from(spans).style(th.hover);
    }
    Line::from(spans)
}

/// The list, and in its last column either the divider from the details pane or, when the
/// list overflows, a scrollbar whose track pages up and down when clicked.
fn draw_list(f: &mut Frame, app: &mut App, area: Rect, divider: bool) {
    let th = app.theme;
    app.view.x = area.x;
    app.view.w = area.width;
    let rows = Rect {
        width: area.width.saturating_sub(2),
        ..area
    };
    let bar = Rect::new(
        area.x + area.width.saturating_sub(1),
        area.y,
        1,
        area.height,
    );
    let h = area.height as usize;
    let paint_bar = |f: &mut Frame, thumb: Option<(usize, usize)>| {
        let lines: Vec<Line> = (0..h)
            .map(|i| match thumb {
                Some((pos, len)) if i >= pos && i < pos + len => Line::styled("┃", th.accent),
                _ => Line::styled("│", th.border),
            })
            .collect();
        f.render_widget(Paragraph::new(lines), bar);
    };
    if app.items.is_empty() || app.matches.is_empty() {
        let lines = if app.items.is_empty() {
            vec![
                Line::styled("  No projects found here.", th.bold),
                Line::styled(
                    format!(
                        "  Choose your project folders with Folders (tab, then ⏎), or run `{} -w <dir>`.",
                        crate::me()
                    ),
                    th.dim,
                ),
            ]
        } else {
            vec![
                Line::styled(format!("  No match for “{}”.", app.query), th.bold),
                Line::styled("  esc clears the search · ⌫ removes a character", th.dim),
            ]
        };
        f.render_widget(Paragraph::new(lines), rows);
        if divider {
            paint_bar(f, None);
        }
        return;
    }
    let entries = app.entries();
    let n = entries.len();
    let cap = h.max(1);
    let sel = app.list.selected().unwrap_or(0);
    let sel_e = entries
        .iter()
        .position(|e| *e == Entry::Item(sel))
        .unwrap_or(0);
    let mut offset = app.offset;
    if sel_e < offset {
        offset = sel_e;
    }
    if sel_e >= offset + cap {
        offset = sel_e + 1 - cap;
    }
    // Keep a group title with its first row.
    if offset > 0 && offset == sel_e && matches!(entries[offset - 1], Entry::Header(_)) {
        offset -= 1;
    }
    offset = offset.min(n.saturating_sub(cap));
    app.offset = offset;
    let shown = cap.min(n - offset);
    let visible = &entries[offset..offset + shown];

    let rows_shown: Vec<usize> = visible
        .iter()
        .filter_map(|e| match e {
            Entry::Item(k) => Some(*k),
            Entry::Header(_) => None,
        })
        .collect();
    let id_w = rows_shown
        .iter()
        .map(|&k| {
            let (prefix, name, more, _) = app.id_label(k);
            width(&prefix) + width(&name) + width(&more)
        })
        .max()
        .unwrap_or(8)
        .clamp(8, 36);
    let task_w = rows_shown
        .iter()
        .filter_map(|&k| {
            let it = &app.items[app.matches[k]];
            it.t.map(|t| width(&app.projects[it.p].tasks[t].name))
        })
        .max()
        .unwrap_or(4)
        .min(24);
    let tasks_shown = || {
        rows_shown.iter().filter_map(|&k| {
            let it = &app.items[app.matches[k]];
            it.t.map(|t| (&app.projects[it.p], &app.projects[it.p].tasks[t]))
        })
    };
    let runner_w = tasks_shown()
        .map(|(_, t)| width(&t.runner))
        .max()
        .unwrap_or(0);
    let branch_w = tasks_shown()
        .filter_map(|(p, _)| p.branch.as_deref().map(width))
        .max()
        .unwrap_or(0)
        .min(24);
    // The branch column appears only when it leaves room for the last-run tag.
    let need = 2 + id_w + 2 + task_w + 2 + runner_w + 2 + branch_w + LAST_RUN_W;
    let cols = Columns {
        id_w,
        task_w,
        runner_w,
        branch_w: if need <= rows.width as usize {
            branch_w
        } else {
            0
        },
        width: rows.width as usize,
        x: rows.x,
    };

    let mut lines: Vec<Line> = Vec::new();
    let mut prev: Option<usize> = None;
    for (i, e) in visible.iter().copied().enumerate() {
        let y = rows.y + i as u16;
        match e {
            Entry::Header(title) => {
                let label = format!("  {title} ");
                let rule = "─".repeat(cols.width.saturating_sub(width(&label)));
                lines.push(Line::from(vec![
                    Span::styled(label, th.dim),
                    Span::styled(rule, th.border),
                ]));
                prev = None;
            }
            Entry::Item(k) => {
                lines.push(row_line(app, k, prev, &cols, y));
                app.view.rows.push((y, k));
                prev = Some(k);
            }
        }
    }
    f.render_widget(Paragraph::new(lines), rows);

    if n > cap {
        let len = (cap * cap / n).max(1);
        let pos = offset * (cap - len) / (n - cap);
        paint_bar(f, Some((pos, len)));
        if pos > 0 {
            app.view
                .hits
                .push((Rect::new(bar.x, bar.y, 1, pos as u16), Button::PageUp));
        }
        let below = cap - pos - len;
        if below > 0 {
            app.view.hits.push((
                Rect::new(bar.x, bar.y + (pos + len) as u16, 1, below as u16),
                Button::PageDown,
            ));
        }
    } else if divider {
        paint_bar(f, None);
    }
}

fn draw_details(f: &mut Frame, app: &App, area: Rect) {
    let th = app.theme;
    let block = Block::new().padding(Padding::new(2, 1, 0, 0));
    let inner_w = block.inner(area).width as usize;
    let kv = |k: &str, v: String, style: Style| -> Line<'static> {
        Line::from(vec![
            Span::styled(format!("{k:<9}"), th.dim),
            Span::styled(v, style),
        ])
    };
    let lines: Vec<Line> = match app.selected() {
        None => vec![Line::styled("no match", th.dim)],
        Some(it) => {
            let p = &app.projects[it.p];
            let about = app
                .abouts
                .borrow_mut()
                .entry(it.p)
                .or_insert_with(|| kiwa_core::about::about(p))
                .clone();
            let mut l = vec![Line::styled(p.id.clone(), th.bold)];
            if let Some(d) = &about.description {
                l.push(Line::styled(d.clone(), th.dim));
            }
            match it.t {
                Some(t) => {
                    let task = &p.tasks[t];
                    l.push(Line::from(vec![
                        Span::styled(task.name.clone(), th.accent),
                        Span::styled(format!("  {}", task.runner), th.dim),
                    ]));
                    l.push(Line::raw(""));
                    l.push(Line::from(vec![
                        Span::styled("$ ", th.dim),
                        Span::raw(display_argv(&task.command)),
                    ]));
                    l.push(Line::raw(""));
                    if let Some(d) = &task.description {
                        l.push(kv("script", d.clone(), th.dim));
                    }
                    if !task.hooks.is_empty() {
                        l.push(kv("hooks", task.hooks.join(", "), Style::new()));
                    }
                    if let Some(url) = &task.url {
                        l.push(kv("url", format!("{url}  (^O opens it)"), th.accent));
                    }
                    if let Some(r) = app.last_run(it) {
                        let (text, st) = match r.exit_code {
                            Some(0) => ("✔ exit 0".to_owned(), th.ok),
                            Some(c) => (format!("✘ {}", exec::describe_exit(c)), th.err),
                            None => ("·".to_owned(), th.dim),
                        };
                        let runs = if r.count == 1 {
                            "1 run".to_owned()
                        } else {
                            format!("{} runs", r.count)
                        };
                        l.push(Line::from(vec![
                            Span::styled(format!("{:<9}", "last run"), th.dim),
                            Span::styled(text, st),
                            Span::styled(
                                format!(" · {} · {runs}", time_ago(app.now.saturating_sub(r.last))),
                                th.dim,
                            ),
                        ]));
                    }
                }
                None => {
                    l.push(Line::styled("no tasks detected", th.dim));
                    l.push(Line::raw(""));
                }
            }
            let folder = p.path.display().to_string();
            l.push(kv(
                "folder",
                truncate_middle(&folder, inner_w.saturating_sub(9)),
                Style::new(),
            ));
            let stack: Vec<_> = p.ecosystems.iter().map(|e| e.label()).collect();
            l.push(kv("stack", stack.join(", "), Style::new()));
            if !about.requires.is_empty() {
                let needs: Vec<String> = about
                    .requires
                    .iter()
                    .map(|r| format!("{} {}", r.tool, r.version))
                    .collect();
                l.push(kv("needs", needs.join(", "), Style::new()));
            }
            if let Some(lic) = &about.license {
                l.push(kv("licence", lic.clone(), th.dim));
            }
            let node = &app.tree[it.p];
            if node.root != it.p {
                let why = p
                    .member_of
                    .as_ref()
                    .map_or("folder".to_owned(), |m| m.from.clone());
                l.push(kv(
                    "monorepo",
                    format!("{} · {why}", app.projects[node.root].id),
                    Style::new(),
                ));
            } else if node.members > 0 {
                l.push(kv("packages", node.members.to_string(), Style::new()));
            }
            for s in &p.detected_from {
                l.push(kv(
                    "detected",
                    format!("{} · {}", s.ecosystem.label(), s.from),
                    th.dim,
                ));
            }
            if let Some(b) = &p.branch {
                let st = if is_default_branch(p, b) {
                    th.branch_main
                } else {
                    th.branch_other
                };
                l.push(kv("branch", b.clone(), st));
            }
            l.push(kv("tasks", p.tasks.len().to_string(), Style::new()));
            if !p.hints.is_empty() || !p.warnings.is_empty() {
                l.push(Line::raw(""));
            }
            for h in &p.hints {
                l.push(Line::styled(format!("ℹ {h}"), th.dim));
            }
            for w in &p.warnings {
                l.push(Line::styled(format!("⚠ {w}"), th.warn));
            }
            l
        }
    };
    f.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(block),
        area,
    );
}

/// The keys of the current mode. Those that act on the list are buttons too, so the mouse
/// reaches every action, even in a terminal too narrow for the details pane.
fn draw_footer(f: &mut Frame, app: &mut App, area: Rect) {
    let th = app.theme;
    let keys: Vec<(&str, &str, Option<Button>)> = match (&app.mode, app.focus) {
        (Mode::Args(_), _) => vec![("⏎", "run", None), ("esc", "cancel", None)],
        (Mode::Spaces(_), _) => vec![
            ("↑↓", "move", None),
            ("⏎", "open", None),
            ("esc", "back", Some(Button::Back)),
        ],
        (Mode::AddFolder(_), _) => vec![("⏎", "add", None), ("esc", "back", None)],
        (Mode::Confirm(..), _) => vec![
            ("y", "run it", Some(Button::Run)),
            ("esc", "cancel", Some(Button::Back)),
        ],
        (_, Some(_)) => vec![
            ("←→", "choose", None),
            ("⏎", "press", None),
            ("↓", "back to the list", None),
            ("esc", "back", None),
        ],
        _ => {
            let group = app.selected_group().map(|(root, _)| Button::Fold(root));
            let mut k = vec![("↑↓", "move", None), ("⏎", "run", Some(Button::Run))];
            if app.can_unfold() {
                k.push(("→", "unfold", group));
            } else {
                k.push(("→", "args", Some(Button::Args)));
            }
            if app.can_fold() {
                k.push(("←", "fold", group));
            }
            let serves = app
                .selected()
                .and_then(|i| i.t.map(|t| &app.projects[i.p].tasks[t]))
                .is_some_and(|t| t.url.is_some());
            if serves {
                k.push(("^O", "open", Some(Button::Open)));
            }
            k.push(("tab", "buttons", None));
            k.push(("esc", app.esc_label(), Some(Button::Back)));
            k
        }
    };
    let mut spans = vec![Span::raw(" ")];
    let mut x = area.x + 1;
    for (i, (key, label, b)) in keys.into_iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled(" · ", th.border));
            x += 3;
        }
        let text_w = (width(key) + 1 + width(label)) as u16;
        let (ks, ls) = match b {
            Some(b) => {
                app.view.hits.push((Rect::new(x, area.y, text_w, 1), b));
                (button_style(app, b, th.bold), button_style(app, b, th.dim))
            }
            None => (th.bold, th.dim),
        };
        spans.push(Span::styled(key, ks));
        spans.push(Span::styled(format!(" {label}"), ls));
        x += text_w;
    }
    if let Some(n) = app.note {
        spans.push(Span::raw("   "));
        spans.push(Span::styled(n, th.warn));
    }
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

#[cfg(test)]
mod picker_tests {
    use super::*;
    use kiwa_core::model::{Ecosystem, Task};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::crossterm::event::{KeyEvent, KeyEventState, MouseEvent};

    fn projects() -> Vec<Project> {
        let mut dev = Task::new("dev", "npm", &["npm", "run", "dev"]);
        dev.description = Some("vite".into());
        let mut build = Task::new("build", "npm", &["npm", "run", "build"]);
        build.hooks = vec!["prebuild".into()];
        vec![
            Project {
                id: "apps/web".into(),
                name: "web".into(),
                path: PathBuf::from("/ws/apps/web"),
                ecosystems: vec![Ecosystem::Node],
                branch: Some("main".into()),
                default_branch: None,
                tasks: vec![dev, build],
                warnings: vec![],
                hints: vec!["dependencies not installed: run `npm install`".into()],
                detected_from: vec![kiwa_core::model::Source {
                    ecosystem: Ecosystem::Node,
                    from: "package-lock.json".into(),
                }],
                member_of: None,
            },
            Project {
                id: "billing".into(),
                name: "billing".into(),
                path: PathBuf::from("/ws/billing"),
                ecosystems: vec![Ecosystem::Go],
                branch: Some("feature/x".into()),
                default_branch: None,
                tasks: vec![
                    Task::new("test", "go", &["go", "test", "./..."]),
                    Task::new("vet", "go", &["go", "vet", "./..."]),
                ],
                warnings: vec!["package.json: invalid JSON".into()],
                hints: vec![],
                detected_from: vec![],
                member_of: None,
            },
            Project {
                id: "notes".into(),
                name: "notes".into(),
                path: PathBuf::from("/ws/notes"),
                ecosystems: vec![Ecosystem::Git],
                branch: None,
                default_branch: None,
                tasks: vec![],
                warnings: vec![],
                hints: vec![],
                detected_from: vec![],
                member_of: None,
            },
        ]
    }

    fn opts(scope: Option<&str>, mouse: bool) -> Options {
        Options {
            accent: Some("amber".into()),
            mouse,
            note: Some("test note"),
            scope: scope.map(PathBuf::from),
            roots: vec![PathBuf::from("/ws")],
            spaces: vec![Space {
                name: "default".into(),
                roots: vec![PathBuf::from("/ws")],
            }],
            space: Some(0),
            home: None,
        }
    }

    /// `default` holds every project; `work` holds `billing`; `old` has a missing folder.
    fn spaced() -> Options {
        let mut o = opts(None, true);
        o.spaces.push(Space {
            name: "work".into(),
            roots: vec![PathBuf::from("/ws/billing")],
        });
        o.spaces.push(Space {
            name: "old".into(),
            roots: vec![PathBuf::from("/nowhere/kiwa-test")],
        });
        o
    }

    fn app<'a>(ps: &'a [Project], o: &Options) -> App<'a> {
        App::new(ps, &State::default(), Duration::from_millis(12), o)
    }

    fn key(c: KeyCode) -> Event {
        Event::Key(KeyEvent::new(c, KeyModifiers::NONE))
    }
    fn ctrl(c: char) -> Event {
        Event::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL))
    }
    fn typed(a: &mut App, s: &str) {
        for c in s.chars() {
            assert!(handle(a, key(KeyCode::Char(c))).is_none());
        }
    }
    fn selected(a: &App) -> (String, Option<String>) {
        let i = a.selected().unwrap();
        let p = &a.projects[i.p];
        (p.id.clone(), i.t.map(|t| p.tasks[t].name.clone()))
    }
    fn render(a: &mut App, w: u16, h: u16) -> String {
        let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
        t.draw(|f| draw(f, a)).unwrap();
        let buf = t.backend().buffer();
        let mut s = String::new();
        for y in 0..h {
            for x in 0..w {
                s.push_str(buf[(x, y)].symbol());
            }
            s.push('\n');
        }
        s
    }
    fn mouse_at(kind: MouseEventKind, column: u16, row: u16) -> Event {
        Event::Mouse(MouseEvent {
            kind,
            column,
            row,
            modifiers: KeyModifiers::NONE,
        })
    }
    fn mouse(kind: MouseEventKind, row: u16) -> Event {
        mouse_at(kind, 5, row)
    }
    fn first_row(a: &App) -> u16 {
        a.view.rows[0].0
    }
    fn hit(a: &App, b: Button) -> Rect {
        a.view
            .hits
            .iter()
            .find(|(_, x)| *x == b)
            .map(|(r, _)| *r)
            .expect("button drawn")
    }

    #[test]
    fn lists_tasks_first_and_projects_without_tasks_last() {
        let ps = projects();
        let a = app(&ps, &opts(None, true));
        assert_eq!(a.matches.len(), 5);
        assert_eq!(a.items.last().unwrap().t, None);
        assert_eq!(a.counts, (3, 4));
    }

    #[test]
    fn typing_filters_across_projects_and_tasks() {
        let ps = projects();
        let mut a = app(&ps, &opts(None, true));
        typed(&mut a, "bil ve");
        assert_eq!(selected(&a), ("billing".into(), Some("vet".into())));
        handle(&mut a, ctrl('u'));
        assert!(a.query.is_empty());
        assert_eq!(a.matches.len(), 5);
        typed(&mut a, "zzzz");
        assert!(a.matches.is_empty() && a.selected().is_none());
        assert!(handle(&mut a, key(KeyCode::Enter)).is_none());
    }

    #[test]
    fn enter_runs_and_ctrl_c_quits() {
        let ps = projects();
        let mut a = app(&ps, &opts(None, true));
        typed(&mut a, "web build");
        assert!(
            matches!(handle(&mut a, key(KeyCode::Enter)), Some(Action::Run(0, 1, ref v)) if v.is_empty())
        );
        assert!(matches!(handle(&mut a, ctrl('c')), Some(Action::Quit)));
    }

    #[test]
    fn escape_clears_then_widens_then_quits() {
        let ps = projects();
        let mut a = app(&ps, &opts(Some("/ws/billing"), true));
        typed(&mut a, "te");
        assert!(handle(&mut a, key(KeyCode::Esc)).is_none());
        assert!(
            a.query.is_empty() && a.scope.is_some(),
            "first esc clears the search"
        );
        assert_eq!(a.esc_label(), "all projects");
        assert!(handle(&mut a, key(KeyCode::Esc)).is_none());
        assert!(a.scope.is_none(), "second esc leaves the project");
        assert_eq!(a.esc_label(), "quit");
        assert!(matches!(
            handle(&mut a, key(KeyCode::Esc)),
            Some(Action::Quit)
        ));
    }

    #[test]
    fn arrows_wrap_around_and_pages_stop_at_the_ends() {
        let ps = projects();
        let mut a = app(&ps, &opts(None, true));
        handle(&mut a, key(KeyCode::Down));
        handle(&mut a, key(KeyCode::PageDown));
        assert_eq!(a.list.selected(), Some(4), "a page stops at the end");
        handle(&mut a, key(KeyCode::Home));
        assert_eq!(a.list.selected(), Some(0));
        handle(&mut a, key(KeyCode::End));
        assert_eq!(a.list.selected(), Some(4));
        handle(&mut a, ctrl('p'));
        assert_eq!(a.list.selected(), Some(3));
        handle(&mut a, ctrl('n'));
        handle(&mut a, key(KeyCode::Down));
        assert_eq!(
            a.list.selected(),
            Some(0),
            "↓ on the last row wraps to the top"
        );
        handle(&mut a, key(KeyCode::Up));
        assert_eq!(
            a.list.selected(),
            Some(4),
            "↑ on the first row wraps to the bottom"
        );
        assert_eq!(a.focus, None, "only Tab reaches the buttons");
        handle(&mut a, key(KeyCode::PageUp));
        assert_eq!(a.list.selected(), Some(0));
    }

    #[test]
    fn toolbar_is_keyboard_complete() {
        let ps = projects();
        let mut a = app(&ps, &opts(None, true));
        handle(&mut a, key(KeyCode::Tab));
        assert_eq!(a.focus, Some(0));
        assert!(
            render(&mut a, 120, 20).contains("press"),
            "footer explains the focused mode"
        );
        handle(&mut a, key(KeyCode::Right));
        assert_eq!(a.focus, Some(1));
        handle(&mut a, key(KeyCode::Left));
        assert!(handle(&mut a, key(KeyCode::Enter)).is_none());
        assert!(
            matches!(a.mode, Mode::Spaces(_)),
            "Folders opens the switcher"
        );
        handle(&mut a, key(KeyCode::Esc));
        assert!(matches!(a.mode, Mode::Search));
        handle(&mut a, key(KeyCode::Tab));
        handle(&mut a, key(KeyCode::Tab));
        assert_eq!(a.focus, Some(1));
        assert!(matches!(
            handle(&mut a, key(KeyCode::Char(' '))),
            Some(Action::Quit)
        ));
        handle(&mut a, key(KeyCode::BackTab));
        assert_eq!(a.focus, Some(0));
        handle(&mut a, key(KeyCode::Tab));
        handle(&mut a, key(KeyCode::Tab));
        assert_eq!(
            a.focus, None,
            "tab past the last button returns to the list"
        );
        handle(&mut a, key(KeyCode::BackTab));
        handle(&mut a, key(KeyCode::Down));
        assert_eq!(a.focus, None);
        handle(&mut a, key(KeyCode::Tab));
        typed(&mut a, "b");
        assert_eq!(a.focus, None, "typing goes back to the search");
        assert_eq!(a.query, "b");
        handle(&mut a, key(KeyCode::Tab));
        handle(&mut a, key(KeyCode::Esc));
        assert_eq!(a.focus, None);
    }

    #[test]
    fn scope_limits_to_one_project_and_backspace_widens() {
        let ps = projects();
        let mut a = app(&ps, &opts(Some("/ws/billing"), true));
        assert_eq!(a.matches.len(), 2);
        typed(&mut a, "t");
        handle(&mut a, key(KeyCode::Backspace));
        assert!(a.scope.is_some(), "first backspace only edits the query");
        handle(&mut a, key(KeyCode::Backspace));
        assert!(a.scope.is_none());
        assert_eq!(a.matches.len(), 5);
    }

    #[test]
    fn scoped_picker_starts_on_the_last_run_task() {
        let ps = projects();
        let mut st = State::default();
        st.record(std::path::Path::new("/ws/billing"), "vet", Some(0));
        let a = App::new(&ps, &st, Duration::ZERO, &opts(Some("/ws/billing"), true));
        assert_eq!(selected(&a), ("billing".into(), Some("vet".into())));
    }

    #[test]
    fn right_arrow_collects_arguments() {
        let ps = projects();
        let mut a = app(&ps, &opts(None, true));
        typed(&mut a, "web dev");
        handle(&mut a, key(KeyCode::Right));
        assert!(matches!(a.mode, Mode::Args(_)));
        typed(&mut a, "--port 4000 \"a b\"x");
        handle(&mut a, key(KeyCode::Backspace));
        let r = handle(&mut a, key(KeyCode::Enter));
        assert!(matches!(r, Some(Action::Run(0, 0, ref v)) if v == &["--port", "4000", "a b"]));
        assert!(matches!(a.mode, Mode::Search));
    }

    #[test]
    fn args_mode_reports_unclosed_quotes_and_can_be_cancelled() {
        let ps = projects();
        let mut a = app(&ps, &opts(None, true));
        handle(&mut a, key(KeyCode::Right));
        typed(&mut a, "'open");
        assert!(handle(&mut a, key(KeyCode::Enter)).is_none());
        assert!(a.status.as_ref().unwrap().1.contains("quote"));
        handle(&mut a, key(KeyCode::Esc));
        assert!(matches!(a.mode, Mode::Search));
    }

    #[test]
    fn project_without_tasks_explains_instead_of_running() {
        let ps = projects();
        let mut a = app(&ps, &opts(None, true));
        typed(&mut a, "notes");
        assert_eq!(selected(&a), ("notes".into(), None));
        assert!(handle(&mut a, key(KeyCode::Enter)).is_none());
        assert!(
            a.status
                .as_ref()
                .unwrap()
                .1
                .contains(&format!("{} path notes", crate::me()))
        );
        handle(&mut a, key(KeyCode::Right));
        assert!(
            matches!(a.mode, Mode::Search),
            "no args prompt without a task"
        );
    }

    #[test]
    fn key_release_events_are_ignored() {
        let ps = projects();
        let mut a = app(&ps, &opts(None, true));
        let mut k = KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE);
        k.kind = KeyEventKind::Release;
        k.state = KeyEventState::NONE;
        assert!(handle(&mut a, Event::Key(k)).is_none());
        assert!(a.query.is_empty());
    }

    #[test]
    fn mouse_scroll_hover_click_and_double_click() {
        let ps = projects();
        let mut a = app(&ps, &opts(None, true));
        render(&mut a, 120, 20);
        let first = first_row(&a);
        handle(&mut a, mouse(MouseEventKind::ScrollDown, first));
        assert_eq!(a.list.selected(), Some(3));
        handle(&mut a, mouse(MouseEventKind::ScrollUp, first));
        assert_eq!(a.list.selected(), Some(0));
        handle(&mut a, mouse(MouseEventKind::Moved, first + 1));
        assert_eq!(a.hover, Some(1));
        assert!(render(&mut a, 120, 20).contains("○ "), "hover marker");
        assert!(
            handle(
                &mut a,
                mouse(MouseEventKind::Down(MouseButton::Left), first + 1)
            )
            .is_none()
        );
        assert_eq!(a.list.selected(), Some(1));
        let r = handle(
            &mut a,
            mouse(MouseEventKind::Down(MouseButton::Left), first + 1),
        );
        assert!(matches!(r, Some(Action::Run(..))));
        assert!(handle(&mut a, mouse(MouseEventKind::Down(MouseButton::Left), 0)).is_none());
        handle(&mut a, mouse(MouseEventKind::Moved, 0));
        assert_eq!(a.hover, None);
    }

    fn click(a: &mut App, b: Button) -> Option<Action> {
        let r = hit(a, b);
        handle(
            a,
            mouse_at(MouseEventKind::Down(MouseButton::Left), r.x, r.y),
        )
    }

    #[test]
    fn footer_keys_are_buttons() {
        let ps = projects();
        let mut a = app(&ps, &opts(None, true));
        handle(&mut a, key(KeyCode::Down));
        // Narrow: no details pane, the footer still reaches every action.
        let s = render(&mut a, 80, 20);
        assert!(s.contains("⏎ run") && s.contains("→ args") && s.contains("esc quit"));
        let run = hit(&a, Button::Run);
        handle(&mut a, mouse_at(MouseEventKind::Moved, run.x + 1, run.y));
        assert_eq!(a.hover_button, Some(Button::Run));
        assert!(matches!(
            click(&mut a, Button::Run),
            Some(Action::Run(0, 1, _))
        ));
        render(&mut a, 80, 20);
        assert!(click(&mut a, Button::Args).is_none());
        assert!(matches!(a.mode, Mode::Args(_)));
        handle(&mut a, key(KeyCode::Esc));
        typed(&mut a, "bil");
        render(&mut a, 80, 20);
        assert!(click(&mut a, Button::Back).is_none());
        assert!(a.query.is_empty(), "esc button clears the search first");
        render(&mut a, 80, 20);
        assert!(matches!(click(&mut a, Button::Back), Some(Action::Quit)));
    }

    #[test]
    fn header_buttons_are_clickable() {
        let ps = projects();
        let mut a = app(&ps, &opts(None, true));
        let s = render(&mut a, 120, 20);
        assert!(s.contains(" Folders ") && s.contains(" Quit "), "{s}");
        assert!(click(&mut a, Button::Folders).is_none());
        assert!(matches!(a.mode, Mode::Spaces(_)));
        render(&mut a, 120, 20);
        assert!(
            click(&mut a, Button::Back).is_none(),
            "the footer's esc closes it"
        );
        assert!(matches!(a.mode, Mode::Search));
        render(&mut a, 120, 20);
        let exit = hit(&a, Button::Quit);
        assert!(matches!(
            handle(
                &mut a,
                mouse_at(MouseEventKind::Down(MouseButton::Left), exit.x + 1, exit.y)
            ),
            Some(Action::Quit)
        ));
    }

    #[test]
    fn right_click_asks_for_arguments_and_never_runs() {
        let ps = projects();
        let mut a = app(&ps, &opts(None, true));
        render(&mut a, 120, 20);
        let row = a.view.rows[2].0; // billing test
        assert!(handle(&mut a, mouse(MouseEventKind::Down(MouseButton::Right), row)).is_none());
        assert_eq!(selected(&a), ("billing".into(), Some("test".into())));
        assert!(matches!(a.mode, Mode::Args(_)));
        handle(&mut a, key(KeyCode::Esc));
        typed(&mut a, "notes");
        render(&mut a, 120, 20);
        let row = first_row(&a);
        handle(&mut a, mouse(MouseEventKind::Down(MouseButton::Right), row));
        assert!(matches!(a.mode, Mode::Search), "no prompt without a task");
    }

    #[test]
    fn clicks_wait_for_the_pointer_to_move_after_opening() {
        let ps = projects();
        let mut a = app(&ps, &opts(None, true));
        render(&mut a, 120, 20);
        let row = first_row(&a);
        a.armed = false;
        a.opened = Instant::now();
        let click = mouse(MouseEventKind::Down(MouseButton::Left), row);
        assert!(handle(&mut a, click.clone()).is_none());
        assert!(
            handle(&mut a, click.clone()).is_none(),
            "a double-click lands on nothing"
        );
        handle(&mut a, mouse(MouseEventKind::Moved, row));
        assert!(matches!(
            handle(&mut a, click.clone()),
            Some(Action::Run(..))
        ));
        a.armed = false;
        handle(&mut a, key(KeyCode::Down));
        handle(&mut a, key(KeyCode::Up));
        assert!(a.armed, "a key press arms clicks too");
        a.armed = false;
        a.opened = Instant::now() - ARM_FALLBACK;
        assert!(
            matches!(handle(&mut a, click), Some(Action::Run(..))),
            "terminals without motion reports get a delay"
        );
    }

    #[test]
    fn mouse_is_ignored_when_disabled() {
        let ps = projects();
        let mut a = app(&ps, &opts(None, false));
        render(&mut a, 120, 20);
        let top = first_row(&a);
        handle(&mut a, mouse(MouseEventKind::ScrollDown, top));
        assert_eq!(a.list.selected(), Some(0));
        assert!(
            !render(&mut a, 120, 20).contains("click"),
            "footer hides mouse hint"
        );
    }

    #[test]
    fn recent_group_and_last_results() {
        let ps = projects();
        let mut st = State::default();
        st.record(std::path::Path::new("/ws/billing"), "vet", Some(0));
        st.record(std::path::Path::new("/ws/billing"), "test", Some(2));
        let mut a = App::new(&ps, &st, Duration::ZERO, &opts(None, true));
        assert_eq!(selected(&a).0, "billing");
        let s = render(&mut a, 130, 24);
        assert!(
            s.contains("recent ───") && s.contains("all tasks ───"),
            "{s}"
        );
        assert!(s.contains("✔ just now") && s.contains("✘ just now"), "{s}");
        // Clicking a row under a group title still selects the right match.
        let row = a.view.rows[2];
        assert_eq!(row.1, 2);
        typed(&mut a, "bil test");
        let s = render(&mut a, 130, 24);
        assert!(!s.contains("recent ───"), "no groups while searching");
        assert!(
            s.contains("last run") && s.contains("exit 2") && s.contains("1 run"),
            "{s}"
        );
    }

    #[test]
    fn runs_older_than_the_window_leave_the_recent_group() {
        let ps = projects();
        let mut st = State::default();
        st.record(std::path::Path::new("/ws/billing"), "vet", Some(0));
        st.record(std::path::Path::new("/ws/apps/web"), "dev", Some(0));
        let stale = st.history.iter_mut().find(|e| e.task == "dev").unwrap();
        stale.last = stale.last.saturating_sub(RECENT_WINDOW + 60);
        let a = App::new(&ps, &st, Duration::ZERO, &opts(None, true));
        assert_eq!(a.recent, 1, "only the run from today");
    }

    #[test]
    fn renders_header_rows_details_and_footer() {
        let ps = projects();
        let mut a = app(&ps, &opts(None, true));
        let s = render(&mut a, 130, 24);
        assert!(
            s.contains(" kiwa ") && s.contains("3 projects · 4 tasks · scanned in 12 ms"),
            "{s}"
        );
        assert!(
            s.contains("──────────") && s.contains("/ws") && !s.contains("╭"),
            "one-line header with roots, then a rule"
        );
        assert!(s.contains("1/5"), "position in the list: {s}");
        assert!(s.contains("apps/web") && s.contains("billing"));
        assert!(s.contains("no tasks · git"));
        assert!(s.contains("$ npm run dev") && s.contains("branch") && s.contains("main"));
        assert!(s.contains("script") && s.contains("vite"));
        assert!(s.contains("detected") && s.contains("node · package-lock.json"));
        assert!(s.contains("ℹ dependencies not installed"));
        assert!(
            s.contains("→")
                && s.contains("args")
                && s.contains("esc quit")
                && s.contains("test note")
        );
    }

    #[test]
    fn details_show_hooks_warnings_and_projects_without_tasks() {
        let ps = projects();
        let mut a = app(&ps, &opts(None, true));
        typed(&mut a, "web build");
        let s = render(&mut a, 130, 24);
        assert!(s.contains("hooks") && s.contains("prebuild"), "{s}");
        handle(&mut a, ctrl('u'));
        typed(&mut a, "bil test");
        let s = render(&mut a, 130, 24);
        assert!(
            s.contains("⚠ package.json: invalid JSON") && s.contains("feature/x"),
            "{s}"
        );
        handle(&mut a, ctrl('u'));
        typed(&mut a, "notes");
        assert!(render(&mut a, 130, 24).contains("no tasks detected"));
    }

    #[test]
    fn long_lists_scroll_and_the_scrollbar_pages() {
        let mut ps = projects();
        for i in 0..30 {
            ps[1].tasks.push(Task::new(format!("t{i}"), "go", &["go"]));
        }
        let has = |a: &App, b: Button| a.view.hits.iter().any(|(_, x)| *x == b);
        for w in [120u16, 80] {
            let mut a = app(&ps, &opts(None, true));
            let s = render(&mut a, w, 16);
            assert!(s.contains("┃"), "scrollbar thumb at {w}: {s}");
            assert!(has(&a, Button::PageDown) && !has(&a, Button::PageUp));
            click(&mut a, Button::PageDown);
            assert_eq!(
                a.list.selected(),
                Some(10),
                "the track below the thumb pages"
            );
            handle(&mut a, key(KeyCode::End));
            render(&mut a, w, 16);
            assert!(has(&a, Button::PageUp) && !has(&a, Button::PageDown));
            click(&mut a, Button::PageUp);
            assert!(a.list.selected().unwrap() < a.matches.len() - 1);
        }
    }

    #[test]
    fn narrow_terminal_hides_details() {
        let ps = projects();
        let mut a = app(&ps, &opts(None, true));
        let s = render(&mut a, 80, 20);
        assert!(!s.contains("folder"));
        assert!(s.contains("apps/web"));
    }

    #[test]
    fn renders_prompts_scope_args_status_and_empty_states() {
        let ps = projects();
        let mut a = app(&ps, &opts(Some("/ws/billing"), true));
        let s = render(&mut a, 120, 20);
        assert!(
            s.contains("billing ❯ type to filter tasks") && s.contains("/ws / billing"),
            "{s}"
        );
        handle(&mut a, key(KeyCode::Right));
        let s = render(&mut a, 120, 20);
        assert!(
            s.contains("billing · test  args ❯") && s.contains("cancel"),
            "{s}"
        );
        a.status = Some((true, "✔ done".into()));
        assert!(render(&mut a, 120, 20).contains("✔ done"));
        handle(&mut a, key(KeyCode::Esc));
        typed(&mut a, "zzzz");
        assert!(render(&mut a, 120, 20).contains("No match for “zzzz”"));
        let none: Vec<Project> = vec![];
        let mut e = app(&none, &opts(None, true));
        let s = render(&mut e, 120, 12);
        assert!(
            s.contains("No projects found here") && s.contains(" Folders "),
            "{s}"
        );
        assert!(s.contains("no match"));
    }

    #[test]
    fn matched_characters_are_underlined() {
        let ps = projects();
        let mut a = app(&ps, &opts(None, true));
        typed(&mut a, "bil");
        let mut t = Terminal::new(TestBackend::new(120, 20)).unwrap();
        t.draw(|f| draw(f, &mut a)).unwrap();
        let buf = t.backend().buffer();
        let y = a.view.rows[0].0;
        let underlined: String = (0..60)
            .filter(|&x| buf[(x, y)].modifier.contains(Modifier::UNDERLINED))
            .map(|x| buf[(x, y)].symbol().to_string())
            .collect();
        assert_eq!(underlined, "bil");
    }

    /// The matches of the task name never underline the padding of a short project id.
    #[test]
    fn task_matches_stay_in_the_task_column() {
        let mut ps = projects();
        ps[1].id = "bil".into(); // padded to the width of `apps/web`
        let mut a = app(&ps, &opts(None, true));
        typed(&mut a, "bil te");
        let mut t = Terminal::new(TestBackend::new(120, 20)).unwrap();
        t.draw(|f| draw(f, &mut a)).unwrap();
        let buf = t.backend().buffer();
        let y = a.view.rows[0].0;
        let underlined: String = (0..60)
            .filter(|&x| buf[(x, y)].modifier.contains(Modifier::UNDERLINED))
            .map(|x| buf[(x, y)].symbol().to_string())
            .collect();
        assert_eq!(underlined, "bilte", "no underlined spaces");
    }

    #[test]
    fn repeated_project_names_are_faint() {
        let ps = projects();
        let mut a = app(&ps, &opts(None, true));
        let mut t = Terminal::new(TestBackend::new(120, 20)).unwrap();
        t.draw(|f| draw(f, &mut a)).unwrap();
        let buf = t.backend().buffer().clone();
        let y = a.view.rows[0].0;
        let dim = |row: u16| buf[(2, y + row)].modifier.contains(Modifier::DIM);
        assert!(dim(1), "continuation row is faint");
        assert!(!dim(2), "first row of a new project is not");
    }

    #[test]
    fn wide_characters_keep_columns_aligned() {
        let mut ps = projects();
        ps[0].id = "日本/web".into();
        let mut a = app(&ps, &opts(None, true));
        let mut t = Terminal::new(TestBackend::new(120, 20)).unwrap();
        t.draw(|f| draw(f, &mut a)).unwrap();
        let buf = t.backend().buffer().clone();
        // Terminal column where a task name starts, read cell by cell (a wide character
        // occupies two cells, the second one being its continuation).
        let start_of = |y: u16, word: &str| {
            let first = word.chars().next().unwrap().to_string();
            (0..60u16)
                .find(|&x| buf[(x, y)].symbol() == first && buf[(x - 1, y)].symbol() == " ")
                .unwrap()
        };
        let web_row = a.view.rows[0].0; // 日本/web › dev
        let billing_row = a.view.rows[2].0; // billing › test
        assert_eq!(start_of(web_row, "dev"), start_of(billing_row, "test"));
        assert_eq!(truncate_left("日本語テキスト", 7), "…キスト");
    }
    #[test]
    fn renders_cleanly_at_every_width_from_60_to_200() {
        let ps = projects();
        let mut st = State::default();
        st.record(std::path::Path::new("/ws/billing"), "vet", Some(1));
        for w in 60u16..=200 {
            let mut a = App::new(&ps, &st, Duration::from_millis(9), &opts(None, true));
            a.status = Some((false, "✘ billing vet · exit 1 · 0.4s".into()));
            let s = render(&mut a, w, 14);
            for line in s.lines() {
                assert_eq!(
                    UnicodeWidthStr::width(line),
                    w as usize,
                    "overflow at {w}: {line}"
                );
            }
            assert!(
                s.contains(" kiwa ") && s.contains("apps/web"),
                "at {w}: {s}"
            );
            assert!(
                s.contains("Quit") && s.contains("⏎ run"),
                "buttons at {w}: {s}"
            );
            assert_eq!(
                s.contains("$ go vet"),
                w >= 100,
                "details pane from 100 columns"
            );
        }
    }

    #[test]
    fn no_color_theme_still_shows_structure() {
        let ps = projects();
        let mut a = app(&ps, &opts(None, true));
        a.theme = Theme::new(None, false);
        a.focus = Some(0);
        let mut t = Terminal::new(TestBackend::new(120, 18)).unwrap();
        t.draw(|f| draw(f, &mut a)).unwrap();
        let buf = t.backend().buffer();
        let b = hit(&a, Button::Folders);
        assert!(
            buf[(b.x + 1, b.y)].modifier.contains(Modifier::REVERSED),
            "focused button is reversed, an attribute NO_COLOR keeps"
        );
        assert_eq!(buf[(0, 0)].fg, ratatui::style::Color::Reset, "no colour");
        assert!(buf[(1, 0)].modifier.contains(Modifier::REVERSED), "badge");
        let y = a.view.rows[0].0;
        assert_eq!(buf[(0, y)].symbol(), "●", "selection marker without colour");
    }

    fn project(id: &str, tasks: &[&str]) -> Project {
        Project {
            id: id.into(),
            name: id.rsplit('/').next().unwrap().into(),
            path: PathBuf::from(format!("/ws/{id}")),
            ecosystems: vec![Ecosystem::Node],
            branch: None,
            default_branch: None,
            tasks: tasks
                .iter()
                .map(|t| Task::new(*t, "npm", &["npm", "run", t]))
                .collect(),
            warnings: vec![],
            hints: vec![],
            detected_from: vec![],
            member_of: None,
        }
    }

    /// A monorepo `shop` with three packages (one nested two levels down), a sibling whose
    /// name sorts between the root and its packages, and an unrelated project.
    fn monorepo() -> Vec<Project> {
        vec![
            project("billing", &["test"]),
            project("shop", &["dev", "build"]),
            project("shop-tools", &["lint"]),
            project("shop/apps/admin", &["dev"]),
            project("shop/apps/admin/e2e", &["test"]),
            project("shop/apps/web", &["dev", "test"]),
        ]
    }

    fn rows(a: &App) -> Vec<String> {
        a.matches
            .iter()
            .map(|&i| {
                let it = &a.items[i];
                let p = &a.projects[it.p];
                format!("{} {}", p.id, p.tasks[it.t.unwrap()].name)
            })
            .collect()
    }

    #[test]
    fn tree_of_finds_roots_depths_and_paths() {
        let ps = monorepo();
        let t = tree_of(&ps);
        assert_eq!((t[1].root, t[1].depth, t[1].members), (1, 0, 3));
        assert_eq!(
            (t[3].root, t[3].depth, t[3].rel.as_str()),
            (1, 1, "apps/admin")
        );
        assert_eq!(
            (t[4].root, t[4].depth, t[4].rel.as_str()),
            (1, 2, "apps/admin/e2e")
        );
        assert_eq!(
            (t[2].root, t[2].members),
            (2, 0),
            "a name prefix is not a folder"
        );
        assert_eq!(
            tree_path_cmp("apps/admin/e2e", "apps/web"),
            std::cmp::Ordering::Less
        );
    }

    #[test]
    fn declared_members_win_and_excluded_folders_stay_apart() {
        let mut ps = monorepo();
        let member = |p: &mut Project| {
            p.member_of = Some(kiwa_core::model::Membership {
                root: "shop".into(),
                path: PathBuf::from("/ws/shop"),
                from: "pnpm-workspace.yaml".into(),
            })
        };
        member(&mut ps[3]); // shop/apps/admin
        member(&mut ps[5]); // shop/apps/web
        ps.push(project("shop/legacy", &["old"])); // inside shop, not declared by it
        let t = tree_of(&ps);
        assert_eq!((t[3].root, t[3].depth), (1, 1));
        assert_eq!(
            (t[4].root, t[4].depth),
            (1, 2),
            "an undeclared package of a member falls back to its folder"
        );
        assert_eq!(
            t[6].root, 6,
            "shop declares its members and left this one out"
        );
        assert_eq!(t[1].members, 3);
        let mut a = app(&ps, &opts(None, true));
        for _ in 0..3 {
            handle(&mut a, key(KeyCode::Down));
        }
        let s = render(&mut a, 130, 24);
        assert!(s.contains("monorepo shop · pnpm-workspace.yaml"), "{s}");
    }

    #[test]
    fn browsing_lists_packages_under_their_root() {
        let ps = monorepo();
        let mut a = app(&ps, &opts(None, true));
        assert_eq!(
            rows(&a),
            [
                "billing test",
                "shop dev",
                "shop build",
                "shop/apps/admin dev",
                "shop/apps/admin/e2e test",
                "shop/apps/web dev",
                "shop/apps/web test",
                "shop-tools lint",
            ]
        );
        let s = render(&mut a, 120, 20);
        assert!(s.contains("▾ shop "), "{s}");
        assert!(s.contains("    apps/admin "), "packages are indented: {s}");
        assert!(s.contains("      apps/admin/e2e "), "{s}");
        assert!(!s.contains("← fold"), "billing has no packages: {s}");
        handle(&mut a, key(KeyCode::Down));
        let s = render(&mut a, 120, 20);
        assert!(s.contains("packages 3"), "details of the root: {s}");
        handle(&mut a, key(KeyCode::Down));
        handle(&mut a, key(KeyCode::Down));
        let s = render(&mut a, 120, 20);
        assert!(s.contains("monorepo shop"), "details of a package: {s}");
        assert!(s.contains("← fold"), "{s}");
    }

    #[test]
    fn left_folds_and_right_unfolds_a_monorepo() {
        let ps = monorepo();
        let mut a = app(&ps, &opts(None, true));
        for _ in 0..4 {
            handle(&mut a, key(KeyCode::Down));
        }
        assert_eq!(
            selected(&a),
            ("shop/apps/admin/e2e".into(), Some("test".into()))
        );
        handle(&mut a, key(KeyCode::Left));
        assert_eq!(
            selected(&a),
            ("shop".into(), Some("dev".into())),
            "selection moves to the group"
        );
        assert_eq!(rows(&a), ["billing test", "shop dev", "shop-tools lint"]);
        let s = render(&mut a, 120, 20);
        assert!(s.contains("▸ shop +5"), "{s}");
        assert!(s.contains("→ unfold") && !s.contains("← fold"), "{s}");
        handle(&mut a, key(KeyCode::Left));
        assert_eq!(a.matches.len(), 3, "folding twice changes nothing");
        handle(&mut a, key(KeyCode::Right));
        assert!(
            matches!(a.mode, Mode::Search),
            "→ unfolds instead of asking for arguments"
        );
        assert_eq!(a.matches.len(), 8);
        assert_eq!(selected(&a), ("shop".into(), Some("dev".into())));
        handle(&mut a, key(KeyCode::Right));
        assert!(
            matches!(a.mode, Mode::Args(_)),
            "on an open group → asks for arguments"
        );
        handle(&mut a, key(KeyCode::Esc));
        handle(&mut a, key(KeyCode::Home));
        handle(&mut a, key(KeyCode::Left));
        assert_eq!(
            a.matches.len(),
            8,
            "a project without packages does not fold"
        );
    }

    #[test]
    fn clicking_the_marker_folds_and_unfolds() {
        let ps = monorepo();
        let mut a = app(&ps, &opts(None, true));
        render(&mut a, 120, 20);
        let m = hit(&a, Button::Fold(1));
        handle(
            &mut a,
            mouse_at(MouseEventKind::Down(MouseButton::Left), m.x, m.y),
        );
        assert_eq!(a.matches.len(), 3);
        render(&mut a, 120, 20);
        let m = hit(&a, Button::Fold(1));
        handle(
            &mut a,
            mouse_at(MouseEventKind::Down(MouseButton::Left), m.x, m.y),
        );
        assert_eq!(a.matches.len(), 8);
    }

    #[test]
    fn search_and_scope_show_a_flat_list() {
        let ps = monorepo();
        let mut a = app(&ps, &opts(None, true));
        handle(&mut a, key(KeyCode::Down));
        handle(&mut a, key(KeyCode::Left));
        typed(&mut a, "e2e");
        assert_eq!(
            rows(&a),
            ["shop/apps/admin/e2e test"],
            "folded packages are still found"
        );
        let s = render(&mut a, 120, 20);
        assert!(s.contains("shop/apps/admin/e2e") && !s.contains("▸"), "{s}");
        handle(&mut a, key(KeyCode::Left));
        assert_eq!(a.matches.len(), 1, "no folding while searching");
        handle(&mut a, key(KeyCode::Esc));
        assert_eq!(a.matches.len(), 3, "the fold is remembered");
        let b = App::new(
            &ps,
            &State::default(),
            Duration::ZERO,
            &opts(Some("/ws/shop"), true),
        );
        assert_eq!(b.matches.len(), 2, "a scoped picker lists one project only");
    }

    #[test]
    fn recent_rows_stay_flat_above_the_tree() {
        let ps = monorepo();
        let mut st = State::default();
        st.record(std::path::Path::new("/ws/shop"), "dev", Some(0));
        st.record(std::path::Path::new("/ws/shop"), "build", Some(0));
        let mut a = App::new(&ps, &st, Duration::ZERO, &opts(None, true));
        assert_eq!(a.recent, 2);
        // The group keeps the place of its first row left below RECENT.
        assert_eq!(
            &rows(&a)[2..5],
            ["billing test", "shop-tools lint", "shop/apps/admin dev"]
        );
        let s = render(&mut a, 120, 20);
        assert!(
            s.contains("recent ───") && s.contains("all tasks ───"),
            "{s}"
        );
        // Every root row is in RECENT: the group opens on its first package, by full id.
        assert!(s.contains("▾ shop/apps/admin "), "{s}");
        handle(&mut a, key(KeyCode::Left));
        assert_eq!(a.matches.len(), 8, "RECENT rows do not fold");
        for _ in 0..4 {
            handle(&mut a, key(KeyCode::Down));
        }
        handle(&mut a, key(KeyCode::Left));
        assert_eq!(rows(&a)[4], "shop/apps/admin dev");
        assert_eq!(a.matches.len(), 5);
    }

    /// `cargo xtask snapshot` (or `cargo test -p kiwa --lib -- --ignored --nocapture snapshot`)
    /// prints the picker on the demo workspace, for reviews and the README.
    #[test]
    #[ignore]
    fn snapshot_demo() {
        let demo = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/demo");
        let ps = kiwa_core::discover(&kiwa_core::DiscoverOptions {
            roots: vec![demo],
            ..Default::default()
        });
        let mut st = State::default();
        st.record(&ps[0].path, &ps[0].tasks[0].name, Some(0));
        let mut o = opts(None, true);
        o.roots = vec![PathBuf::from("~/dev")];
        o.note = None;
        let mut a = App::new(&ps, &st, Duration::from_millis(17), &o);
        println!("{}", render(&mut a, 120, 24));
        typed(&mut a, "api te");
        println!("{}", render(&mut a, 120, 14));
    }

    #[test]
    fn result_lines() {
        assert_eq!(
            result_line("web build", 0, 4.21).1,
            "✔ web build · ok · 4.2s"
        );
        assert!(
            result_line("web build", 1, 0.8)
                .1
                .starts_with("✘ web build · exit 1")
        );
        assert!(!result_line("web build", 1, 0.8).0);
        let r = ruled("✔ x · ok · 1.0s");
        assert!(r.starts_with("── ✔ x · ok · 1.0s ─"), "{r}");
    }

    #[test]
    fn ansi_painter_respects_switch() {
        let off = Ansi { on: false };
        let th = Theme::new(Some("amber"), true);
        assert_eq!(off.paint("x", th.accent), "x");
        let on = Ansi { on: true };
        assert_eq!(on.paint("x", th.accent), "\x1b[1;38;5;214mx\x1b[0m");
        assert_eq!(on.paint("x", th.ok), "\x1b[32mx\x1b[0m");
        assert_eq!(on.paint("x", Style::new()), "x");
    }

    /// Keystroke budget on a polyglot workspace shaped like a real machine: 10 domains × 6
    /// kinds of project, each with the tasks its ecosystem usually has, so names and tasks
    /// collide the way they do in practice (`api` next to `api-gateway`, `test` almost
    /// everywhere). For each of the 190 tasks, finds the shortest input that puts it under
    /// the cursor: characters typed plus ↓ presses, Enter excluded.
    fn keystrokes_to_reach_every_task() -> (usize, f64, String) {
        let domains = [
            "web", "api", "auth", "billing", "search", "mobile", "admin", "infra", "docs", "data",
        ];
        let kinds: [(&str, &str, &[&str]); 6] = [
            ("", "npm", &["dev", "build", "test", "lint"]),
            ("-gateway", "cargo", &["build", "test", "clippy", "run"]),
            ("-worker", "go", &["build", "test", "vet"]),
            ("-sdk", "uv", &["test", "lint", "publish"]),
            ("-e2e", "npm", &["test", "test:ui"]),
            ("-ops", "make", &["deploy", "plan", "clean"]),
        ];
        let ps: Vec<Project> = domains
            .iter()
            .flat_map(|d| {
                kinds.iter().map(move |(suffix, runner, tasks)| {
                    let id = format!("{d}{suffix}");
                    Project {
                        id: id.clone(),
                        name: id.clone(),
                        path: PathBuf::from(format!("/ws/{id}")),
                        ecosystems: vec![Ecosystem::Node],
                        branch: None,
                        default_branch: None,
                        tasks: tasks
                            .iter()
                            .map(|t| Task::new(*t, runner, &[*runner, *t]))
                            .collect(),
                        warnings: vec![],
                        hints: vec![],
                        detected_from: vec![],
                        member_of: None,
                    }
                })
            })
            .collect();
        assert_eq!(ps.iter().map(|p| p.tasks.len()).sum::<usize>(), 190);
        let a = app(&ps, &opts(None, false));
        let hay: Vec<&str> = a.items.iter().map(|i| i.hay.as_str()).collect();
        let (mut worst, mut total, mut worst_target) = (0usize, 0usize, String::new());
        for (target, it) in a.items.iter().enumerate() {
            let p = &ps[it.p];
            let t = &p.tasks[it.t.unwrap()].name;
            // What people type: the start of each segment of the project name (`au ap` for
            // `auth-api`), with or without spaces, then the start of the task name.
            let segments: Vec<Vec<char>> =
                p.id.split(['-', '/', '_'])
                    .map(|s| s.chars().collect())
                    .collect();
            let mut id_queries: Vec<String> = vec![String::new()];
            for seg in &segments {
                let mut next = Vec::new();
                for q in &id_queries {
                    for n in 0..=seg.len() {
                        let part: String = seg[..n].iter().collect();
                        next.push(format!("{q}{part}"));
                        if !q.is_empty() && n > 0 {
                            next.push(format!("{q} {part}"));
                        }
                    }
                }
                id_queries = next;
            }
            let task: Vec<char> = t.chars().collect();
            let mut best = usize::MAX;
            for idq in &id_queries {
                for j in 0..=task.len() {
                    let mut q = idq.clone();
                    if j > 0 {
                        if !q.is_empty() {
                            q.push(' ');
                        }
                        q.extend(task[..j].iter());
                    }
                    if q.is_empty() {
                        continue;
                    }
                    if let Some(pos) = fuzzy::rank(&q, &hay).iter().position(|&k| k == target) {
                        best = best.min(q.chars().count() + pos);
                    }
                }
            }
            total += best;
            if best > worst {
                worst = best;
                worst_target = format!("{} {t}", p.id);
            }
        }
        (worst, total as f64 / a.items.len() as f64, worst_target)
    }

    #[test]
    fn every_task_of_a_polyglot_workspace_is_within_budget() {
        let (worst, mean, target) = keystrokes_to_reach_every_task();
        eprintln!("keystrokes: worst {worst} ({target}), mean {mean:.2}");
        assert!(
            worst <= KEYSTROKE_BUDGET,
            "worst case {worst} for `{target}` exceeds {KEYSTROKE_BUDGET}"
        );
    }

    #[test]
    fn the_switcher_lists_workspaces_with_counts_and_missing_folders() {
        let ps = projects();
        let mut a = app(&ps, &spaced());
        assert_eq!(a.counts, (3, 4), "default first");
        handle(&mut a, key(KeyCode::Tab));
        handle(&mut a, key(KeyCode::Enter));
        let s = render(&mut a, 140, 20);
        for part in [
            "all",
            "default",
            "work",
            "1 projects",
            "⚠ missing:",
            "+ add a folder…",
            "kw init",
            "(in view)",
        ] {
            assert!(s.contains(part), "{part}: {s}");
        }
        assert_eq!(
            a.switcher(),
            [
                Switch::All,
                Switch::Space(0),
                Switch::Space(1),
                Switch::Space(2),
                Switch::Add,
                Switch::Setup
            ]
        );
        assert!(
            matches!(a.mode, Mode::Spaces(1)),
            "starts on the one in view"
        );
        handle(&mut a, key(KeyCode::Down));
        assert!(handle(&mut a, key(KeyCode::Enter)).is_none());
        assert_eq!(a.space, Some(1));
        assert_eq!(a.counts, (1, 2), "billing only");
        assert!(render(&mut a, 140, 20).contains("work · /ws/billing"));
        // `all`, by mouse.
        handle(&mut a, key(KeyCode::Tab));
        handle(&mut a, key(KeyCode::Enter));
        render(&mut a, 140, 20);
        assert!(click(&mut a, Button::Switch(0)).is_none());
        assert_eq!((a.space, a.counts.0), (None, 3));
        assert!(render(&mut a, 140, 20).contains("all workspaces"));
        // Setup, wrapping upwards from the first entry.
        a.open_switcher();
        handle(&mut a, key(KeyCode::Up));
        assert!(matches!(
            handle(&mut a, key(KeyCode::Enter)),
            Some(Action::Folders)
        ));
    }

    #[test]
    fn at_name_narrows_the_search_to_a_workspace() {
        let ps = projects();
        let mut a = app(&ps, &spaced());
        typed(&mut a, "@wo");
        assert_eq!(a.matches.len(), 2, "a unique prefix");
        typed(&mut a, " ve");
        assert_eq!(selected(&a), ("billing".into(), Some("vet".into())));
        assert_eq!(a.search(), "ve");
        handle(&mut a, ctrl('u'));
        typed(&mut a, "@all web");
        assert!(a.matches.iter().all(|&i| a.items[i].p == 0));
        handle(&mut a, ctrl('u'));
        typed(&mut a, "@nope");
        assert!(a.matches.is_empty());
        assert!(render(&mut a, 120, 20).contains("no workspace “@nope”"));
        handle(&mut a, ctrl('u'));
        typed(&mut a, "@");
        assert_eq!(a.matches.len(), 5, "a lone @ keeps the workspace in view");
        // Choosing a workspace replaces the typed one and keeps the rest of the search.
        handle(&mut a, ctrl('u'));
        typed(&mut a, "@work te");
        a.open_switcher();
        let all = a.switcher().iter().position(|e| *e == Switch::All).unwrap();
        a.mode = Mode::Spaces(all);
        handle(&mut a, key(KeyCode::Enter));
        assert_eq!((a.query.as_str(), a.space), ("te", None));
    }

    #[test]
    fn add_a_folder_from_the_switcher() {
        let ps = projects();
        let mut a = app(&ps, &opts(None, true));
        a.open_switcher();
        assert_eq!(
            a.switcher(),
            [Switch::Space(0), Switch::Add, Switch::Setup],
            "no `all` alone"
        );
        handle(&mut a, key(KeyCode::Down));
        handle(&mut a, key(KeyCode::Enter));
        assert!(matches!(a.mode, Mode::AddFolder(_)));
        assert!(render(&mut a, 120, 20).contains("add a folder ❯"));
        typed(&mut a, "/surely/not/here");
        handle(&mut a, key(KeyCode::Backspace));
        assert!(handle(&mut a, key(KeyCode::Enter)).is_none());
        assert!(a.status.as_ref().unwrap().1.starts_with("not a folder"));
        handle(&mut a, key(KeyCode::Esc));
        assert!(matches!(a.mode, Mode::Spaces(1)), "back on the entry");
        handle(&mut a, key(KeyCode::Enter));
        let dir = tempfile::tempdir().unwrap();
        typed(&mut a, &format!("\"{}\"", dir.path().display()));
        match handle(&mut a, key(KeyCode::Enter)) {
            Some(Action::AddFolder(p)) => assert!(p.is_dir() && p.is_absolute()),
            _ => panic!("expected AddFolder"),
        }
        assert!(matches!(handle(&mut a, ctrl('c')), Some(Action::Quit)));
    }

    #[test]
    fn a_dev_server_shows_its_url_and_ctrl_o_opens_it() {
        let mut ps = projects();
        ps[0].tasks[0].url = Some("http://localhost:5173".into()); // apps/web dev
        let mut a = app(&ps, &opts(None, true));
        typed(&mut a, "web dev");
        let s = render(&mut a, 130, 24);
        assert!(
            s.contains("http://localhost:5173") && s.contains("^O open"),
            "{s}"
        );
        assert!(matches!(
            handle(&mut a, ctrl('o')),
            Some(Action::Open(ref u)) if u == "http://localhost:5173"
        ));
        assert!(matches!(click(&mut a, Button::Open), Some(Action::Open(_))));
        handle(&mut a, ctrl('u'));
        typed(&mut a, "bil test");
        assert!(handle(&mut a, ctrl('o')).is_none());
        assert!(a.status.as_ref().unwrap().1.contains("no known URL"));
        assert!(!render(&mut a, 130, 24).contains("^O open"));
    }

    #[test]
    fn risky_tasks_and_shell_lines_are_shown_and_confirmed() {
        let mut ps = projects();
        ps[1].tasks[0].risky = true; // billing test
        ps[1].tasks[1] = Task::from_line("vet", "go", "go vet ./... && echo ok"); // billing vet
        let mut a = app(&ps, &opts(None, true));
        typed(&mut a, "bil test");
        assert!(handle(&mut a, key(KeyCode::Enter)).is_none(), "asks first");
        assert!(matches!(a.mode, Mode::Confirm(1, 0, _)));
        let s = render(&mut a, 120, 20);
        assert!(
            s.contains("looks risky ❯ go test ./...") && s.contains("y run it"),
            "{s}"
        );
        assert!(handle(&mut a, key(KeyCode::Esc)).is_none());
        assert!(matches!(a.mode, Mode::Search), "esc cancels");
        handle(&mut a, key(KeyCode::Enter));
        assert!(matches!(
            handle(&mut a, key(KeyCode::Char('y'))),
            Some(Action::Run(1, 0, _))
        ));
        // A shell line, with arguments, confirmed with the footer button.
        handle(&mut a, ctrl('u'));
        typed(&mut a, "bil vet");
        handle(&mut a, key(KeyCode::Right));
        typed(&mut a, "-v");
        handle(&mut a, key(KeyCode::Enter));
        let s = render(&mut a, 120, 20);
        assert!(
            s.contains("runs a shell line ❯ sh -c \"go vet ./... && echo ok\" sh -v"),
            "{s}"
        );
        assert!(matches!(
            click(&mut a, Button::Run),
            Some(Action::Run(1, 1, ref v)) if v == &["-v"]
        ));
        assert!(matches!(a.mode, Mode::Search));
        handle(&mut a, key(KeyCode::Enter));
        render(&mut a, 120, 20);
        assert!(click(&mut a, Button::Back).is_none());
        assert!(matches!(a.mode, Mode::Search));
        handle(&mut a, key(KeyCode::Enter));
        assert!(matches!(handle(&mut a, ctrl('c')), Some(Action::Quit)));
    }

    #[test]
    fn a_task_that_needs_input_opens_the_prompt_labelled_with_it() {
        let mut ps = projects();
        ps[1].tasks[0].params = vec![kiwa_core::model::Param::new(
            "env",
            kiwa_core::model::ParamForm::Positional,
        )];
        let mut a = app(&ps, &opts(None, true));
        typed(&mut a, "bil test");
        assert!(
            handle(&mut a, key(KeyCode::Enter)).is_none(),
            "never runs bare"
        );
        assert!(matches!(a.mode, Mode::Args(_)));
        assert!(render(&mut a, 120, 20).contains("needs <env>"));
        assert!(handle(&mut a, key(KeyCode::Enter)).is_none());
        assert_eq!(a.status.as_ref().unwrap().1, "test needs <env>");
        typed(&mut a, "prod");
        assert!(matches!(
            handle(&mut a, key(KeyCode::Enter)),
            Some(Action::Run(1, 0, ref v)) if v == &["prod"]
        ));
    }

    #[test]
    fn branch_column_when_wide_enough_with_the_default_branch_faint() {
        let mut ps = projects();
        ps[0].branch = Some("develop".into());
        ps[0].default_branch = Some("main".into());
        ps[1].branch = Some("main".into());
        let mut a = app(&ps, &opts(None, true));
        let mut t = Terminal::new(TestBackend::new(140, 20)).unwrap();
        t.draw(|f| draw(f, &mut a)).unwrap();
        let buf = t.backend().buffer().clone();
        let line = |y: u16| -> String { (0..80).map(|x| buf[(x, y)].symbol()).collect() };
        let at = |y: u16, word: &str| line(y).find(word).map(|i| line(y)[..i].chars().count());
        let web = a.view.rows[0].0;
        let billing = a.view.rows[2].0;
        let x = at(web, "develop").expect("branch shown");
        assert_eq!(at(billing, "main"), Some(x), "one aligned column");
        assert_eq!(
            at(web + 1, "develop"),
            None,
            "only on a project's first row"
        );
        let dim = |y: u16| buf[(x as u16, y)].modifier.contains(Modifier::DIM);
        assert!(
            !dim(web),
            "develop is not this clone's default branch: it stands out"
        );
        assert!(dim(billing), "the default branch is faint");
        assert!(is_default_branch(&ps[1], "main") && !is_default_branch(&ps[0], "develop"));
        ps[1].branch = Some("feature/a-rather-long-branch-name".into());
        let mut a = app(&ps, &opts(None, true));
        let s = render(&mut a, 60, 20);
        assert!(!s.contains("develop"), "no room left for it: {s}");
        assert!(
            render(&mut a, 160, 20).contains("feature/a-r…"),
            "cut in the middle"
        );
    }

    #[test]
    fn main_branch_names() {
        assert!(is_main_branch("main") && is_main_branch("master") && is_main_branch("trunk"));
        assert!(!is_main_branch("feature/x"));
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn keeps_the_end_that_fits() {
        assert_eq!(super::truncate_left("apps/web", 20), "apps/web");
        assert_eq!(super::truncate_left("apps/storefront", 8), "…refront");
        assert_eq!(super::truncate_left("abc", 0), "…");
        assert_eq!(
            super::truncate_left("日本語", 4),
            "…語",
            "a wide character never splits"
        );
    }
}
