// SPDX-License-Identifier: MIT OR Apache-2.0
//! Hardening of the release workflow that dist generates (`cargo xtask dist-generate`).
//!
//! dist writes `.github/workflows/release.yml` from its own template and rewrites it on every
//! upgrade, so it cannot be fixed by hand. What zizmor and actionlint find in it is fixed here,
//! after each generation, instead of being ignored:
//!
//! - permissions: none at the workflow level, and per job only what it needs;
//! - `custom-publish-tap` grants `contents: read` only (publish-tap.yml asks for it: without it
//!   the whole workflow fails to start; it needs no id-token or packages) and passes no secrets (publish-tap reads its key from its own
//!   `release` environment);
//! - every `${{ … }}` of a `run:` moves into an environment variable; steps that ran in the
//!   runner's default shell keep it (bash, pwsh on Windows), as two steps;
//! - no `container:` (dist sets none for kiwa's targets);
//! - `steps.cargo-cyclonedx.output.paths` reads `outputs`, so the SBOMs are uploaded.
//!
//! Each rewrite is idempotent and fails when dist's output no longer has the expected shape, so
//! a dist upgrade that changes the template is noticed instead of silently left unhardened.

/// One rewrite: `from` becomes `to`. Already applied when `to` is there and `from` is not.
struct Rewrite {
    why: &'static str,
    from: &'static str,
    to: &'static str,
}

