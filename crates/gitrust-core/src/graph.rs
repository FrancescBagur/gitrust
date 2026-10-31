//! Working out the layout of the commit graph.
//!
//! The algorithm gives each commit a lane (a vertical column) and produces the
//! lines joining each row to the next. It walks the commits from top to bottom
//! keeping, for every live lane, which commit it expects to find further down.
//!
//! When a commit arrives:
//!
//! 1. The lanes that were expecting it converge on it. The commit takes the
//!    leftmost free lane of that group, and the lines in the band above are
//!    corrected so that they end there.
//! 2. Its first parent carries on in that same lane, which keeps a branch's
//!    colour steady along its history.
//! 3. Every further parent (a merge) opens a new lane to its right.

use git2::Oid;

use crate::model::{Commit, Graph, Link, Row};

/// Takes a lane, reusing a free gap before widening the graph.
fn alloc_lane(lanes: &mut Vec<Option<Oid>>, colors: &mut Vec<usize>, color: usize) -> usize {
    match lanes.iter().position(Option::is_none) {
        Some(index) => {
            colors[index] = color;
            index
        }
        None => {
            lanes.push(None);
            colors.push(color);
            lanes.len() - 1
        }
    }
}

/// Turns a list of commits into a graph with lanes and lines.
pub fn layout(commits: Vec<Commit>) -> Graph {
    let mut rows: Vec<Row> = Vec::with_capacity(commits.len());

    // For each lane, which commit it expects to find further down.
    let mut lanes: Vec<Option<Oid>> = Vec::new();
    let mut colors: Vec<usize> = Vec::new();
    let mut next_color = 0usize;
    let mut lane_count = 0usize;

    for commit in commits {
        // Lanes that were expecting exactly this commit.
        let expecting: Vec<usize> = lanes
            .iter()
            .enumerate()
            .filter_map(|(index, slot)| (slot.as_ref() == Some(&commit.id)).then_some(index))
            .collect();

        // The commit goes in the first of them; if nobody was expecting it, it
        // is the tip of a branch and takes a fresh lane.
        let lane = match expecting.first() {
            Some(&index) => index,
            None => {
                let color = next_color;
                next_color += 1;
                alloc_lane(&mut lanes, &mut colors, color)
            }
        };
        let color = colors[lane];

        // The lines in the band above were drawn pointing at their own lane,
        // because we did not yet know where this commit would land. Now we do.
        if let Some(previous) = rows.last_mut() {
            for link in previous.links.iter_mut() {
                if expecting.contains(&link.to) {
                    link.to = lane;
                }
            }
        }

        // The lanes that converged here are free again.
        for &index in &expecting {
            lanes[index] = None;
        }

        // The first parent inherits the lane; the rest open a new one unless
        // another lane was already expecting them.
        let mut touched = vec![lane];
        let mut parents = commit.parents.iter();
        match parents.next() {
            Some(&first) => {
                lanes[lane] = Some(first);
                colors[lane] = color;
                for &parent in parents {
                    if lanes.iter().any(|slot| slot.as_ref() == Some(&parent)) {
                        continue;
                    }
                    let branch_color = next_color;
                    next_color += 1;
                    let index = alloc_lane(&mut lanes, &mut colors, branch_color);
                    lanes[index] = Some(parent);
                    touched.push(index);
                }
            }
            // A root commit closes its lane.
            None => lanes[lane] = None,
        }

        // With the lanes brought up to date, the lines towards the next row:
        // the ones this commit has just opened leave from its dot, the rest run
        // straight down their own lane.
        let links = lanes
            .iter()
            .enumerate()
            .filter(|(_, slot)| slot.is_some())
            .map(|(index, _)| Link {
                from: if touched.contains(&index) { lane } else { index },
                to: index,
                color: colors[index],
            })
            .collect();

        lane_count = lane_count.max(lanes.len());
        rows.push(Row { commit, lane, color, links });
    }

    // The last row has no band below it to draw.
    if let Some(last) = rows.last_mut() {
        last.links.clear();
    }

    Graph { rows, lane_count }
}

