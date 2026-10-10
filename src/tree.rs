//! The procs list as a tree: each process under its parent, siblings in the
//! list's sort order, with the branch lines drawn in front of the name.
//!
//! Only the shown processes (the scope and the filter) are in the tree; one
//! whose parent is hidden hangs under its nearest shown ancestor, or is a
//! root. A parent that started after the child is not its parent but a
//! newer process given the parent's old PID (Windows reuses them), so the
//! child is treated as an orphan.

use std::collections::HashMap;

use crate::collect::Proc;

/// The rows in tree order, each with the branch lines to draw before its
/// name. `shown` holds indices into `procs`, already sorted.
pub fn order(procs: &[Proc], shown: &[usize]) -> Vec<(usize, String)> {
    let by_pid: HashMap<u32, usize> = procs.iter().enumerate().map(|(i, p)| (p.pid, i)).collect();
    let mut is_shown = vec![false; procs.len()];
    for &i in shown {
        is_shown[i] = true;
    }
    let mut children: HashMap<usize, Vec<usize>> = HashMap::new();
    let mut roots = Vec::new();
    for &i in shown {
        match parent(procs, &by_pid, &is_shown, i) {
            Some(p) => children.entry(p).or_default().push(i),
            None => roots.push(i),
        }
    }

    let mut out = Vec::with_capacity(shown.len());
    let mut seen = vec![false; procs.len()];
    // Roots first; then anything a parent loop left unreached, as a root.
    for &r in roots.iter().chain(shown) {
        if seen[r] {
            continue;
        }
        // (process, its line, the lines its children continue from)
        let mut stack = vec![(r, String::new(), String::new())];
        while let Some((i, line, under)) = stack.pop() {
            if std::mem::replace(&mut seen[i], true) {
                continue;
            }
            out.push((i, line));
            let kids: Vec<usize> = children.get(&i).map_or_else(Vec::new, |k| k.iter().copied().filter(|&k| !seen[k]).collect());
            for (n, &k) in kids.iter().enumerate().rev() {
                let last = n + 1 == kids.len();
                let line = format!("{under}{}", if last { "└ " } else { "├ " });
                let next = format!("{under}{}", if last { "  " } else { "│ " });
                stack.push((k, line, next));
            }
        }
    }
    out
}

/// The nearest shown ancestor of `i`, walking up through hidden ones.
fn parent(procs: &[Proc], by_pid: &HashMap<u32, usize>, shown: &[bool], i: usize) -> Option<usize> {
    let mut cur = i;
    // A loop of PIDs cannot walk forever: no chain is longer than the list.
    for _ in 0..procs.len() {
        let c = &procs[cur];
        let &p = by_pid.get(&c.ppid?)?;
        let pp = &procs[p];
        if p == cur || (pp.start != 0 && c.start != 0 && pp.start > c.start) {
            return None;
        }
        if shown[p] {
            return (p != i).then_some(p);
        }
        cur = p;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(pid: u32, ppid: Option<u32>, start: u64) -> Proc {
        Proc {
            pid,
            ppid,
            name: format!("p{pid}"),
            cpu: 0.0,
            mem: 0,
            threads: None,
            user: None,
            cmd: String::new(),
            exe: None,
            start,
            status: String::new(),
        }
    }

    /// `pid line` for each row.
    fn show(procs: &[Proc], shown: &[usize]) -> Vec<String> {
        order(procs, shown).into_iter().map(|(i, l)| format!("{l}{}", procs[i].pid)).collect()
    }

    #[test]
    fn children_hang_under_their_parent_in_the_given_order() {
        let procs = [p(1, None, 0), p(10, Some(1), 0), p(11, Some(1), 0), p(20, Some(10), 0), p(2, None, 0)];
        // Sorted with 11 before 10: siblings keep that order.
        assert_eq!(show(&procs, &[4, 0, 2, 1, 3]), ["2", "1", "├ 11", "└ 10", "  └ 20"]);
        let procs = [p(1, None, 0), p(10, Some(1), 0), p(20, Some(10), 0), p(11, Some(1), 0)];
        assert_eq!(show(&procs, &[0, 1, 2, 3]), ["1", "├ 10", "│ └ 20", "└ 11"]);
    }

    #[test]
    fn a_hidden_parent_is_skipped_to_the_nearest_shown_ancestor() {
        let procs = [p(1, None, 0), p(10, Some(1), 0), p(20, Some(10), 0)];
        assert_eq!(show(&procs, &[0, 2]), ["1", "└ 20"]);
        assert_eq!(show(&procs, &[2]), ["20"]);
    }

    #[test]
    fn a_parent_started_after_the_child_is_a_reused_pid() {
        let procs = [p(1, None, 100), p(10, Some(1), 50)];
        assert_eq!(show(&procs, &[0, 1]), ["1", "10"]);
    }

    #[test]
    fn a_loop_of_parents_still_shows_every_process_once() {
        let procs = [p(1, Some(2), 0), p(2, Some(1), 0), p(3, Some(3), 0)];
        // 3 is its own parent, so a root; 1 and 2 come after, from the loop.
        assert_eq!(show(&procs, &[0, 1, 2]), ["3", "1", "└ 2"]);
    }
}
