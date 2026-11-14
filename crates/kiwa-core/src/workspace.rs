//! Monorepo members, from what each ecosystem declares: `workspaces` in package.json (npm, Yarn,
//! Bun), `pnpm-workspace.yaml`, Cargo `[workspace]`, `go.work`, uv `[tool.uv.workspace]`, Gradle
//! `settings.gradle(.kts)` and .NET `.sln` files. Static, like every detector: files are read,
//! nothing is run. A project nested in a folder that declares nothing falls back to folder
//! nesting in the picker; one nested in a root that declares members but leaves it out (or
//! excludes it) is not a member.

use crate::model::{Membership, Project};
use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use serde_json::Value;
use std::path::Path;

/// Members declared by one file of a monorepo root, as paths or globs relative to the root.
#[derive(Debug, Default, PartialEq)]
pub struct Declared {
    pub from: String,
    pub include: Vec<String>,
    pub exclude: Vec<String>,
}

fn clean(p: &str) -> String {
    let p = p.trim().replace('\\', "/");
    let p = p.trim_start_matches("./").trim_end_matches('/');
    p.to_owned()
}

/// Sort patterns into include and exclude (`!pattern`, the npm and pnpm convention).
fn split(from: &str, patterns: impl IntoIterator<Item = String>) -> Declared {
    let mut d = Declared {
        from: from.to_owned(),
        ..Default::default()
    };
    for p in patterns {
        match p.trim().strip_prefix('!') {
            Some(x) => d.exclude.push(clean(x)),
            None => d.include.push(clean(&p)),
        }
    }
    d.include.retain(|p| !p.is_empty());
    d
}