/// The same as `layout`, but with a row of uncommitted changes above
/// everything, hanging off HEAD.
///
/// The row enters the layout as one more commit, with HEAD for a parent, so
/// the algorithm places it on its own: it takes a lane, HEAD converges on it
/// and inherits its colour, and the line joining them is drawn by the same
/// code that joins two commits. If HEAD is not the newest thing in the history
/// -- because you are on an old branch -- that line runs down the left lane to
/// wherever it is, which is exactly what you need to see.
///
/// If HEAD is not in the list (because the limit cut it off) nothing is added:
/// a line running to the bottom without reaching anywhere would be saying
/// something untrue.
pub fn layout_with_working(commits: Vec<Commit>, head: Oid) -> Graph {
    if !commits.iter().any(|commit| commit.id == head) {
        return layout(commits);
    }

    let mut all = Vec::with_capacity(commits.len() + 1);
    all.push(Commit::working(head));
    all.extend(commits);
    layout(all)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Commit;

    /// A deterministic Oid from a number, so the tests read well.
    fn oid(n: u8) -> Oid {
        Oid::from_str(&format!("{n:040x}")).unwrap()
    }

    fn commit(id: u8, parents: &[u8]) -> Commit {
        Commit {
            id: oid(id),
            short_id: format!("{id:07x}"),
            summary: format!("commit {id}"),
            author: "test".to_string(),
            time: 0,
            parents: parents.iter().copied().map(oid).collect(),
            refs: Vec::new(),
        }
    }

    #[test]
    fn a_linear_history_takes_one_single_lane() {
        let graph = layout(vec![commit(3, &[2]), commit(2, &[1]), commit(1, &[])]);

        assert_eq!(graph.lane_count, 1);
        assert!(graph.rows.iter().all(|row| row.lane == 0));

        // Every row but the last runs straight down lane 0.
        for row in &graph.rows[..2] {
            assert_eq!(row.links.len(), 1);
            assert_eq!((row.links[0].from, row.links[0].to), (0, 0));
        }
        assert!(graph.rows[2].links.is_empty());
    }

    #[test]
    fn a_merge_opens_and_closes_a_lane() {
        // M merges A and B, which both come off the root R.
        let graph = layout(vec![
            commit(4, &[3, 2]),
            commit(3, &[1]),
            commit(2, &[1]),
            commit(1, &[]),
        ]);

        assert_eq!(graph.lane_count, 2);
        let lanes: Vec<usize> = graph.rows.iter().map(|row| row.lane).collect();
        assert_eq!(lanes, vec![0, 0, 1, 0], "M and A in lane 0, B in lane 1, R back in lane 0");

        // The merge leaves its dot towards the new lane.
        let merge = &graph.rows[0];
        assert!(merge.links.iter().any(|l| l.from == 0 && l.to == 1));

        // And lane 1 converges on the root, which is in lane 0.
        let before_root = &graph.rows[2];
        assert!(before_root.links.iter().any(|l| l.from == 1 && l.to == 0));
    }

    #[test]
    fn the_first_parent_keeps_the_branchs_color() {
        let graph = layout(vec![commit(4, &[3, 2]), commit(3, &[1]), commit(2, &[1]), commit(1, &[])]);

        // M, A and R share lane 0, so they share a colour.
        assert_eq!(graph.rows[0].color, graph.rows[1].color);
        assert_eq!(graph.rows[1].color, graph.rows[3].color);
        // The merged branch takes a fresh colour.
        assert_ne!(graph.rows[2].color, graph.rows[0].color);
    }

    #[test]
    fn the_uncommitted_changes_hang_off_head_and_wear_its_color() {
        let graph = layout_with_working(vec![commit(2, &[1]), commit(1, &[])], oid(2));

        assert_eq!(graph.rows.len(), 3, "la fila de cambios va por delante");
        let working = &graph.rows[0];
        assert!(working.commit.is_working());
        assert_eq!(working.commit.parents, vec![oid(2)]);

        // It shares lane and colour with HEAD: they are the same branch.
        assert_eq!(working.lane, graph.rows[1].lane);
        assert_eq!(working.color, graph.rows[1].color);

        // And a single straight line joins them.
        assert_eq!(working.links.len(), 1);
        assert_eq!((working.links[0].from, working.links[0].to), (0, 0));
    }

    #[test]
    fn the_line_runs_down_to_head_even_when_it_is_not_the_newest() {
        // We are on the old branch (commit 2) while another carries on.
        let graph = layout_with_working(
            vec![commit(4, &[3]), commit(3, &[1]), commit(2, &[1]), commit(1, &[])],
            oid(2),
        );

        let working_lane = graph.rows[0].lane;
        let head = graph
            .rows
            .iter()
            .position(|row| row.commit.id == oid(2))
            .expect("HEAD sigue en el grafo");

        // The changes lane stays taken all the way down to HEAD.
        for row in &graph.rows[..head] {
            assert!(
                row.links.iter().any(|link| link.to == working_lane),
                "the line has to carry on down its own lane"
            );
        }
        assert_eq!(graph.rows[head].lane, working_lane, "y terminar en HEAD");
    }

    #[test]
    fn with_no_head_in_the_history_the_row_is_not_added() {
        // The limit left out the commit HEAD is on.
        let graph = layout_with_working(vec![commit(3, &[2])], oid(1));

        assert_eq!(graph.rows.len(), 1);
        assert!(!graph.rows[0].commit.is_working());
    }
}
