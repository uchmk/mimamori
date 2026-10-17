//! F12: open a bug report with the version, the OS and what yagura knew already
//! filled in. The lines are `ito-bugreport`'s, the same in every uchmk app; this
//! file gives it yagura's name, repository and rows.

use std::path::PathBuf;

use crate::config::settings::{self, Settings};

const REPO: &str = "https://github.com/uchmk/yagura";
const TEMPLATE: &str = "bug_report.yml";

/// The link that opens the report: the `os`, `version` and `context` fields of
/// `.github/ISSUE_TEMPLATE/bug_report.yml` filled in. `scale` is common.toml's.
pub fn url(s: &Settings, scale: f32) -> String {
    let spec = ito_bugreport::Spec { app: "yagura", version: env!("CARGO_PKG_VERSION"), repo: REPO, template: TEMPLATE };
    ito_bugreport::url(&spec, &[("context", context(s, scale))])
}

fn context(s: &Settings, scale: f32) -> String {
    let home = ito_common::home_dir();
    let files = read_files(s);
    // What the loader complained about is the first thing a report needs.
    let rows = [("Warnings", s.warnings.join("; ")), ("Backend", s.backend.clone().unwrap_or_else(|| "auto".into()))];
    ito_bugreport::context(&ito_bugreport::Context {
        home: home.as_deref(),
        scale: Some(scale),
        config: &files,
        anchors: &["uchmk", "yagura"],
        rows: &rows,
        ..Default::default()
    })
}

/// The config files that exist, in the order they are read.
fn read_files(s: &Settings) -> Vec<PathBuf> {
    let mut files = Vec::new();
    if let Some(base) = settings::base_dir() {
        files.push(base.join("common.toml"));
    }
    if let Some(own) = &s.dir {
        files.extend(["config.toml", "keymap.toml"].map(|n| own.join(n)));
    }
    files.retain(|p| p.is_file());
    files
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The ids `url` fills must exist in the form, or GitHub drops them silently.
    #[test]
    fn the_form_has_the_fields_the_link_fills() {
        let form = include_str!("../.github/ISSUE_TEMPLATE/bug_report.yml");
        for id in ["version", "os", "context"] {
            assert!(form.contains(&format!("id: {id}\n")), "bug_report.yml has no field `{id}`");
        }
        assert!(!form.contains("{{app}}"), "the template's placeholder was not replaced");
    }

    #[test]
    fn the_link_names_the_app_and_never_a_path() {
        let dir = crate::util::test_dir("bugreport");
        let s = settings::load(Some(&dir));
        let u = url(&s, 1.0);
        assert!(u.starts_with("https://github.com/uchmk/yagura/issues/new?template=bug_report.yml&version=yagura%20"), "{u}");
        assert!(u.contains("&context="), "{u}");
        let ctx = context(&s, 1.0);
        assert!(!ctx.contains(&*dir.to_string_lossy()), "a folder of this machine went into the context: {ctx}");
    }
}