fn strings(v: Option<&Value>) -> Vec<String> {
    v.and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

/// `"workspaces": [...]` or `"workspaces": { "packages": [...] }` (Yarn 1).
pub fn npm_workspaces(package_json: &str) -> Option<Declared> {
    let v: Value = serde_json::from_str(package_json.trim_start_matches('\u{feff}')).ok()?;
    let w = v.get("workspaces")?;
    let list = if w.is_array() {
        strings(Some(w))
    } else {
        strings(w.get("packages"))
    };
    Some(split("package.json workspaces", list))
}

/// The `packages:` list of pnpm-workspace.yaml (a block or a flow sequence). A minimal scan,
/// like the Taskfile one, to avoid a YAML dependency.
pub fn pnpm_workspace(yaml: &str) -> Declared {
    let unquote = |s: &str| {
        let s = s.trim();
        let s = s.split(" #").next().unwrap_or(s).trim();
        s.trim_matches(|c| c == '"' || c == '\'').to_owned()
    };
    let mut out = Vec::new();
    let mut inside = false;
    for line in yaml.lines() {
        let t = line.trim_end();
        if t.trim().is_empty() || t.trim_start().starts_with('#') {
            continue;
        }
        let top = !t.starts_with([' ', '\t', '-']);
        if top {
            inside = false;
            if let Some(rest) = t.strip_prefix("packages:") {
                let rest = rest.trim();
                if let Some(flow) = rest.strip_prefix('[') {
                    out.extend(flow.trim_end_matches(']').split(',').map(unquote));
                } else {
                    inside = true;
                }
            }
            continue;
        }
        if inside {
            if let Some(item) = t.trim_start().strip_prefix("- ") {
                out.push(unquote(item));
            }
        }
    }
    split(
        "pnpm-workspace.yaml",
        out.into_iter().filter(|s| !s.is_empty()),
    )
}

fn toml_strings(t: Option<&toml::Value>) -> Vec<String> {
    t.and_then(toml::Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

/// `[workspace] members` and `exclude` of Cargo.toml.
pub fn cargo_workspace(cargo_toml: &str) -> Option<Declared> {
    let t: toml::Table = cargo_toml.parse().ok()?;
    let w = t.get("workspace")?;
    let mut d = split("Cargo.toml [workspace]", toml_strings(w.get("members")));
    d.exclude
        .extend(toml_strings(w.get("exclude")).iter().map(|s| clean(s)));
    Some(d)
}

/// `[tool.uv.workspace] members` and `exclude` of pyproject.toml.
pub fn uv_workspace(pyproject: &str) -> Option<Declared> {
    let t: toml::Table = pyproject.parse().ok()?;
    let w = t.get("tool")?.get("uv")?.get("workspace")?;
    let mut d = split(
        "pyproject.toml [tool.uv.workspace]",
        toml_strings(w.get("members")),
    );
    d.exclude
        .extend(toml_strings(w.get("exclude")).iter().map(|s| clean(s)));
    Some(d)
}

/// `"workspace": ["./a", "./b"]` (or `{ "members": [...] }`) of deno.json(c).
pub fn deno_workspace(text: &str, file: &str) -> Option<Declared> {
    let v: Value = serde_json::from_str(&crate::detect::jsonc(text)).ok()?;
    let w = v.get("workspace")?;
    let list = if w.is_array() {
        strings(Some(w))
    } else {
        strings(w.get("members"))
    };
    Some(split(&format!("{file} workspace"), list))
}

/// `use ./a` and `use ( ./a ./b )` of go.work.
pub fn go_work(text: &str) -> Declared {
    let mut out = Vec::new();
    let mut block = false;
    for line in text.lines() {
        let l = line.split("//").next().unwrap_or("").trim();
        if block {
            if l.starts_with(')') {
                block = false;
            } else if !l.is_empty() {
                out.push(l.trim_matches('"').to_owned());
            }
        } else if let Some(rest) = l.strip_prefix("use") {
            let rest = rest.trim();
            if rest.starts_with('(') {
                block = true;
            } else if !rest.is_empty() && rest != l {
                out.push(rest.trim_matches('"').to_owned());
            }
        }
    }
    split("go.work", out)
}

/// `include("app", ":lib:core")` and `include 'a', 'b'` of settings.gradle(.kts). Gradle paths
/// map to folders (`:lib:core` → `lib/core`); `projectDir` overrides are not followed.
pub fn gradle_settings(text: &str) -> Declared {
    let mut out = Vec::new();
    for line in text.lines() {
        let l = line.split("//").next().unwrap_or("").trim();
        if !l.starts_with("include") || l.starts_with("includeBuild") {
            continue;
        }
        for s in l.split(['"', '\'']).skip(1).step_by(2) {
            out.push(s.trim_start_matches(':').replace(':', "/"));
        }
    }
    split("settings.gradle", out)
}

/// Project folders listed in a .sln: `Project("{…}") = "Name", "src\App\App.csproj", "{…}"`.
/// Solution folders (no project file) are skipped.
pub fn sln_projects(text: &str, file: &str) -> Declared {
    let mut out = Vec::new();
    for line in text.lines() {
        let l = line.trim();
        if !l.starts_with("Project(") {
            continue;
        }
        let Some(path) = l.split('"').nth(5) else {
            continue;
        };
        let lower = path.to_lowercase();
        if [".csproj", ".fsproj", ".vbproj"]
            .iter()
            .any(|e| lower.ends_with(e))
        {
            let p = path.replace('\\', "/");
            out.push(
                p.rsplit_once('/')
                    .map_or(String::new(), |(d, _)| d.to_owned()),
            );
        }
    }
    split(file, out)
}

/// Every declaration found in a folder.
pub fn declared(dir: &Path) -> Vec<Declared> {
    let read = |f: &str| std::fs::read_to_string(dir.join(f)).ok();
    let mut out = Vec::new();
    if let Some(d) = read("pnpm-workspace.yaml").map(|t| pnpm_workspace(&t)) {
        out.push(d);
    }
    if let Some(d) = read("package.json").and_then(|t| npm_workspaces(&t)) {
        out.push(d);
    }
    if let Some(d) = read("Cargo.toml").and_then(|t| cargo_workspace(&t)) {
        out.push(d);
    }
    for f in ["deno.json", "deno.jsonc"] {
        if let Some(d) = read(f).and_then(|t| deno_workspace(&t, f)) {
            out.push(d);
            break;
        }
    }
    if let Some(d) = read("go.work").map(|t| go_work(&t)) {
        out.push(d);
    }
    if let Some(d) = read("pyproject.toml").and_then(|t| uv_workspace(&t)) {
        out.push(d);
    }
    for f in ["settings.gradle.kts", "settings.gradle"] {
        if let Some(mut d) = read(f).map(|t| gradle_settings(&t)) {
            d.from = f.to_owned();
            out.push(d);
            break;
        }
    }
    if let Ok(rd) = std::fs::read_dir(dir) {
        let mut slns: Vec<String> = rd
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.to_lowercase().ends_with(".sln"))
            .collect();
        slns.sort();
        for n in slns {
            if let Some(t) = read(&n) {
                out.push(sln_projects(&t, &n));
            }
        }
    }
    out
}

fn globs(patterns: &[String]) -> GlobSet {
    let mut b = GlobSetBuilder::new();
    for p in patterns {
        // `*` stays within one folder, `**` crosses them, as in every workspace tool.
        if let Ok(g) = GlobBuilder::new(p).literal_separator(true).build() {
            b.add(g);
        }
    }
    b.build().unwrap_or_else(|_| GlobSet::empty())
}

/// `/`-separated path of `p` below `root`.
fn rel(p: &Path, root: &Path) -> Option<String> {
    let r = p.strip_prefix(root).ok()?;
    let s: Vec<_> = r
        .components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect();
    (!s.is_empty()).then(|| s.join("/"))
}

/// One declaration, compiled: the file it comes from, its include and exclude globs.
struct Compiled {
    from: String,
    include: GlobSet,
    exclude: GlobSet,
}

impl Compiled {
    /// Whether it claims `rel`: an include matches and no exclude does.
    fn claims(&self, rel: &str) -> bool {
        self.include.is_match(rel) && !self.exclude.is_match(rel)
    }
}

/// Fill `member_of` for every project a monorepo root declares. When several roots claim one
/// project, the nearest wins.
pub fn assign(projects: &mut [Project]) {
    // Only folders that contain another project can be roots: read nothing elsewhere.
    let candidates: Vec<usize> = (0..projects.len())
        .filter(|&i| {
            projects
                .iter()
                .any(|q| q.path != projects[i].path && q.path.starts_with(&projects[i].path))
        })
        .collect();
    let mut claims_by: Vec<(usize, Vec<Compiled>)> = Vec::new();
    for i in candidates {
        let ds: Vec<Compiled> = declared(&projects[i].path)
            .into_iter()
            .filter(|d| !d.include.is_empty())
            .map(|d| Compiled {
                include: globs(&d.include),
                exclude: globs(&d.exclude),
                from: d.from,
            })
            .collect();
        if !ds.is_empty() {
            claims_by.push((i, ds));
        }
    }
    for q in 0..projects.len() {
        let mut best: Option<(usize, String)> = None;
        for (r, ds) in &claims_by {
            let root = &projects[*r];
            let Some(rel) = rel(&projects[q].path, &root.path) else {
                continue;
            };
            // A root inside the one found so far is nearer to the project.
            let nearer = best
                .as_ref()
                .is_none_or(|(b, _)| root.path.starts_with(&projects[*b].path));
            if !nearer {
                continue;
            }
            if let Some(d) = ds.iter().find(|d| d.claims(&rel)) {
                best = Some((*r, d.from.clone()));
            }
        }
        projects[q].member_of = best.map(|(r, from)| Membership {
            root: projects[r].id.clone(),
            path: projects[r].path.clone(),
            from,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn npm_yarn_and_bun_workspaces() {
        let d = npm_workspaces(r#"{"workspaces":["packages/*","./apps/web/","!packages/old"]}"#)
            .unwrap();
        assert_eq!(d.include, ["packages/*", "apps/web"]);
        assert_eq!(d.exclude, ["packages/old"]);
        let y = npm_workspaces(r#"{"workspaces":{"packages":["libs/**"]}}"#).unwrap();
        assert_eq!(y.include, ["libs/**"]);
        assert!(npm_workspaces(r#"{"name":"x"}"#).is_none());
        assert!(npm_workspaces("not json").is_none());
    }

    #[test]
    fn pnpm_workspace_yaml() {
        let d = pnpm_workspace(
            "# root\npackages:\n  - 'apps/*'\n  - \"packages/**\" # all\n  - '!**/test/**'\ncatalog:\n  react: ^18\n",
        );
        assert_eq!(d.include, ["apps/*", "packages/**"]);
        assert_eq!(d.exclude, ["**/test/**"]);
        let f = pnpm_workspace("packages: ['a/*', \"b\"]\n");
        assert_eq!(f.include, ["a/*", "b"]);
    }

    #[test]
    fn cargo_and_uv_workspaces() {
        let c = cargo_workspace(
            "[workspace]\nmembers = [\"crates/*\", \"xtask\"]\nexclude = [\"crates/old\"]\n",
        )
        .unwrap();
        assert_eq!(
            (c.include, c.exclude),
            (
                vec!["crates/*".to_owned(), "xtask".into()],
                vec!["crates/old".to_owned()]
            )
        );
        assert!(cargo_workspace("[package]\nname = \"x\"\n").is_none());
        let u = uv_workspace(
            "[tool.uv.workspace]\nmembers = [\"packages/*\"]\nexclude = [\"packages/seeds\"]\n",
        )
        .unwrap();
        assert_eq!(u.include, ["packages/*"]);
        assert_eq!(u.exclude, ["packages/seeds"]);
        assert!(uv_workspace("[project]\nname = \"x\"\n").is_none());
    }

    #[test]
    fn deno_workspaces() {
        let d = deno_workspace(
            "{ // jsonc\n \"workspace\": [\"./add\", \"./sub/\"], }",
            "deno.jsonc",
        )
        .unwrap();
        assert_eq!(d.include, ["add", "sub"]);
        assert_eq!(d.from, "deno.jsonc workspace");
        assert!(deno_workspace(r#"{"tasks":{}}"#, "deno.json").is_none());
    }

    #[test]
    fn go_work_files() {
        let d = go_work("go 1.22\n\nuse (\n\t./api // the API\n\t./tools\n)\nuse ./cli\n");
        assert_eq!(d.include, ["api", "tools", "cli"]);
    }

    #[test]
    fn gradle_settings_files() {
        let d = gradle_settings(
            "rootProject.name = \"x\"\ninclude(\"app\", \":lib:core\")\ninclude 'tools'\nincludeBuild(\"build-logic\")\n// include(\"old\")\n",
        );
        assert_eq!(d.include, ["app", "lib/core", "tools"]);
    }

    #[test]
    fn solution_files() {
        let sln = "Project(\"{FAE04EC0}\") = \"Api\", \"src\\Api\\Api.csproj\", \"{1}\"\nEndProject\nProject(\"{2150E333}\") = \"Solution Items\", \"Solution Items\", \"{2}\"\nEndProject\nProject(\"{F2A71F9B}\") = \"Lib\", \"lib\\Lib.fsproj\", \"{3}\"\n";
        let d = sln_projects(sln, "App.sln");
        assert_eq!(d.include, ["src/Api", "lib"]);
        assert_eq!(d.from, "App.sln");
    }

    fn project(root: &Path, id: &str) -> Project {
        Project {
            id: id.into(),
            name: id.rsplit('/').next().unwrap().into(),
            path: root.join(id),
            ecosystems: vec![],
            branch: None,
            default_branch: None,
            tasks: vec![],
            warnings: vec![],
            hints: vec![],
            detected_from: vec![],
            member_of: None,
        }
    }

    #[test]
    fn assign_honours_globs_exclusions_and_the_nearest_root() {
        let tmp = tempfile::tempdir().unwrap();
        let r = tmp.path();
        std::fs::create_dir_all(r.join("shop/apps/web")).unwrap();
        std::fs::write(
            r.join("shop/pnpm-workspace.yaml"),
            "packages:\n  - 'apps/*'\n  - 'tools/*'\n  - '!tools/legacy'\n",
        )
        .unwrap();
        std::fs::create_dir_all(r.join("shop/apps/web/inner")).unwrap();
        std::fs::write(
            r.join("shop/apps/web/package.json"),
            r#"{"workspaces":["inner"]}"#,
        )
        .unwrap();
        let mut ps: Vec<Project> = [
            "shop",
            "shop/apps/web",
            "shop/apps/web/inner",
            "shop/tools/lint",
            "shop/tools/legacy",
            "shop/scratch",
            "other",
        ]
        .iter()
        .map(|id| project(r, id))
        .collect();
        assign(&mut ps);
        let of = |i: usize| {
            ps[i]
                .member_of
                .as_ref()
                .map(|m| (m.root.as_str(), m.from.as_str()))
        };
        assert_eq!(of(1), Some(("shop", "pnpm-workspace.yaml")));
        assert_eq!(
            of(2),
            Some(("shop/apps/web", "package.json workspaces")),
            "nearest root"
        );
        assert_eq!(of(3), Some(("shop", "pnpm-workspace.yaml")));
        assert_eq!(of(4), None, "excluded");
        assert_eq!(of(5), None, "not declared");
        assert_eq!((of(0), of(6)), (None, None));
    }
}