const REWRITES: &[Rewrite] = &[
    Rewrite {
        why: "no permissions at the workflow level",
        from: "name: Release\npermissions:\n  \"contents\": \"write\"\n",
        to: "name: Release\n# Each job asks for what it needs (xtask/src/release.rs).\npermissions: {}\n",
    },
    Rewrite {
        why: "plan: creates the release on tags",
        from: "  plan:\n    runs-on: \"ubuntu-22.04\"\n",
        to: "  plan:\n    runs-on: \"ubuntu-22.04\"\n    permissions:\n      \"contents\": \"write\"\n",
    },
    Rewrite {
        why: "plan: the dist command through the environment",
        from: "      - id: plan\n        run: |\n          dist ${{ (!github.event.pull_request && format('host --steps=create --tag={0}', github.ref_name)) || 'plan' }} --output-format=json > plan-dist-manifest.json\n",
        to: "      - id: plan\n        env:\n          DIST_STEP: ${{ (!github.event.pull_request && format('host --steps=create --tag={0}', github.ref_name)) || 'plan' }}\n        run: |\n          dist $DIST_STEP --output-format=json > plan-dist-manifest.json\n",
    },
    Rewrite {
        why: "build-local-artifacts: no container (dist sets none for kiwa's targets)",
        from: "    container: ${{ matrix.container && matrix.container.image || null }}\n",
        to: "",
    },
    Rewrite {
        why: "build-local-artifacts: install dist through the environment, in the default shell",
        from: "      - name: Install dist\n        run: ${{ matrix.install_dist.run }}\n",
        to: "      - name: Install dist\n        if: runner.os != 'Windows'\n        shell: bash\n        env:\n          INSTALL_DIST: ${{ matrix.install_dist.run }}\n        run: eval \"$INSTALL_DIST\"\n      - name: Install dist (Windows)\n        if: runner.os == 'Windows'\n        shell: pwsh\n        env:\n          INSTALL_DIST: ${{ matrix.install_dist.run }}\n        run: Invoke-Expression $env:INSTALL_DIST\n",
    },
    Rewrite {
        why: "build-local-artifacts: system packages through the environment",
        from: "      - name: Install dependencies\n        run: |\n          ${{ matrix.packages_install }}\n",
        to: "      - name: Install dependencies\n        if: runner.os != 'Windows'\n        shell: bash\n        env:\n          PACKAGES_INSTALL: ${{ matrix.packages_install }}\n        run: eval \"$PACKAGES_INSTALL\"\n      - name: Install dependencies (Windows)\n        if: runner.os == 'Windows'\n        shell: pwsh\n        env:\n          PACKAGES_INSTALL: ${{ matrix.packages_install }}\n        run: if ($env:PACKAGES_INSTALL) { Invoke-Expression $env:PACKAGES_INSTALL }\n",
    },
    Rewrite {
        why: "build-local-artifacts: the build command through the environment",
        from: "      - name: Build artifacts\n        run: |\n          # Actually do builds and make zips and whatnot\n          dist build ${{ needs.plan.outputs.tag-flag }} --print=linkage --output-format=json ${{ matrix.dist_args }} > dist-manifest.json\n          echo \"dist ran successfully\"\n",
        to: "      - name: Build artifacts\n        if: runner.os != 'Windows'\n        shell: bash\n        env:\n          TAG_FLAG: ${{ needs.plan.outputs.tag-flag }}\n          DIST_ARGS: ${{ matrix.dist_args }}\n        run: |\n          # Actually do builds and make zips and whatnot\n          dist build $TAG_FLAG --print=linkage --output-format=json $DIST_ARGS > dist-manifest.json\n          echo \"dist ran successfully\"\n      - name: Build artifacts (Windows)\n        if: runner.os == 'Windows'\n        shell: pwsh\n        env:\n          TAG_FLAG: ${{ needs.plan.outputs.tag-flag }}\n          DIST_ARGS: ${{ matrix.dist_args }}\n        run: |\n          Invoke-Expression \"dist build $env:TAG_FLAG --print=linkage --output-format=json $env:DIST_ARGS > dist-manifest.json\"\n          echo \"dist ran successfully\"\n",
    },
    Rewrite {
        why: "build-global-artifacts: read only",
        from: "      - build-local-artifacts\n    runs-on: \"ubuntu-22.04\"\n    env:\n      GH_TOKEN: ${{ secrets.GITHUB_TOKEN }}\n      BUILD_MANIFEST_NAME: target/distrib/global-dist-manifest.json\n",
        to: "      - build-local-artifacts\n    runs-on: \"ubuntu-22.04\"\n    permissions:\n      \"contents\": \"read\"\n    env:\n      GH_TOKEN: ${{ secrets.GITHUB_TOKEN }}\n      BUILD_MANIFEST_NAME: target/distrib/global-dist-manifest.json\n",
    },
    Rewrite {
        why: "build-global-artifacts: the tag flag through the environment",
        from: "      - id: cargo-dist\n        shell: bash\n        run: |\n          dist build ${{ needs.plan.outputs.tag-flag }} --output-format=json \"--artifacts=global\" > dist-manifest.json\n",
        to: "      - id: cargo-dist\n        shell: bash\n        env:\n          TAG_FLAG: ${{ needs.plan.outputs.tag-flag }}\n        run: |\n          dist build $TAG_FLAG --output-format=json \"--artifacts=global\" > dist-manifest.json\n",
    },
    Rewrite {
        why: "build-global-artifacts: upload the SBOMs (dist reads `output`, not `outputs`)",
        from: "            ${{ steps.cargo-cyclonedx.output.paths }}\n",
        to: "            ${{ steps.cargo-cyclonedx.outputs.paths }}\n",
    },
    Rewrite {
        why: "host: uploads the release assets",
        from: "    runs-on: \"ubuntu-22.04\"\n    outputs:\n      val: ${{ steps.host.outputs.manifest }}\n",
        to: "    runs-on: \"ubuntu-22.04\"\n    permissions:\n      \"contents\": \"write\"\n    outputs:\n      val: ${{ steps.host.outputs.manifest }}\n",
    },
    Rewrite {
        why: "host: the tag flag through the environment",
        from: "      - id: host\n        shell: bash\n        run: |\n          dist host ${{ needs.plan.outputs.tag-flag }} --steps=upload",
        to: "      - id: host\n        shell: bash\n        env:\n          TAG_FLAG: ${{ needs.plan.outputs.tag-flag }}\n        run: |\n          dist host $TAG_FLAG --steps=upload",
    },
    Rewrite {
        why: "host: the release tag through the environment",
        from: "          RELEASE_COMMIT: \"${{ github.sha }}\"\n        run: |\n          # Write and read notes from a file to avoid quoting breaking things\n          echo \"$ANNOUNCEMENT_BODY\" > $RUNNER_TEMP/notes.txt\n\n          gh release create \"${{ needs.plan.outputs.tag }}\"",
        to: "          RELEASE_COMMIT: \"${{ github.sha }}\"\n          RELEASE_TAG: \"${{ needs.plan.outputs.tag }}\"\n        run: |\n          # Write and read notes from a file to avoid quoting breaking things\n          echo \"$ANNOUNCEMENT_BODY\" > \"$RUNNER_TEMP/notes.txt\"\n\n          gh release create \"$RELEASE_TAG\"",
    },
    Rewrite {
        why: "custom-publish-tap: contents: read for publish-tap.yml, and no inherited secrets",
        from: "    secrets: inherit\n    # publish jobs get escalated permissions\n    permissions:\n      \"id-token\": \"write\"\n      \"packages\": \"write\"\n",
        to: "    # publish-tap.yml reads HOMEBREW_TAP_SSH_KEY from its own `release` environment (no secrets to\n    # pass) and pushes to the tap over SSH: it only reads the release, so contents: read, and no\n    # id-token or packages.\n    permissions:\n      \"contents\": \"read\"\n",
    },
    Rewrite {
        why: "announce: read only",
        from: "    if: ${{ always() && needs.host.result == 'success' && (needs.custom-publish-tap.result == 'skipped' || needs.custom-publish-tap.result == 'success') }}\n    runs-on: \"ubuntu-22.04\"\n",
        to: "    if: ${{ always() && needs.host.result == 'success' && (needs.custom-publish-tap.result == 'skipped' || needs.custom-publish-tap.result == 'success') }}\n    runs-on: \"ubuntu-22.04\"\n    permissions:\n      \"contents\": \"read\"\n",
    },
];

