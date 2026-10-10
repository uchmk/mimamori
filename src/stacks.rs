//! The panels inside a column: how a screen shares a column's room between
//! the panels it stacks there. A screen says what it wants (the default);
//! a divider dragged with the mouse leaves the same shape with other
//! shares, which is kept per screen and column while the shape stays.

use ito_layout::{Dir, MIN_RATIO, Node};

/// Two parts in `dir`, the first taking `ratio` of the room.
pub fn split(dir: Dir, ratio: f32, first: Node<u8>, second: Node<u8>) -> Node<u8> {
    Node::Split { dir, ratio: ratio.clamp(MIN_RATIO, 1.0 - MIN_RATIO), first: Box::new(first), second: Box::new(second) }
}

/// Panels `from..to` one after another in `dir`, each the same share.
pub fn chain(dir: Dir, from: u8, to: u8) -> Node<u8> {
    let mut n = nest(dir, from, to.max(from + 1));
    n.equalize();
    n
}

fn nest(dir: Dir, from: u8, to: u8) -> Node<u8> {
    if to - from == 1 { Node::Leaf(from) } else { split(dir, 0.5, Node::Leaf(from), nest(dir, from + 1, to)) }
}

/// Panels `0..n` in rows of `cols`, the rows stacked down; a short last
/// row shares the whole width. Every row the same height.
pub fn grid(n: u8, cols: u8) -> Node<u8> {
    let cols = cols.clamp(1, n.max(1));
    let starts: Vec<u8> = (0..n.max(1)).step_by(cols as usize).collect();
    let rows: Vec<Node<u8>> = starts.iter().map(|&s| nest(Dir::Right, s, (s + cols).min(n.max(1)))).collect();
    let mut node = rows.into_iter().rev().reduce(|below, row| split(Dir::Down, 0.5, row, below)).expect("one row at least");
    node.equalize();
    node
}

/// How many columns `n` panels need so that each is at least `min_h` tall
/// in `height`, at most `max_cols`.
pub fn cols_for(n: u8, height: f32, min_h: f32, max_cols: u8) -> u8 {
    (1..max_cols.max(1)).find(|&c| height / n.div_ceil(c).max(1) as f32 >= min_h).unwrap_or(max_cols.max(1))
}

/// The same splits, the same ways, around the same panels; only the shares
/// may differ.
pub fn same_shape(a: &Node<u8>, b: &Node<u8>) -> bool {
    match (a, b) {
        (Node::Leaf(x), Node::Leaf(y)) => x == y,
        (Node::Split { dir: d1, first: f1, second: s1, .. }, Node::Split { dir: d2, first: f2, second: s2, .. }) => {
            d1 == d2 && same_shape(f1, f2) && same_shape(s1, s2)
        }
        _ => false,
    }
}

/// Panels numbered `0..n`, each once: anything else in `state.toml` is not
/// trusted.
pub fn valid(n: &Node<u8>) -> bool {
    let mut leaves = n.leaves();
    leaves.sort_unstable();
    leaves.iter().enumerate().all(|(i, &l)| l as usize == i)
}

/// Each panel's room in a 1000 × 1000 column, for a scripted run's report:
/// `0=1000x333 1=1000x333 2=1000x333`.
pub fn sizes(n: &Node<u8>) -> String {
    let mut parts = n.layout(ito_layout::Rect::new(0.0, 0.0, 1000.0, 1000.0), 0.0);
    parts.sort_by_key(|(i, _)| *i);
    parts.iter().map(|(i, r)| format!("{i}={:.0}x{:.0}", r.w, r.h)).collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chains_and_grids_share_the_room_equally() {
        assert_eq!(chain(Dir::Down, 0, 1), Node::Leaf(0));
        assert_eq!(sizes(&chain(Dir::Down, 0, 3)), "0=1000x333 1=1000x334 2=1000x333");
        assert_eq!(sizes(&chain(Dir::Right, 0, 2)), "0=500x1000 1=500x1000");
        // Six gauges in two columns: three rows of two.
        assert_eq!(sizes(&grid(6, 2)), "0=500x333 1=500x333 2=500x334 3=500x334 4=500x333 5=500x333");
        // Five: the last one alone across the width.
        assert_eq!(sizes(&grid(5, 2)), "0=500x333 1=500x333 2=500x334 3=500x334 4=1000x333");
        assert_eq!(grid(3, 1), chain(Dir::Down, 0, 3));
        assert!(valid(&grid(7, 3)) && valid(&chain(Dir::Right, 0, 4)));
        assert!(!valid(&chain(Dir::Right, 1, 3)));
    }

    #[test]
    fn columns_are_added_only_when_the_panels_get_too_short() {
        assert_eq!(cols_for(6, 720.0, 90.0, 3), 1);
        assert_eq!(cols_for(6, 400.0, 90.0, 3), 2);
        assert_eq!(cols_for(9, 200.0, 90.0, 3), 3);
        assert_eq!(grid(1, cols_for(1, 10.0, 90.0, 3)), Node::Leaf(0));
    }

    #[test]
    fn the_shape_ignores_the_shares() {
        let a = split(Dir::Down, 0.4, chain(Dir::Right, 0, 2), Node::Leaf(2));
        let mut b = a.clone();
        b.set_ratio(&[], 0.7);
        assert!(same_shape(&a, &b));
        assert_ne!(a, b);
        assert!(!same_shape(&a, &chain(Dir::Down, 0, 3)));
        assert!(!same_shape(&chain(Dir::Down, 0, 3), &chain(Dir::Right, 0, 3)));
    }
}
