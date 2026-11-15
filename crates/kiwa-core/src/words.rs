// SPDX-License-Identifier: MIT OR Apache-2.0

//! Arguments typed in the picker → an argv, without a shell.
//!
//! A word ends at unquoted whitespace. Inside `'…'` everything is literal; inside `"…"`, `\"`
//! and `\\` stand for `"` and `\`. Outside quotes a backslash is an ordinary character, so a
//! Windows path such as `C:\logs\out.txt` needs no escaping. Quoted and unquoted parts glue
//! together (`--name="a b"` is one word). Nothing is expanded: no variables, no globs.

use std::iter::Peekable;
use std::str::CharIndices;

/// Splits `line` into words, or explains which quote is left open and where.
pub fn split(line: &str) -> Result<Vec<String>, String> {
    let mut chars = line.char_indices().peekable();
    let mut argv = Vec::new();
    loop {
        while chars.next_if(|(_, c)| c.is_whitespace()).is_some() {}
        if chars.peek().is_none() {
            return Ok(argv);
        }
        let mut word = String::new();
        while let Some((at, c)) = chars.next_if(|(_, c)| !c.is_whitespace()) {
            if c == '"' || c == '\'' {
                if !quoted(&mut chars, c, &mut word) {
                    let column = line[..at].chars().count() + 1;
                    return Err(format!("unclosed {c} quote, opened at column {column}"));
                }
            } else {
                word.push(c);
            }
        }
        argv.push(word);
    }
}

/// Whether a command line from a file (README, CI step, Procfile, `devcontainer.json`…) means
/// something only a shell understands: operators (`&& | ; > <`), expansions (`$VAR`, `$(…)`,
/// backquotes, globs, a leading `~`), grouping, comments, backslash escapes or line
/// continuations, several lines, or a leading `NAME=value` assignment. Quoted text is literal.
/// When `false`, [`split`] gives the exact argv the shell would have run.
pub fn needs_shell(line: &str) -> bool {
    if line.trim().contains('\n') || split(line).is_err() {
        return true;
    }
    let mut quote: Option<char> = None;
    let mut word_start = true;
    let mut first_word = true;
    let mut first: String = String::new();
    for c in line.trim().chars() {
        match quote {
            Some(q) if c == q => quote = None,
            Some('"') if c == '$' || c == '`' || c == '\\' => return true,
            Some(_) => {}
            None => {
                if c == '\'' || c == '"' {
                    quote = Some(c);
                } else if c.is_whitespace() {
                    first_word = first_word && first.is_empty();
                    word_start = true;
                    continue;
                } else if "|&;<>$`()*?{}[]\\".contains(c) || (word_start && (c == '~' || c == '#'))
                {
                    return true;
                }
                if first_word {
                    first.push(c);
                }
            }
        }
        word_start = false;
    }
    // `NODE_ENV=production node app.js`: an environment assignment, not a program.
    first.split_once('=').is_some_and(|(name, _)| {
        name.chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
            && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    })
}

/// Appends the inside of a quoted part to `word`; `false` when the closing quote is missing.
fn quoted(chars: &mut Peekable<CharIndices>, quote: char, word: &mut String) -> bool {
    while let Some((_, c)) = chars.next() {
        if c == quote {
            return true;
        }
        let escaped = (quote == '"' && c == '\\')
            .then(|| chars.next_if(|(_, n)| *n == '"' || *n == '\\'))
            .flatten();
        word.push(escaped.map_or(c, |(_, n)| n));
    }
    false
}

#[cfg(test)]
mod tests {
    use super::{needs_shell, split};

    #[test]
    fn lines_that_only_a_shell_understands() {
        for plain in [
            "npm install",
            "cargo test --all",
            "docker compose up -d",
            r#"echo "hello world""#,
            "python -m http.server 8000",
            "git commit -m 'a & b; c'",
            "--name=x",
        ] {
            assert!(!needs_shell(plain), "{plain}");
        }
        for shell in [
            "npm ci && npm test",
            "cat a | grep b",
            "echo $HOME",
            r#"echo "$HOME""#,
            "echo `date`",
            "rm -rf build/*",
            "cd ~/x",
            "NODE_ENV=production node app.js",
            "echo a > out.txt",
            "make \\\n  all",
            "a\nb",
            "(cd x; make)",
            "ls # comment",
            "run 'open",
        ] {
            assert!(needs_shell(shell), "{shell}");
        }
    }

    #[test]
    fn whitespace_separates_and_quotes_group() {
        assert_eq!(split("--port 4000").unwrap(), ["--port", "4000"]);
        assert_eq!(
            split(r#"a "b c" 'd e'   f"#).unwrap(),
            ["a", "b c", "d e", "f"]
        );
        assert_eq!(
            split(r#""" x"#).unwrap(),
            ["", "x"],
            "an empty quoted word is kept"
        );
        assert_eq!(split(" \t ").unwrap(), Vec::<String>::new());
        assert_eq!(
            split("& echo x").unwrap(),
            ["&", "echo", "x"],
            "no shell meaning"
        );
    }

    #[test]
    fn parts_glue_and_escapes_work_only_where_needed() {
        assert_eq!(split(r#"--name="a b"c"#).unwrap(), [r#"--name=a bc"#]);
        assert_eq!(
            split(r#""say \"hi\" \\ \n""#).unwrap(),
            [r#"say "hi" \ \n"#]
        );
        assert!(split(r#"'it\'s'"#).is_err(), "no escapes in single quotes");
        assert_eq!(split(r"C:\logs\out.txt").unwrap(), [r"C:\logs\out.txt"]);
    }

    #[test]
    fn an_open_quote_is_reported_with_its_column() {
        assert_eq!(
            split("run 'open").unwrap_err(),
            "unclosed ' quote, opened at column 5"
        );
        assert_eq!(
            split("é \"x").unwrap_err(),
            "unclosed \" quote, opened at column 3",
            "columns count characters, not bytes"
        );
    }
}
