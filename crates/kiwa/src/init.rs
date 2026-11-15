// SPDX-License-Identifier: MIT OR Apache-2.0

//! `kw init`: first-run setup, one screen. Shows the folders that exist with how many projects
//! and tasks each holds; the user ticks one or more and presses Enter. Nothing else is asked:
//! the accent keeps its current value (`kw config set accent …` or `kw init --accent` change it).
//! The wizard is a state machine (`Wizard::handle` + `Wizard::draw`), tested without a terminal.

use kiwa_core::config::{Editor, expand_home};
use kiwa_core::suggest::Suggestion;
use ratatui::Frame;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, ListItem, ListState, Paragraph};
use std::path::PathBuf;

#[derive(Debug, PartialEq)]
pub struct Choice {
    pub roots: Vec<PathBuf>,
    pub accent: String,
}

struct Row {
    path: PathBuf,
    label: String,
    on: bool,
}

#[derive(Debug, PartialEq)]
pub enum Outcome {
    Done(Choice),
    Cancelled,
}

struct Wizard {
    rows: Vec<Row>,
    folders: ListState,
    typing: Option<String>,
    error: Option<String>,
    accent: String,
}

impl Wizard {
    fn new(suggestions: &[Suggestion], current_accent: Option<&str>) -> Self {
        let rows = suggestions
            .iter()
            .map(|s| Row {
                path: s.path.clone(),
                label: format!(
                    "{:<44} {:>4} projects · {:>4} tasks  ({})",
                    s.path.display(),
                    s.projects,
                    s.tasks,
                    s.reason
                ),
                on: false,
            })
            .collect();
        Self {
            rows,
            folders: ListState::default().with_selected(Some(0)),
            typing: None,
            error: None,
            accent: current_accent.unwrap_or("cyan").to_owned(),
        }
    }

    fn handle(&mut self, k: KeyEvent) -> Option<Outcome> {
        if k.kind != KeyEventKind::Press {
            return None;
        }
        if k.code == KeyCode::Char('c') && k.modifiers.contains(KeyModifiers::CONTROL) {
            return Some(Outcome::Cancelled);
        }
        if let Some(t) = self.typing.as_mut() {
            match k.code {
                KeyCode::Esc => self.typing = None,
                KeyCode::Backspace => {
                    t.pop();
                }
                KeyCode::Char(c) => t.push(c),
                KeyCode::Enter => {
                    let p = expand_home(t);
                    if p.is_dir() {
                        let p = std::path::absolute(&p).unwrap_or(p);
                        let label = format!("{:<44} (added)", p.display());
                        self.rows.push(Row {
                            path: p,
                            label,
                            on: true,
                        });
                        self.folders.select(Some(self.rows.len() - 1));
                        self.error = None;
                    } else {
                        self.error = Some(format!("not a folder: {}", p.display()));
                    }
                    self.typing = None;
                }
                _ => {}
            }
            return None;
        }
        let sel = self.folders.selected().unwrap_or(0);
        let add_row = self.rows.len(); // the "add a folder" row
        let n = add_row + 1;
        match k.code {
            KeyCode::Esc => return Some(Outcome::Cancelled),
            KeyCode::Up => self.folders.select(Some((sel + n - 1) % n)),
            KeyCode::Down => self.folders.select(Some((sel + 1) % n)),
            KeyCode::Char(' ') | KeyCode::Enter if sel == add_row => {
                self.typing = Some(String::new());
            }
            KeyCode::Char(' ') => self.rows[sel].on = !self.rows[sel].on,
            KeyCode::Enter => {
                if !self.rows.iter().any(|r| r.on) {
                    self.rows[sel].on = true; // Enter with nothing ticked = this row
                }
                return Some(Outcome::Done(Choice {
                    roots: self
                        .rows
                        .iter()
                        .filter(|r| r.on)
                        .map(|r| r.path.clone())
                        .collect(),
                    accent: self.accent.clone(),
                }));
            }
            _ => {}
        }
        None
    }

    fn draw(&mut self, f: &mut Frame) {
        let bold = Style::new().add_modifier(Modifier::BOLD);
        let dim = Style::new().add_modifier(Modifier::DIM);
        let badge = bold.add_modifier(Modifier::REVERSED);
        let [h, rule, body, input, foot] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(3),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .areas(f.area());
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(" kiwa ", badge),
                Span::styled("  Which folders hold your projects?", bold),
                Span::styled("  Counts show what kiwa found in each.", dim),
            ])),
            h,
        );
        f.render_widget(
            Paragraph::new(Line::styled("─".repeat(rule.width as usize), dim)),
            rule,
        );
        let mut items: Vec<ListItem> = self
            .rows
            .iter()
            .map(|r| {
                let mark = if r.on { "◉" } else { "○" };
                ListItem::new(format!(" {mark}  {}", r.label))
            })
            .collect();
        items.push(ListItem::new(Span::styled(
            " +  add a folder by path…",
            dim,
        )));
        let list = List::new(items)
            .highlight_style(bold)
            .highlight_symbol("● ");
        f.render_stateful_widget(list, body, &mut self.folders);
        let line = match (&self.typing, &self.error) {
            (Some(t), _) => Line::from(vec![Span::styled(" path ❯ ", bold), Span::raw(t.clone())]),
            (None, Some(e)) => Line::styled(format!(" {e}"), Style::new().fg(Color::Red)),
            _ => Line::raw(""),
        };
        f.render_widget(Paragraph::new(line), input);
        f.render_widget(
            Paragraph::new(Line::styled(
                " ↑↓ move · space tick · ⏎ save · esc cancel",
                dim,
            )),
            foot,
        );
    }
}