/// dist writes local reusable workflows with GitHub's self-repository syntax (`uses: $/…`), which
/// zizmor asks for; an older rewrite of this tool had turned it into `./…`, put back here.
const LOCAL_USES: (&str, &str) = ("uses: ./.github/workflows/", "uses: $/.github/workflows/");

/// The hardened workflow. An error names the rewrite whose source text dist no longer writes.
pub fn harden(text: &str) -> Result<String, String> {
    let mut out = text.replace(LOCAL_USES.0, LOCAL_USES.1);
    for r in REWRITES {
        // Already applied: its result is there (checked first, since a result may contain its
        // source, e.g. a permissions block added after a line), or the removed line is gone.
        let applied = if r.to.is_empty() {
            !out.contains(r.from)
        } else {
            out.contains(r.to)
        };
        if applied {
            continue;
        }
        if !out.contains(r.from) {
            return Err(format!(
                "release.yml no longer has the shape this rewrite expects: {}; update xtask/src/release.rs",
                r.why
            ));
        }
        out = out.replacen(r.from, r.to, 1);
    }
    let left = expressions_in_run(&out);
    if !left.is_empty() {
        return Err(format!(
            "release.yml runs `${{{{ … }}}}` in a shell on line(s) {left:?}: move them into `env:` in xtask/src/release.rs"
        ));
    }
    Ok(out)
}

/// Lines (1-based) where a `${{` appears inside a `run:` script, the template-injection shape.
pub fn expressions_in_run(text: &str) -> Vec<usize> {
    let mut found = Vec::new();
    let mut run_indent: Option<usize> = None;
    for (i, line) in text.lines().enumerate() {
        let trimmed = line.trim_start();
        let indent = line.len() - trimmed.len();
        if let Some(at) = run_indent {
            if trimmed.is_empty() || indent > at {
                if line.contains("${{") {
                    found.push(i + 1);
                }
                continue;
            }
            run_indent = None;
        }
        // `- run: |`: the key's column is after the dash, so `env:` below it is not the script.
        let (key, key_col) = match trimmed.strip_prefix("- ") {
            Some(k) => (k, indent + 2),
            None => (trimmed, indent),
        };
        if let Some(value) = key.strip_prefix("run:") {
            let value = value.trim();
            if value.starts_with('|') || value.starts_with('>') {
                run_indent = Some(key_col);
            } else if value.contains("${{") {
                found.push(i + 1);
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn committed() -> String {
        std::fs::read_to_string(crate::root().join(".github/workflows/release.yml")).unwrap()
    }

    /// The committed workflow is the hardened one: hardening it again changes nothing.
    #[test]
    fn the_committed_release_workflow_is_hardened() {
        let text = committed();
        assert_eq!(
            harden(&text).unwrap(),
            text,
            "run `cargo xtask dist-generate`"
        );
        assert!(!text.contains("secrets: inherit"));
        assert!(text.contains("permissions: {}\n"));
        assert!(!text.contains("container:"));
        assert!(text.contains("steps.cargo-cyclonedx.outputs.paths"));
    }

    #[test]
    fn expressions_in_scripts_are_found() {
        let wf = "jobs:\n  a:\n    steps:\n      - run: echo ${{ github.ref }}\n      - run: |\n          safe\n          echo ${{ matrix.x }}\n        env:\n          X: ${{ matrix.x }}\n      - uses: x\n        with:\n          path: ${{ env.Y }}\n";
        assert_eq!(
            expressions_in_run(wf),
            [4, 7],
            "env: and with: are not scripts"
        );
    }

    #[test]
    fn an_unexpected_template_is_reported() {
        let err = harden("name: Release\npermissions:\n  \"contents\": \"read\"\n").unwrap_err();
        assert!(
            err.contains("no permissions at the workflow level"),
            "{err}"
        );
    }
}
