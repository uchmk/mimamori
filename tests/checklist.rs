//! TESTING.md's rows keep the form the lane scripts read: `wintest-queue.ps1`
//! picks `- [ ] 3d.8 text` rows by their section, `merge-lanes.py` lets a lane
//! change only the mark, and a `retest` issue names rows by their ID. A row
//! that slips out of the form is silently skipped by all three, so this test
//! (and the Checklists workflow that runs it) says so instead.

const TESTING: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/TESTING.md"));

/// `## 3d. Disks (`8`)` → `3d`.
fn heading(line: &str) -> Option<&str> {
    let rest = line.strip_prefix("## ")?;
    let (id, _) = rest.split_once(". ")?;
    section_id(id).then_some(id)
}

/// `3`, `3d`: digits, then at most one lower-case letter.
fn section_id(s: &str) -> bool {
    let digits = s.trim_end_matches(|c: char| c.is_ascii_lowercase());
    !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) && s.len() - digits.len() <= 1
}

/// `3d.8` → (`3d`, `8`).
fn row_id(s: &str) -> Option<(&str, &str)> {
    let (section, n) = s.split_once('.')?;
    (section_id(section) && !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit())).then_some((section, n))
}

/// What is wrong with the file, one line each.
fn problems(text: &str) -> (Vec<String>, usize) {
    let mut out = Vec::new();
    let mut section: Option<&str> = None;
    let mut seen = std::collections::HashMap::new();
    let mut rows = 0;
    for (i, line) in text.lines().enumerate() {
        let n = i + 1;
        if line.starts_with("## ") {
            section = heading(line);
            if section.is_none() {
                out.push(format!("line {n}: a section heading reads `## <id>. <title>` (id like 3 or 3d): {line}"));
            }
            continue;
        }
        if !line.trim_start().starts_with("- [") {
            continue;
        }
        if line.starts_with(' ') || line.starts_with('\t') {
            out.push(format!("line {n}: a row is not indented: {line}"));
            continue;
        }
        rows += 1;
        let Some(rest) = line.strip_prefix("- [") else { continue };
        let mut chars = rest.chars();
        let mark = chars.next();
        let rest = chars.as_str();
        if !matches!(mark, Some(' ' | 'x' | '~')) || !rest.starts_with("] ") {
            out.push(format!("line {n}: the mark is one of `[ ]`, `[x]`, `[~]`: {line}"));
            continue;
        }
        let rest = &rest[2..];
        let id = rest.split(' ').next().unwrap_or("");
        let Some((row_section, _)) = row_id(id) else {
            out.push(format!("line {n}: the row starts with its ID (`3d.8`) and a space: {line}"));
            continue;
        };
        if rest.len() == id.len() || rest[id.len()..].trim().is_empty() {
            out.push(format!("line {n}: row {id} says nothing after its ID"));
        }
        match section {
            None => out.push(format!("line {n}: row {id} is not under a `## <id>. <title>` heading")),
            Some(s) if s != row_section => out.push(format!("line {n}: row {id} is under section {s}")),
            Some(_) => {}
        }
        if let Some(first) = seen.insert(id.to_string(), n) {
            out.push(format!("line {n}: row {id} is also on line {first}"));
        }
    }
    (out, rows)
}

#[test]
fn rows_keep_their_form() {
    let (problems, rows) = problems(TESTING);
    assert!(rows > 0, "TESTING.md has no rows");
    assert!(problems.is_empty(), "TESTING.md:\n{}", problems.join("\n"));
}

#[test]
fn a_bad_row_is_caught() {
    let text = "## 1. Start\n\n- [x] 1.1 Fine.\n- [?] 1.2 Bad mark.\n- [ ] 2.1 Wrong section.\n- [ ] 1.1 Twice.\n- [ ] No ID.\n  - [ ] 1.3 Indented.\n## Notes\n- [ ] 1.4 Under no section.\n";
    let (problems, rows) = problems(text);
    assert_eq!(rows, 6, "{problems:?}");
    let want = [
        "line 4: the mark",
        "line 5: row 2.1 is under section 1",
        "line 6: row 1.1 is also on line 3",
        "line 7: the row starts",
        "line 8: a row is not indented",
        "line 9: a section heading",
        "line 10: row 1.4 is not under",
    ];
    assert_eq!(problems.len(), want.len(), "{problems:#?}");
    for (p, w) in problems.iter().zip(want) {
        assert!(p.starts_with(w), "{p:?} should start with {w:?}");
    }
}
