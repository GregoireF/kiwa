// SPDX-License-Identifier: MIT OR Apache-2.0

//! `kw doctor`: what kiwa sees on this machine. Read-only: environment variables, files and
//! `PATH` lookups only; no process is started.
//!
//! `kw doctor --probe`: an interactive check of what the terminal really delivers (clicks,
//! motion, wheel, resizing, colours), because terminals differ and the picker depends on it.
//! The summary line is meant to be pasted into docs/reference/terminals.md.

use crate::theme::Theme;
use ratatui::crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, KeyModifiers,
    MouseButton, MouseEventKind,
};
use ratatui::crossterm::execute;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::{DefaultTerminal, Frame};
use serde::Serialize;
use std::path::PathBuf;

/// Every program kiwa can run a task with; `install_hint` has a line for each.
pub const RUNNERS: &[&str] = &[
    "npm",
    "node",
    "pnpm",
    "yarn",
    "bun",
    "deno",
    "cargo",
    "go",
    "uv",
    "poetry",
    "pdm",
    "hatch",
    "poe",
    "python",
    "pytest",
    "just",
    "task",
    "mise",
    "make",
    "gradle",
    "mvn",
    "dotnet",
    "composer",
    "bundle",
    "rake",
    "docker",
    "devcontainer",
    "sh",
];

#[derive(Debug, Serialize)]
pub struct Report {
    pub kiwa: Kiwa,
    pub terminal: Terminal,
    pub config: Files,
    pub state: Files,
    pub runners: Vec<Runner>,
}

#[derive(Debug, Serialize)]
pub struct Kiwa {
    pub version: &'static str,
    pub os: &'static str,
    pub arch: &'static str,
}

