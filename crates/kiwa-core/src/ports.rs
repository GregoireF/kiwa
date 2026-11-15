// SPDX-License-Identifier: MIT OR Apache-2.0

//! Where a dev server will listen, worked out from files before it runs: a `--port` in the
//! command, the tool's config (`vite.config.*`), `PORT` in `.env`, Compose `ports`, and the
//! tool's documented default. Static like discovery; the output of the task is never read.

use crate::model::{Ecosystem, Project, Task};
use std::fs;
use std::path::Path;

/// Dev servers and the port they use when none is given, from their documentation.
const DEFAULTS: &[(&[&str], u16)] = &[
    (&["vite"], 5173),
    (&["next", "dev"], 3000),
    (&["nuxt", "dev"], 3000),
    (&["nuxi", "dev"], 3000),
    (&["astro", "dev"], 4321),
    (&["remix", "dev"], 3000),
    (&["svelte-kit", "dev"], 5173),
    (&["webpack", "serve"], 8080),
    (&["webpack-dev-server"], 8080),
    (&["storybook", "dev"], 6006),
    (&["start-storybook"], 6006),
    (&["ng", "serve"], 4200),
    (&["react-scripts", "start"], 3000),
    (&["gatsby", "develop"], 8000),
    (&["eleventy", "--serve"], 8080),
    (&["hugo", "server"], 1313),
    (&["jekyll", "serve"], 4000),
    (&["manage.py", "runserver"], 8000),
    (&["flask", "run"], 5000),
    (&["uvicorn"], 8000),
    (&["fastapi", "dev"], 8000),
    (&["http.server"], 8000),
    (&["rails", "server"], 3000),
    (&["rails", "s"], 3000),
    (&["artisan", "serve"], 8000),
    (&["mkdocs", "serve"], 8000),
];

/// `--port 4000`, `--port=4000`, `-p 4000`, `-p4000`, or `host:port` after `runserver`.
pub fn explicit_port(words: &[&str]) -> Option<u16> {
    for (i, w) in words.iter().enumerate() {
        let next = words.get(i + 1).and_then(|n| n.parse().ok());
        if let Some(v) = w
            .strip_prefix("--port=")
            .or_else(|| w.strip_prefix("--PORT="))
        {
            return v.parse().ok();
        }
        if matches!(*w, "--port" | "-p" | "--PORT")
            && let Some(p) = next
        {
            return Some(p);
        }
        if let Some(v) = w.strip_prefix("-p").filter(|v| !v.is_empty())
            && let Ok(p) = v.parse()
        {
            return Some(p);
        }
        // `runserver 0.0.0.0:8001`, `http.server 9000`
        if (*w == "runserver" || w.ends_with("http.server")) && i + 1 < words.len() {
            let arg = words[i + 1];
            let port = arg.rsplit(':').next().unwrap_or(arg);
            if let Ok(p) = port.parse() {
                return Some(p);
            }
        }
    }
    None
}

/// The default port of the dev server a command starts, if it is one we know.
fn default_port(words: &[&str]) -> Option<u16> {
    let base: Vec<&str> = words
        .iter()
        .map(|w| w.rsplit(['/', '\\']).next().unwrap_or(w))
        .collect();
    DEFAULTS
        .iter()
        .find(|(seq, _)| {
            // `vite` alone, or followed by `dev`/`serve`, but not `vite build`.
            base.windows(seq.len()).enumerate().any(|(i, w)| {
                w == *seq
                    && !(*seq == ["vite"]
                        && base
                            .get(i + 1)
                            .is_some_and(|n| matches!(*n, "build" | "preview" | "optimize")))
            })
        })
        .map(|(_, p)| *p)
}

/// `server: { port: 3001 }` in a Vite config file (a plain scan for `port:` after `server`).
fn vite_port(dir: &Path) -> Option<u16> {
    let text = [
        "vite.config.ts",
        "vite.config.js",
        "vite.config.mjs",
        "vite.config.mts",
    ]
    .iter()
    .find_map(|f| fs::read_to_string(dir.join(f)).ok())?;
    let after = &text[text.find("server")?..];
    let at = after.find("port")?;
    let digits: String = after[at + 4..]
        .chars()
        .skip_while(|c| *c == ':' || c.is_whitespace())
        .take_while(char::is_ascii_digit)
        .collect();
    digits.parse().ok()
}

/// `PORT=4000` in `.env`.
fn env_port(dir: &Path) -> Option<u16> {
    fs::read_to_string(dir.join(".env"))
        .ok()?
        .lines()
        .find_map(|l| l.trim().strip_prefix("PORT="))
        .and_then(|v| v.trim().trim_matches(['"', '\'']).parse().ok())
}

/// The port a task's command will serve on: explicit, then config and `.env` for a known dev
/// server, then that server's default.
pub fn port_of(dir: &Path, t: &Task) -> Option<u16> {
    let text = t
        .line()
        .map(str::to_owned)
        .or_else(|| t.description.clone().filter(|_| t.runner != "docker"))
        .unwrap_or_else(|| t.command.join(" "));
    let words: Vec<&str> = text.split_whitespace().collect();
    if let Some(p) = explicit_port(&words) {
        return Some(p);
    }
    let default = default_port(&words)?;
    let vite = words.iter().any(|w| w.ends_with("vite"));
    if vite && let Some(p) = vite_port(dir) {
        return Some(p);
    }
    Some(env_port(dir).unwrap_or(default))
}

