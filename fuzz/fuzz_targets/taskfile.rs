// SPDX-License-Identifier: MIT OR Apache-2.0
//! Any input: the parser must return, never panic or hang.
#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(text) = std::str::from_utf8(data) {
        let _ = kiwa_core::detect::parse_taskfile_tasks(text);
        let _ = kiwa_core::detect::taskfile_requires(text);
        let _ = kiwa_core::workspace::pnpm_workspace(text);
        let _ = kiwa_core::detect::parse_compose_services(text);
        let _ = kiwa_core::detect::jsonc(text);
        let _ = kiwa_core::words::needs_shell(text);
        let _ = kiwa_core::sources::github_jobs(text);
        let _ = kiwa_core::sources::gitlab_jobs(text);
        let _ = kiwa_core::sources::vscode_tasks(text);
        let _ = kiwa_core::sources::parse_procfile(text);
        let _ = kiwa_core::sources::readme_steps(text);
    }
});
