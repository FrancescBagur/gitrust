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
- [x] In review means a pull request is open, asked of the forge through `gh`
- [x] A branch cut from a branch is a subtask: go into a card and there it is
- [x] Dragging a card runs the operation that would have moved it
- [x] A backlog everyone with the repository shares, kept in a reference
- [x] A branch can say what it is for, in words, and the card shows them
- [x] Everything in English, interface and code alike
- [x] Switch branch, including to one that only exists on the remote
- [x] Delete branches, here or on the remote, saying what is lost
- [x] Merge a branch into the current one, or replay it on top with a rebase
- [x] Amend the last commit, with a warning if it already went to the remote
- [x] Tags: make them lightweight or annotated, and delete them
- [x] Open a branch in a folder of its own, and jump between the folders
- [x] Right-click a branch for everything that can be done to it or from it
- [x] Take a commit along, take one back, and move a branch to one
- [x] Decide a clash file by file: take a side, or write it yourself
- [x] Carry a rebase on commit by commit, or throw the whole thing away
- [ ] Interactive rebase: squash, reorder, edit, drop
- [ ] Three-way editor with line-by-line picking

## Architecture

```
crates/
  gitrust-core/     Git logic, with no UI dependencies
    repo.rs         repository, commits, index and diffs (libgit2)
    branch.rs       making branches, switching between them and deleting them
    merge.rs        joining two branches or rebasing one, and stopping where it clashes
    conflict.rs     getting out of a merge or rebase that clashed
    tag.rs          lightweight and annotated tags
    remote.rs       fetching, pushing, and the distance to the remote branch
    stash.rs        the pile of changes set aside
    graph.rs        the lane algorithm, and the uncommitted-changes row
    history.rs      taking a commit along, taking one back, moving a branch
    board.rs        which column each branch falls in, and what came from what
    backlog.rs      the work with no branch yet, and how it travels
    note.rs         what a branch is for, in the words somebody used
    worktree.rs     the same repository checked out in several places
    start.rs        cloning one down, or starting an empty one
    known.rs        the repositories this machine has opened
    forge.rs        the pull requests, which Git does not store
    status.rs       the working tree's state
    diff.rs         a diff cut into hunks
    file.rs         one file: at a commit, blamed, its history, a hunk undone
    detail.rs       a commit's detail, and the references
    message.rs      putting the commit message together
    model.rs        the shared types, and which reference rules a commit
  gitrust-ui/       the egui application
    main.rs         window, state and carrying actions out
    theme.rs        color, typography and spacing
    action_bar.rs   pull, push, branch and stash
    refs_view.rs    the sidebar of branches, tags and checkouts
    picker.rs       choosing a repository, cloning one, starting one
    chooser.rs      asking the desktop for its own folder dialog
    graph_view.rs   painting the graph and the references column
    board_view.rs   the branches laid out in columns
    detail_view.rs  the commit detail panel
    file_view.rs    one file across the window, in its four readings
    conflict_view.rs the two answers side by side, and the ways out
    changes_view.rs the change lists, the diff and the commit box
    diff_view.rs    the shared painting of diff lines
```

The views do not touch the repository: they give back an `Action` and `main.rs`
carries it out. That keeps the painting free of side effects.

The core knows nothing of the UI. If egui ever stops fitting, `gitrust-ui` is
replaced and the Git logic is left intact.

What takes time leaves for a thread of its own, which opens its own repository
and sends back one answer over a channel: fetching and pushing, because the
network takes as long as it takes, and laying the board out, because weighing
thirty branches against each other is well over a second. The board already on
screen stays up until the new one arrives. The painting thread waits for
nobody.

**Git backend:** `git2` (libgit2) for the bulk of the operations. For
credential helpers, LFS and hooks the plan is to delegate to the real `git`
binary, as the commercial clients do, because libgit2 does not cover them well.

**The backlog:** the one column that is stored, because work nobody has begun
leaves no trace in a repository. It is kept in `refs/gitrust/backlog`, a
reference outside the branch namespace — the same trick `git notes` plays with
`refs/notes/*` — so it is invisible to the graph, to the labels on the commits
and to the sidebar without any of them knowing it exists, and it reaches
everybody who has the repository by being pushed and fetched by name. One file
per item, so two people writing down two different things merge without
noticing. What keeps it from rotting is a rule rather than a mechanism: an item
lives there only until it becomes a branch, and from that moment it is read off
the graph like everything else.

