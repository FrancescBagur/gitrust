# gitrust

A desktop Git client in Rust. The aim is to cover the day-to-day flow without
depending on a commercial licence, starting with what is hardest to find free
and well made: **the commit graph with every branch woven through it**.

## State

- [x] Open a repository and walk every reference
- [x] Graph layout in lanes, with merges and a color per branch
- [x] Virtualised painting (only the visible rows)
- [x] A change list split between index and working tree
- [x] Per-file diff, stage and unstage, and commit with subject and description
- [x] Stage and unstage a single hunk
- [x] Discard changes: by file, by hunk or all of them, with a warning
- [x] Three panels: branches, graph and commit detail
- [x] A theme of its own: one single saturated family, the lanes'
- [x] The uncommitted changes, as one more row above the graph
- [x] An action bar: pull, push, new branch and stash
- [x] Force push with lease, and sending tags
- [x] Pull four ways: fetch, merge, fast-forward only and rebase
- [x] A column of branches and tags to the left of the graph
- [x] Search the history by message, author or hash
- [x] A board of columns, worked out from the graph and storing nothing
- [x] Dragging a card runs the operation that would have moved it
- [x] Everything in English, interface and code alike
- [x] Switch branch, including to one that only exists on the remote
- [x] Delete branches, here or on the remote, saying what is lost
- [x] Merge a branch into the current one, stopping dead if it clashes
- [x] Amend the last commit, with a warning if it already went to the remote
- [x] Tags: make them lightweight or annotated, and delete them
- [ ] Conflict editor, interactive rebase, worktrees

## Architecture

```
crates/
  gitrust-core/     Git logic, with no UI dependencies
    repo.rs         repository, commits, index and diffs (libgit2)
    branch.rs       making branches, switching between them and deleting them
    merge.rs        joining two branches or rebasing one, or refusing untouched
    tag.rs          lightweight and annotated tags
    remote.rs       fetching, pushing, and the distance to the remote branch
    stash.rs        the pile of changes set aside
    graph.rs        the lane algorithm, and the uncommitted-changes row
    board.rs        which column each branch falls in
    status.rs       the working tree's state
    diff.rs         a diff cut into hunks
    detail.rs       a commit's detail, and the references
    message.rs      putting the commit message together
    model.rs        the shared types, and which reference rules a commit
  gitrust-ui/       the egui application
    main.rs         window, state and carrying actions out
    theme.rs        color, typography and spacing
    action_bar.rs   pull, push, branch and stash
    refs_view.rs    the sidebar of branches and tags
    graph_view.rs   painting the graph and the references column
    board_view.rs   the branches laid out in columns
    detail_view.rs  the commit detail panel
    changes_view.rs the change lists, the diff and the commit box
    diff_view.rs    the shared painting of diff lines
```

The views do not touch the repository: they give back an `Action` and `main.rs`
carries it out. That keeps the painting free of side effects.

The core knows nothing of the UI. If egui ever stops fitting, `gitrust-ui` is
replaced and the Git logic is left intact.

What goes out on the network — fetching and pushing — leaves for a thread of
its own, which opens its own repository and sends back a sentence over a
channel. The painting thread waits for nobody.

**Git backend:** `git2` (libgit2) for the bulk of the operations. For
credential helpers, LFS and hooks the plan is to delegate to the real `git`
binary, as the commercial clients do, because libgit2 does not cover them well.

## Building

System dependencies on Debian/Ubuntu/Pop!_OS:

```bash
sudo apt install -y rustc cargo cmake libssl-dev libxkbcommon-dev \
  libwayland-dev libx11-dev libxcursor-dev libxi-dev libxrandr-dev \
  libgl1-mesa-dev
```

Then:

```bash
cargo run --release -- /path/to/a/repository
```

With no argument it opens the repository in the current directory.

## Tests

```bash
cargo test -p gitrust-core
```

The graph algorithm is the piece most at risk of regression, and the operations
that write — index, stash, branches and remote — are the ones that touch your
files. Those are the ones that carry cover: ninety-nine tests, seventy-six of
them against real repositories made in `/tmp` and deleted when they finish.

The remote ones talk to a bare repository in another temporary directory. Over
a disk path they walk the same road as a real remote — the refspecs, the fast
forward, the branch that starts tracking another — without depending on there
being a network or credentials.

## Licence

GPL-3.0-only. The full text is in [LICENSE](LICENSE).

libgit2 is GPLv2 with a linking exception and the Rust crates are
`MIT OR Apache-2.0`, so no dependency forced this choice: a tool made to stop
paying for a licence should not be something anyone else can close.