/// Interactive wizard. `None` when the user cancels.
pub fn wizard(
    suggestions: &[Suggestion],
    current_accent: Option<&str>,
) -> std::io::Result<Option<Choice>> {
    let mut w = Wizard::new(suggestions, current_accent);
    let mut term = ratatui::try_init()?;
    let res = loop {
        if let Err(e) = term.draw(|f| w.draw(f)) {
            break Err(e);
        }
        match event::read() {
            Ok(Event::Key(k)) => match w.handle(k) {
                Some(Outcome::Done(c)) => break Ok(Some(c)),
                Some(Outcome::Cancelled) => break Ok(None),
                None => {}
            },
            Ok(_) => {}
            Err(e) => break Err(e),
        }
    };
    ratatui::restore();
    res
}

/// Write the choice, keeping every other setting and comment already in the file.
pub fn save(choice: &Choice) -> anyhow::Result<PathBuf> {
    let mut e = Editor::open_default()?;
    let roots = choice
        .roots
        .iter()
        .map(|p| kdl::KdlValue::String(p.to_string_lossy().replace('\\', "/")))
        .collect();
    e.set("roots", roots);
    e.set("accent", vec![kdl::KdlValue::String(choice.accent.clone())]);
    e.save()?;
    Ok(e.path().to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    fn sugg(path: &str, projects: usize) -> Suggestion {
        Suggestion {
            path: PathBuf::from(path),
            reason: "in your home folder",
            projects,
            tasks: projects * 3,
        }
    }
    fn k(c: KeyCode) -> KeyEvent {
        KeyEvent::new(c, KeyModifiers::NONE)
    }
    fn screen(w: &mut Wizard) -> String {
        let mut t = Terminal::new(TestBackend::new(110, 26)).unwrap();
        t.draw(|f| w.draw(f)).unwrap();
        let b = t.backend().buffer().clone();
        (0..26)
            .map(|y| {
                (0..110)
                    .map(|x| b[(x, y)].symbol().to_string())
                    .collect::<String>()
                    + "\n"
            })
            .collect()
    }

    #[test]
    fn one_screen_enter_on_a_row_saves_it_with_the_current_accent() {
        let s = [sugg("/home/dev", 12), sugg("/home/code", 0)];
        let mut w = Wizard::new(&s, Some("violet"));
        let first = screen(&mut w);
        assert!(
            first.contains("Which folders hold your projects?")
                && first.contains("12 projects")
                && first.contains("add a folder by path")
                && first.contains("⏎ save"),
            "{first}"
        );
        assert_eq!(
            w.handle(k(KeyCode::Enter)),
            Some(Outcome::Done(Choice {
                roots: vec![PathBuf::from("/home/dev")],
                accent: "violet".into()
            }))
        );
    }

    #[test]
    fn space_ticks_several_rows_and_arrows_wrap() {
        let s = [sugg("/a", 1), sugg("/b", 2), sugg("/c", 3)];
        let mut w = Wizard::new(&s, None);
        w.handle(k(KeyCode::Char(' ')));
        w.handle(k(KeyCode::Up));
        assert_eq!(
            w.folders.selected(),
            Some(3),
            "↑ on the first row wraps to the last"
        );
        w.handle(k(KeyCode::Down));
        assert_eq!(w.folders.selected(), Some(0));
        w.handle(k(KeyCode::Down));
        w.handle(k(KeyCode::Down));
        w.handle(k(KeyCode::Char(' ')));
        w.handle(k(KeyCode::Char(' ')));
        w.handle(k(KeyCode::Char(' ')));
        assert!(screen(&mut w).contains("◉"));
        let Some(Outcome::Done(c)) = w.handle(k(KeyCode::Enter)) else {
            panic!()
        };
        assert_eq!(c.roots, [PathBuf::from("/a"), PathBuf::from("/c")]);
        assert_eq!(c.accent, "cyan", "default accent when none is set");
    }

    #[test]
    fn typed_path_is_added_or_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let mut w = Wizard::new(&[], None);
        w.handle(k(KeyCode::Enter)); // only row = "add a folder by path"
        assert!(w.typing.is_some());
        for c in "/definitely/not/here".chars() {
            w.handle(k(KeyCode::Char(c)));
        }
        assert!(screen(&mut w).contains("path ❯"));
        w.handle(k(KeyCode::Enter));
        assert!(w.error.as_ref().unwrap().contains("not a folder"));
        assert!(screen(&mut w).contains("not a folder"));
        w.handle(k(KeyCode::Char(' ')));
        for c in format!("\"{}\"x", dir.path().display()).chars() {
            w.handle(k(KeyCode::Char(c)));
        }
        w.handle(k(KeyCode::Backspace));
        w.handle(k(KeyCode::Enter));
        assert!(w.error.is_none());
        assert_eq!(w.rows.len(), 1);
        assert!(w.rows[0].on);
        w.handle(k(KeyCode::Char(' ')));
        w.handle(k(KeyCode::Esc));
        assert!(w.typing.is_none());
    }

    #[test]
    fn escape_and_ctrl_c_cancel() {
        let s = [sugg("/a", 1)];
        let mut w = Wizard::new(&s, None);
        assert_eq!(w.handle(k(KeyCode::Esc)), Some(Outcome::Cancelled));
        let mut w = Wizard::new(&s, None);
        let ctrl_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert_eq!(w.handle(ctrl_c), Some(Outcome::Cancelled));
        let mut release = k(KeyCode::Enter);
        release.kind = KeyEventKind::Release;
        assert!(w.handle(release).is_none());
    }
}
