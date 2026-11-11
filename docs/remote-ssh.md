# Remote checkouts over SSH

## Goal

Open a Git checkout that already exists on another machine and work on that
machine without cloning its files into the computer running gitrust. A remote
entry is `user@host:/absolute/path/to/checkout`. The connection lasts while
its tab is open; closing the tab closes its SSH and SFTP channels.

This is not Git's ordinary SSH transport. That transport only fetches and
pushes objects. The feature needs Git commands to run in the remote checkout
and SFTP to read or write its working-tree files.

## Boundaries

- The server needs SSH, SFTP and Git. It does not need gitrust installed.
- Passwords live only in memory for the current connection. They are never
  written to settings, the known-repository list or the SSH configuration.
- An SSH agent remains the default. A password is an alternative for hosts
  that allow it.
- `~/.ssh/config` is read on macOS/Linux and `%USERPROFILE%\\.ssh\\config` on
  Windows to offer aliases, users and ports already configured by the user.
- A remote checkout is never silently mirrored, mounted or copied locally.

## Architecture before UI work

The current `Repo` owns `git2::Repository`, so it can only work with a local
path. Remote support must introduce a common repository backend rather than
teach individual views to special-case SSH.

```
Open tab
  ├── LocalRepo   → libgit2 and the local filesystem
  └── RemoteRepo  → persistent SSH session, remote Git commands, SFTP
                         │
                         └── shared plain models: commits, refs, status,
                             diffs, files and operation results
```

Views continue returning `Action`; the selected backend carries it out. No
view gets a password, an SSH session or a shell command.

Remote commands are constructed as argument arrays and quoted once at the
transport boundary. The transport must collect stdout, stderr and exit status
separately, so a Git error is shown as an error rather than read as data.

## Deliverable 1 — Connection and remote folder choice

**User can:** choose a saved SSH host or enter host, user and port; authenticate
with the SSH agent or a password; browse remote folders over SFTP; select an
absolute folder; and receive a clear error if it is not a Git worktree.

**Core work**

- Finish `ssh::Session` as a persistent native SSH/SFTP session.
- Parse usable aliases from SSH config, including user, host and port.
- Add host-key verification. Never accept an unknown or changed host key
  without an explicit confirmation in the UI.
- List only directories through SFTP and keep navigation inside absolute
  remote paths.
- Verify the selected folder with `git -C <path> rev-parse
  --is-inside-work-tree` on the server.

**UI work**

- Add “Open remote checkout” beside “Open a folder”, not under Clone.
- Show configured aliases first, with a manual host form below them.
- Keep the password field masked and clear it immediately after connecting.
- Show an SFTP directory picker after authentication.

**Tests**

- Parser tests for macOS/Linux/Windows configuration locations and aliases.
- A fake SSH transport for authentication, host-key and directory-picker
  failures; no real credentials in tests.
- Manual check against a disposable server checkout.

**Not included:** adding the remote checkout to the main graph view. A
successful connection closes cleanly and exposes only the verified `RemoteRepo`
to the next deliverable.

## Deliverable 2 — Read-only remote tab

**User can:** open a remote checkout in a normal tab and inspect its graph,
branches, tags, commit detail, working-tree state and diffs.

**Core work**

- Define the backend boundary and implement the read models for both local
  libgit2 and remote Git output.
- Parse stable machine-readable Git output (`--format`, `-z`, `--porcelain`)
  rather than prose intended for terminals.
- Read files and diff content through SFTP or `git show` on the server.
- Make refresh reconnect only when the persistent session has failed.

**Tests**

- Fixtures for machine-readable Git output, including spaces, renames, binary
  files and detached HEAD.
- Snapshot/manual comparison of a local checkout and the same checkout opened
  remotely.

**Not included:** any remote mutation. Buttons that can write stay disabled
with an explanation until the next deliverable.

## Deliverable 3 — Working tree and commits

**User can:** stage or unstage files and hunks, discard changes, read/edit a
remote file, and create or amend commits in the remote checkout.

**Core work**

- Route status, diff, index and commit actions through `RemoteRepo`.
- Use SFTP only for direct file reads/writes; use remote Git for index and
  history changes.
- Keep the existing confirmation and conflict protections before every write.
- Report the remote command's exact failure without exposing a password.

**Tests**

- An isolated SSH test server checkout for stage, unstage, commit, amend and
  discard operations.
- Failure tests for permissions, an interrupted connection and conflicting
  worktree changes.

## Deliverable 4 — Branches, remotes and conflicts

**User can:** switch, create, rename and delete branches; fetch, pull and
push from the remote checkout; stash; merge/rebase/cherry-pick; and resolve
conflicts there.

**Core work**

- Implement every existing mutating `Action` on `RemoteRepo`.
- Preserve operation state on the server, then rediscover it after reconnect.
- Use the checkout's own Git credentials for its upstream remote; those are
  separate from the SSH credentials used to reach the server.

**Tests**

- Reuse the existing operation suite against an SSH-backed checkout.
- Add reconnect tests while a merge or rebase is paused.

## Deliverable 5 — Lifecycle and release readiness

**User can:** reopen saved remote connections without a password being saved,
see when a connection has dropped, reconnect deliberately, and close a tab
without leaving a session behind.

**Core work**

- Store only the non-secret connection descriptor and chosen remote path.
- Add connection state, timeouts, reconnect and explicit disconnect handling.
- Validate macOS, Linux and Windows SSH-config discovery and SSH backends.

**Acceptance criteria**

- A remote tab performs the same supported actions as a local tab.
- No source file from the remote checkout is created locally.
- No password appears in configuration, logs, errors or known repositories.
- Loss of a connection never makes gitrust claim an operation succeeded.

## Review strategy

Each deliverable is one PR and one user-visible capability. The first two are
read-only and safe to review independently; writes begin only after the
backend boundary is proven with the same repository models used by local tabs.