**What a branch is for:** a branch name is what is left of a sentence after
it has been filed down to fit a reference, and until now it was all the
written intention a repository held — starting a backlog item turned the
sentence you wrote into a slug and lost it. A note gives it back: a line in
your own words, and a paragraph under it if there is more to say. It lives in
the backlog's own reference, under `notes/` beside its `items/`, because
starting an item has to take it out of the waiting room *and* write its note,
and across two references that is two commits with a half-done state between
them. One file per branch, named after the branch with its slashes turned
into `~`, which Git will not have in a reference name and so cannot be
confused for one. A note is not tidied away when its branch goes: a branch
can come back and a deletion can be a mistake, and that is worth more than
the clutter.

**Subtasks:** a branch cut from another branch is a piece of that branch's
work. Nothing records that either, so it is read off the graph: a branch is a
child of whichever other branch it shares more history with than it shares
with the trunk. The board inside a card is then the same board measured
against a different branch, and "done" for a subtask is having gone into its
parent. It is a reading and not a fact: once a branch keeps committing after
something was cut from it, the two are the same shape, and the names are what
decide — `report` under `report-export`, the same convention that already
groups the cards. Where it is wrong it is wrong out loud, with a test pinning
each case. A card carries the count of what came out of it, and that badge is
the way in.

**Worktrees:** one repository can be checked out in several folders at once,
each with its own branch, sharing one history. A card says which folder its
branch is open in and takes you there; right-clicking one that is open
nowhere offers it a folder of its own. A branch can only be open in one
place, so switching to one that is open elsewhere says where it is rather
than refusing, and the checkout you are looking at is not one you can close.
The folders go beside the repository, never inside it.

**The forge:** Git stores nothing about pull requests, so a branch that is
merely pushed and one somebody is reviewing look alike from inside the
repository. The board asks `gh` instead of guessing, on its own thread, the
first time it is opened. Without `gh`, without a login, or on a remote that is
not GitHub, nothing breaks: the board goes back to reading the graph alone and
the header says so, because a column that is guessing should admit it.

**The folder dialog:** the one dependency that was not part of the plan.
Opening a repository meant typing its path, and the obvious fix was to run
`gdbus` and ask `xdg-desktop-portal`, the way `gh` is run to ask GitHub. It
does not work, and the reason is not a bug anybody can fix: the portal's
reply is a directed signal addressed to whoever asked, `gdbus call` exits the
moment it has a handle, and `gdbus monitor` never sees it -- measured, with a
process that stayed alive getting the reply at the same moment the monitor
printed nothing. The caller has to stay on the bus, which means a D-Bus
client in-process. So `rfd`, which costs three crates rather than the tree
that number usually implies: the D-Bus machinery is already in this binary,
pulled in by eframe for Wayland. A folder dragged onto the window still
works everywhere, and still costs nothing.

**An assistant:** not built, and perhaps never. The thinking that was done
about putting Claude Code inside gitrust is written down in
[docs/assistant.md](docs/assistant.md) rather than lost: what gitrust knows
that a terminal does not, the fact that a task here holds no text at all
today, and why the part worth building first has no assistant in it.

## Building

System dependencies on Debian/Ubuntu/Pop!_OS:

```bash
sudo apt install -y rustc cargo cmake libssl-dev libxkbcommon-dev \
  libwayland-dev libx11-dev libxcursor-dev libxi-dev libxrandr-dev \
  libgl1-mesa-dev
```

Optional, for the board's pull requests:

```bash
sudo apt install -y gh && gh auth login
```

Then:

```bash
cargo run --release -- /path/to/a/repository
```

With no argument it opens the repository in the current directory.

## Tests

```bash
cargo test
```

The graph algorithm is the piece most at risk of regression, and the operations
that write — index, stash, branches and remote — are the ones that touch your
files. Those are the ones that carry cover: two hundred and fifty-one
tests, a hundred and eighty of them against real repositories made in
`/tmp` and
deleted when they finish.

The remote ones talk to a bare repository in another temporary directory. Over
a disk path they walk the same road as a real remote — the refspecs, the fast
forward, the branch that starts tracking another — without depending on there
being a network or credentials.

## Licence

GPL-3.0-only. The full text is in [LICENSE](LICENSE).

libgit2 is GPLv2 with a linking exception and the Rust crates are
`MIT OR Apache-2.0`, so no dependency forced this choice: a tool made to stop
paying for a licence should not be something anyone else can close.