/// Host ports published by a Compose service (`"8080:80"`, `127.0.0.1:5432:5432`).
pub fn compose_ports(text: &str, service: &str) -> Vec<u16> {
    let mut out = Vec::new();
    let (mut in_services, mut level, mut current) = (false, None::<usize>, false);
    let mut in_ports: Option<usize> = None;
    for line in text.lines() {
        let t = line.trim_start();
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        let d = line.len() - t.len();
        if d == 0 {
            in_services = t.starts_with("services:");
            level = None;
            continue;
        }
        if !in_services {
            continue;
        }
        let lv = *level.get_or_insert(d);
        if d == lv {
            let name = t
                .split(':')
                .next()
                .unwrap_or("")
                .trim()
                .trim_matches(['"', '\'']);
            current = name == service;
            in_ports = None;
            continue;
        }
        if !current {
            continue;
        }
        let mut host = |spec: &str| {
            let spec = spec.trim().trim_matches(['"', '\'']);
            let parts: Vec<&str> = spec.split(':').collect();
            if parts.len() >= 2
                && let Ok(p) = parts[parts.len() - 2]
                    .split('-')
                    .next()
                    .unwrap_or("")
                    .parse()
            {
                out.push(p);
            }
        };
        if let Some(pd) = in_ports {
            if d > pd {
                if let Some(item) = t.strip_prefix("- ") {
                    host(item);
                }
                continue;
            }
            in_ports = None;
        }
        if let Some(rest) = t.strip_prefix("ports:") {
            match rest.trim().strip_prefix('[') {
                Some(flow) => flow.trim_end_matches(']').split(',').for_each(&mut host),
                None => in_ports = Some(d),
            }
        }
    }
    out
}

/// Set `url` on the tasks that start a server we can locate.
pub fn annotate(p: &mut Project) {
    // Compose files are read only for a Compose project (most projects have none).
    let compose = p
        .ecosystems
        .contains(&Ecosystem::Compose)
        .then(|| {
            [
                "compose.yaml",
                "compose.yml",
                "docker-compose.yml",
                "docker-compose.yaml",
            ]
            .iter()
            .find_map(|f| fs::read_to_string(p.path.join(f)).ok())
        })
        .flatten();
    for t in &mut p.tasks {
        let port = match (&compose, t.runner.as_str(), t.name.strip_suffix(":up")) {
            (Some(text), "docker", Some(service)) if service != "compose" => {
                compose_ports(text, service).first().copied()
            }
            (_, "docker", _) => None,
            _ => port_of(&p.path, t),
        };
        t.url = port.map(|p| format!("http://localhost:{p}"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn port(cmd: &str) -> Option<u16> {
        let t = Task::from_argv("dev", "npm", vec!["npm".into(), "run".into(), "dev".into()])
            .with_description(Some(cmd.into()));
        port_of(Path::new("/nowhere"), &t)
    }

    #[test]
    fn ports_from_commands_and_defaults() {
        assert_eq!(port("vite"), Some(5173));
        assert_eq!(port("vite --port 4000"), Some(4000));
        assert_eq!(port("vite build"), None, "a build serves nothing");
        assert_eq!(port("next dev -p 3005"), Some(3005));
        assert_eq!(port("next dev"), Some(3000));
        assert_eq!(port("astro dev --port=4400"), Some(4400));
        assert_eq!(port("python manage.py runserver 0.0.0.0:8001"), Some(8001));
        assert_eq!(port("python -m http.server 9000"), Some(9000));
        assert_eq!(port("uvicorn app:app --reload"), Some(8000));
        assert_eq!(port("tsc --watch"), None);
        assert_eq!(
            port("node server.js --port 7000"),
            Some(7000),
            "explicit wins for any tool"
        );
        assert_eq!(port("jest -p4"), Some(4), "`-pN` is read as a port");
    }

    #[test]
    fn config_and_env_files() {
        let d = tempfile::tempdir().unwrap();
        fs::write(
            d.path().join("vite.config.ts"),
            "export default defineConfig({ plugins: [], server: { host: true, port: 3333 } })",
        )
        .unwrap();
        fs::write(d.path().join(".env"), "PORT=\"4100\"\n").unwrap();
        let vite =
            Task::new("dev", "npm", &["npm", "run", "dev"]).with_description(Some("vite".into()));
        assert_eq!(
            port_of(d.path(), &vite),
            Some(3333),
            "the Vite config first"
        );
        let next = Task::new("dev", "npm", &["npm", "run", "dev"])
            .with_description(Some("next dev".into()));
        assert_eq!(port_of(d.path(), &next), Some(4100), "then PORT in .env");
    }

    #[test]
    fn compose_published_ports() {
        let text = "services:\n  web:\n    image: x\n    ports:\n      - \"8080:80\"\n      - 127.0.0.1:9229:9229\n  db:\n    ports: [\"5432:5432\"]\n  worker:\n    expose: [\"9000\"]\n";
        assert_eq!(compose_ports(text, "web"), [8080, 9229]);
        assert_eq!(compose_ports(text, "db"), [5432]);
        assert!(
            compose_ports(text, "worker").is_empty(),
            "expose is not published"
        );
    }
}