#[derive(Debug, Serialize)]
pub struct Terminal {
    pub stdin_tty: bool,
    pub stdout_tty: bool,
    /// Best guess from the environment (`Windows Terminal`, `VS Code`, `mintty`, …).
    pub program: String,
    pub term: Option<String>,
    /// Columns and rows, when stdout is a terminal.
    pub size: Option<(u16, u16)>,
    pub colours: Colours,
    pub mouse: Mouse,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct Colours {
    /// Whether kiwa uses colours (`NO_COLOR` unset or empty).
    pub enabled: bool,
    /// `truecolor`, `256`, `none` or `unknown` (at least 16 is assumed).
    pub depth: &'static str,
    /// The variable that decided `depth`.
    pub from: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct Mouse {
    pub enabled: bool,
    pub reason: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct Files {
    pub path: Option<PathBuf>,
    pub exists: bool,
    /// Config: whether it loads. State: always true (a broken state file is ignored).
    pub valid: bool,
    pub error: Option<String>,
    pub warnings: Vec<String>,
    /// State only: tasks in the run history.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runs: Option<usize>,
}

#[derive(Debug, Serialize)]
pub struct Runner {
    pub name: &'static str,
    pub path: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

/// The terminal program, from the variables terminals set. `var` reads one variable.
pub fn terminal_program(var: &dyn Fn(&str) -> Option<String>) -> String {
    let has = |k: &str| var(k).is_some_and(|v| !v.is_empty());
    let term_program = var("TERM_PROGRAM").unwrap_or_default();
    let term = var("TERM").unwrap_or_default();
    let named = match term_program.as_str() {
        "vscode" => Some("VS Code"),
        "mintty" => Some("mintty"),
        "Apple_Terminal" => Some("macOS Terminal"),
        "iTerm.app" => Some("iTerm2"),
        "WezTerm" => Some("WezTerm"),
        "ghostty" => Some("Ghostty"),
        _ => None,
    };
    if let Some(n) = named {
        return n.into();
    }
    if has("WT_SESSION") {
        "Windows Terminal".into()
    } else if has("KITTY_WINDOW_ID") || term == "xterm-kitty" {
        "Kitty".into()
    } else if has("KONSOLE_VERSION") {
        "Konsole".into()
    } else if has("GNOME_TERMINAL_SCREEN") {
        "GNOME Terminal".into()
    } else if has("VTE_VERSION") {
        "VTE-based terminal".into()
    } else if has("MSYSTEM") && term_program.is_empty() {
        "mintty".into()
    } else if !term_program.is_empty() {
        term_program
    } else if cfg!(windows) {
        "Windows console (conhost) or unknown".into()
    } else {
        "unknown".into()
    }
}

/// Colour depth as the environment advertises it (terminals cannot be asked without a
/// round-trip, which `--probe` does visually instead).
pub fn colours(var: &dyn Fn(&str) -> Option<String>) -> Colours {
    let enabled = var("NO_COLOR").is_none_or(|v| v.is_empty());
    let colorterm = var("COLORTERM").unwrap_or_default().to_lowercase();
    let term = var("TERM").unwrap_or_default();
    let (depth, from) = if colorterm == "truecolor" || colorterm == "24bit" {
        ("truecolor", Some(format!("COLORTERM={colorterm}")))
    } else if var("WT_SESSION").is_some() {
        ("truecolor", Some("WT_SESSION".into()))
    } else if term.contains("256color") {
        ("256", Some(format!("TERM={term}")))
    } else if term == "dumb" {
        ("none", Some("TERM=dumb".into()))
    } else {
        ("unknown", None)
    };
    Colours {
        enabled,
        depth,
        from,
    }
}

fn env(k: &str) -> Option<String> {
    std::env::var(k).ok()
}

pub fn report(
    mouse: (bool, Option<String>),
    config: Result<&kiwa_core::config::Config, String>,
) -> Report {
    use std::io::IsTerminal;
    let stdout_tty = std::io::stdout().is_terminal();
    let config_path = kiwa_core::config::config_path();
    let config = Files {
        exists: config_path.as_ref().is_some_and(|p| p.is_file()),
        path: config_path,
        valid: config.is_ok(),
        warnings: config
            .as_ref()
            .map(|c| c.warnings.clone())
            .unwrap_or_default(),
        error: config.err(),
        runs: None,
    };
    let state_path = kiwa_core::state::state_path();
    let state = Files {
        exists: state_path.as_ref().is_some_and(|p| p.is_file()),
        path: state_path,
        valid: true,
        error: None,
        warnings: vec![],
        runs: Some(kiwa_core::state::State::load().history.len()),
    };
    let runners = RUNNERS
        .iter()
        .map(|&name| {
            let path = which::which(name).ok();
            let hint = path.is_none().then(|| crate::exec::install_hint(name));
            Runner { name, path, hint }
        })
        .collect();
    Report {
        kiwa: Kiwa {
            version: env!("CARGO_PKG_VERSION"),
            os: std::env::consts::OS,
            arch: std::env::consts::ARCH,
        },
        terminal: Terminal {
            stdin_tty: std::io::stdin().is_terminal(),
            stdout_tty,
            program: terminal_program(&env),
            term: env("TERM"),
            size: stdout_tty
                .then(ratatui::crossterm::terminal::size)
                .and_then(Result::ok),
            colours: colours(&env),
            mouse: Mouse {
                enabled: mouse.0,
                reason: mouse.1,
            },
        },
        config,
        state,
        runners,
    }
}

fn file_line(f: &Files) -> String {
    let path = f
        .path
        .as_ref()
        .map_or("(no location on this system)".into(), |p| {
            p.display().to_string()
        });
    let status = match (&f.error, f.exists, f.runs) {
        (Some(e), _, _) => format!("✘ {e}"),
        (None, false, _) => "· not created yet".into(),
        (None, true, Some(n)) => format!("✔ {n} task(s) in the history"),
        (None, true, None) => "✔ valid".into(),
    };
    format!("{path}  {status}")
}

/// The human-readable report.
pub fn render(r: &Report) -> String {
    use std::fmt::Write;
    let mut s = String::new();
    let t = &r.terminal;
    let _ = writeln!(s, "kiwa {} · {} {}", r.kiwa.version, r.kiwa.os, r.kiwa.arch);
    let _ = writeln!(s, "\nterminal");
    let _ = writeln!(s, "  program   {}", t.program);
    if let Some(term) = &t.term {
        let _ = writeln!(s, "  TERM      {term}");
    }
    let tty = |b: bool| if b { "terminal" } else { "redirected" };
    let _ = writeln!(
        s,
        "  streams   stdin {}, stdout {}",
        tty(t.stdin_tty),
        tty(t.stdout_tty)
    );
    if let Some((w, h)) = t.size {
        let _ = writeln!(s, "  size      {w} × {h}");
    }
    let c = &t.colours;
    let _ = writeln!(
        s,
        "  colours   {}{}{}",
        if c.enabled { "on" } else { "off (NO_COLOR)" },
        match c.depth {
            "unknown" => ", depth not advertised".to_owned(),
            d => format!(", {d}"),
        },
        c.from
            .as_ref()
            .map(|f| format!(" ({f})"))
            .unwrap_or_default()
    );
    let _ = writeln!(
        s,
        "  mouse     {}{}",
        if t.mouse.enabled { "on" } else { "off" },
        t.mouse
            .reason
            .as_ref()
            .map(|r| format!(" ({r})"))
            .unwrap_or_default()
    );
    let _ = writeln!(s, "\nfiles");
    let _ = writeln!(s, "  config    {}", file_line(&r.config));
    for w in &r.config.warnings {
        let _ = writeln!(s, "            ⚠ {w}");
    }
    let _ = writeln!(s, "  state     {}", file_line(&r.state));
    let found = r.runners.iter().filter(|x| x.path.is_some()).count();
    let _ = writeln!(s, "\nrunners on PATH ({found} of {})", r.runners.len());
    let w = r.runners.iter().map(|x| x.name.len()).max().unwrap_or(0);
    for x in &r.runners {
        match &x.path {
            Some(p) => {
                let _ = writeln!(s, "  ✔ {:<w$}  {}", x.name, p.display());
            }
            None => {
                let _ = writeln!(
                    s,
                    "  · {:<w$}  not found · {}",
                    x.name,
                    x.hint.as_deref().unwrap_or_default()
                );
            }
        }
    }
    let _ = write!(
        s,
        "\nCheck what this terminal really sends with `{} doctor --probe`.",
        crate::me()
    );
    s
}

// ------------------------------------------------------------------ probe

/// What `--probe` checks, in display order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Check {
    LeftClick,
    RightClick,
    MiddleClick,
    Motion,
    Drag,
    WheelUp,
    WheelDown,
    Resize,
    Truecolor,
}

impl Check {
    pub const ALL: [Check; 9] = [
        Check::LeftClick,
        Check::RightClick,
        Check::MiddleClick,
        Check::Motion,
        Check::Drag,
        Check::WheelUp,
        Check::WheelDown,
        Check::Resize,
        Check::Truecolor,
    ];

    fn label(self) -> &'static str {
        match self {
            Check::LeftClick => "left click",
            Check::RightClick => "right click",
            Check::MiddleClick => "middle click",
            Check::Motion => "motion without a button (hover)",
            Check::Drag => "drag with the left button",
            Check::WheelUp => "wheel up",
            Check::WheelDown => "wheel down",
            Check::Resize => "resize the window",
            Check::Truecolor => "the bar below is a smooth gradient: press t",
        }
    }
}

#[derive(Debug, Default, Serialize)]
pub struct Probe {
    pub program: String,
    pub seen: Vec<Check>,
    /// Ended with q / Esc (true) rather than Ctrl+C.
    #[serde(skip)]
    pub done: bool,
}

impl Probe {
    pub fn new(program: String) -> Self {
        Self {
            program,
            ..Default::default()
        }
    }

    fn mark(&mut self, c: Check) {
        if !self.seen.contains(&c) {
            self.seen.push(c);
        }
    }

    /// Apply one event; returns true when the probe is over.
    pub fn apply(&mut self, ev: &Event) -> bool {
        match ev {
            Event::Key(k) if k.kind == KeyEventKind::Press => match k.code {
                KeyCode::Char('c') if k.modifiers.contains(KeyModifiers::CONTROL) => return true,
                KeyCode::Char('q') | KeyCode::Esc | KeyCode::Enter => {
                    self.done = true;
                    return true;
                }
                KeyCode::Char('t') => self.mark(Check::Truecolor),
                _ => {}
            },
            Event::Mouse(m) => match m.kind {
                MouseEventKind::Down(MouseButton::Left) => self.mark(Check::LeftClick),
                MouseEventKind::Down(MouseButton::Right) => self.mark(Check::RightClick),
                MouseEventKind::Down(MouseButton::Middle) => self.mark(Check::MiddleClick),
                MouseEventKind::Moved => self.mark(Check::Motion),
                MouseEventKind::Drag(MouseButton::Left) => self.mark(Check::Drag),
                MouseEventKind::ScrollUp => self.mark(Check::WheelUp),
                MouseEventKind::ScrollDown => self.mark(Check::WheelDown),
                _ => {}
            },
            Event::Resize(..) => self.mark(Check::Resize),
            _ => {}
        }
        false
    }

    fn has(&self, c: Check) -> bool {
        self.seen.contains(&c)
    }

    /// One row for docs/reference/terminals.md.
    pub fn markdown_row(&self, version: &str, os: &str) -> String {
        let yes = |c: &[Check]| {
            let n = c.iter().filter(|x| self.has(**x)).count();
            match n {
                0 => "✘",
                _ if n == c.len() => "✔",
                _ => "partial",
            }
        };
        format!(
            "| {} {version} | {os} | {} | {} | {} | {} | {} |",
            self.program,
            yes(&[Check::LeftClick, Check::RightClick, Check::MiddleClick]),
            yes(&[Check::Motion, Check::Drag]),
            yes(&[Check::WheelUp, Check::WheelDown]),
            yes(&[Check::Resize]),
            yes(&[Check::Truecolor]),
        )
    }
}

/// The summary printed after the probe, with the row for docs/reference/terminals.md.
pub fn render_probe(p: &Probe) -> String {
    let mut s = format!("terminal probe · {}\n", p.program);
    for c in Check::ALL {
        let mark = if p.has(c) { "✔" } else { "✘" };
        s.push_str(&format!("  {mark} {}\n", c.label()));
    }
    s.push_str(
        "\nRow for docs/reference/terminals.md (add the terminal's version after its name):\n",
    );
    s.push_str(
        &p.markdown_row("", std::env::consts::OS)
            .replace("  |", " |"),
    );
    s
}

pub fn draw_probe(f: &mut Frame, p: &Probe, th: &Theme) {
    let area = f.area();
    let mut lines = vec![
        Line::from(vec![
            Span::styled(" kiwa ", th.accent),
            Span::styled(format!(" terminal probe · {}", p.program), th.bold),
        ]),
        Line::raw(""),
        Line::styled(
            "  Try each of these here; a check appears when the terminal reports it.",
            th.dim,
        ),
        Line::raw(""),
    ];
    for c in Check::ALL {
        let (mark, st) = if p.has(c) {
            ("✔", th.ok)
        } else {
            ("·", th.dim)
        };
        lines.push(Line::from(vec![
            Span::styled(format!("  {mark} "), st),
            Span::raw(c.label()),
        ]));
    }
    lines.push(Line::raw(""));
    f.render_widget(Paragraph::new(lines), area);
    // A 24-bit gradient: smooth on a truecolor terminal, banded when colours are reduced.
    let y = area.y + 4 + Check::ALL.len() as u16 + 1;
    if y < area.height {
        let w = area.width.saturating_sub(4).min(64);
        let bar: Vec<Span> = (0..w)
            .map(|x| {
                let v = (x as u32 * 255 / w.max(1) as u32) as u8;
                Span::styled(" ", Style::new().bg(Color::Rgb(v, 64, 255 - v)))
            })
            .collect();
        let mut spans = vec![Span::raw("  ")];
        spans.extend(bar);
        f.render_widget(
            Paragraph::new(Line::from(spans)),
            Rect::new(0, y, area.width, 1),
        );
    }
    if y + 2 < area.height {
        f.render_widget(
            Paragraph::new(Line::styled("  q, Esc or Enter: finish", th.dim)),
            Rect::new(0, y + 2, area.width, 1),
        );
    }
}

/// Run the interactive probe. Returns `None` when cancelled with Ctrl+C.
pub fn probe(th: &Theme) -> std::io::Result<Option<Probe>> {
    let mut p = Probe::new(terminal_program(&env));
    let mut term: DefaultTerminal = ratatui::try_init()?;
    let _ = execute!(std::io::stdout(), EnableMouseCapture);
    let res = (|| -> std::io::Result<()> {
        loop {
            term.draw(|f| draw_probe(f, &p, th))?;
            if p.apply(&event::read()?) {
                return Ok(());
            }
        }
    })();
    let _ = execute!(std::io::stdout(), DisableMouseCapture);
    ratatui::restore();
    res?;
    Ok(p.done.then_some(p))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::crossterm::event::{KeyEvent, MouseEvent};
    use std::collections::HashMap;

    fn vars(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let m: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| ((*k).into(), (*v).into()))
            .collect();
        move |k| m.get(k).cloned()
    }

    #[test]
    fn terminal_programs() {
        let cases: &[(&[(&str, &str)], &str)] = &[
            (
                &[("TERM_PROGRAM", "vscode"), ("WT_SESSION", "x")],
                "VS Code",
            ),
            (&[("WT_SESSION", "abc")], "Windows Terminal"),
            (&[("TERM_PROGRAM", "Apple_Terminal")], "macOS Terminal"),
            (&[("TERM_PROGRAM", "iTerm.app")], "iTerm2"),
            (&[("TERM", "xterm-kitty")], "Kitty"),
            (&[("KONSOLE_VERSION", "230800")], "Konsole"),
            (&[("GNOME_TERMINAL_SCREEN", "/x")], "GNOME Terminal"),
            (&[("VTE_VERSION", "7600")], "VTE-based terminal"),
            (&[("MSYSTEM", "MINGW64")], "mintty"),
            (&[("TERM_PROGRAM", "Hyper")], "Hyper"),
        ];
        for (env, want) in cases {
            assert_eq!(terminal_program(&vars(env)), *want, "{env:?}");
        }
        let unknown = terminal_program(&vars(&[]));
        assert!(unknown.contains("unknown"));
    }

    #[test]
    fn colour_depth_from_the_environment() {
        let c = colours(&vars(&[("COLORTERM", "truecolor"), ("NO_COLOR", "1")]));
        assert_eq!((c.enabled, c.depth), (false, "truecolor"));
        assert_eq!(colours(&vars(&[("WT_SESSION", "x")])).depth, "truecolor");
        assert_eq!(
            colours(&vars(&[("TERM", "xterm-256color")]))
                .from
                .as_deref(),
            Some("TERM=xterm-256color")
        );
        assert_eq!(colours(&vars(&[("TERM", "dumb")])).depth, "none");
        let c = colours(&vars(&[("NO_COLOR", "")]));
        assert_eq!((c.enabled, c.depth, c.from), (true, "unknown", None));
    }

    #[test]
    fn every_runner_has_a_specific_install_hint() {
        assert_eq!(RUNNERS.len(), 28);
        for r in RUNNERS {
            let hint = crate::exec::install_hint(r);
            assert!(!hint.contains("make sure it is on PATH"), "{r}: {hint}");
        }
    }

    #[test]
    fn report_renders_every_section() {
        let cfg = kiwa_core::config::Config::default();
        let r = report((false, Some("--no-mouse".into())), Ok(&cfg));
        assert_eq!(r.runners.len(), RUNNERS.len());
        let s = render(&r);
        for part in [
            "terminal",
            "program",
            "colours",
            "mouse     off (--no-mouse)",
            "config",
            "state",
            "runners on PATH",
            "--probe",
        ] {
            assert!(s.contains(part), "{part}: {s}");
        }
        let json = serde_json::to_value(&r).unwrap();
        assert!(json["terminal"]["colours"]["depth"].is_string());
        let bad = report((true, None), Err("line 1: unknown node".into()));
        assert!(render(&bad).contains("✘ line 1: unknown node"));
    }

    fn mouse(kind: MouseEventKind) -> Event {
        Event::Mouse(MouseEvent {
            kind,
            column: 1,
            row: 1,
            modifiers: KeyModifiers::NONE,
        })
    }

    #[test]
    fn probe_records_what_the_terminal_sends() {
        let mut p = Probe::new("Test".into());
        for k in [
            MouseEventKind::Down(MouseButton::Left),
            MouseEventKind::Down(MouseButton::Left),
            MouseEventKind::Moved,
            MouseEventKind::ScrollUp,
            MouseEventKind::ScrollDown,
            MouseEventKind::Up(MouseButton::Left),
        ] {
            assert!(!p.apply(&mouse(k)));
        }
        assert!(!p.apply(&Event::Resize(80, 24)));
        assert!(!p.apply(&Event::Key(KeyEvent::new(
            KeyCode::Char('t'),
            KeyModifiers::NONE
        ))));
        assert_eq!(p.seen.len(), 6, "repeated events count once");
        assert_eq!(
            p.markdown_row("1.0", "linux"),
            "| Test 1.0 | linux | partial | partial | ✔ | ✔ | ✔ |"
        );
        let th = Theme::new(None, false);
        let mut t = ratatui::Terminal::new(ratatui::backend::TestBackend::new(70, 20)).unwrap();
        t.draw(|f| draw_probe(f, &p, &th)).unwrap();
        let text: String = t
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect();
        assert!(
            text.contains("✔ left click") && text.contains("· right click"),
            "{text}"
        );
        assert!(text.contains("q, Esc or Enter"));
        assert!(p.apply(&Event::Key(KeyEvent::new(
            KeyCode::Char('q'),
            KeyModifiers::NONE
        ))));
        assert!(p.done);
        let mut c = Probe::new("x".into());
        assert!(c.apply(&Event::Key(KeyEvent::new(
            KeyCode::Char('c'),
            KeyModifiers::CONTROL
        ))));
        assert!(!c.done, "Ctrl+C cancels");
        assert_eq!(c.markdown_row("1", "x"), "| x 1 | x | ✘ | ✘ | ✘ | ✘ | ✘ |");
    }
}
