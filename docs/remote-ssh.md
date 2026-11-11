# Remote checkouts over SSH

Gitrust can open a Git checkout that already exists on another machine. Its
files stay on that machine: Git commands run there through SSH and files are
read or written through SFTP. This is different from Git's normal SSH remote,
which only fetches and pushes objects.

## What the server needs

- An SSH server with SFTP enabled.
- Git installed and available on `PATH` for the account used to connect.
- An existing Git working tree at an absolute path.

The server does not need gitrust installed. Its own Git remotes and Git
credentials remain responsible for fetch, pull and push from that checkout.

## Who the commits are by

With no profile, whoever the server's Git configuration says, as before. With
a profile that has a name and an address, that name: it goes along on the
command line of every Git command run there (`git -c user.name=... -c
user.email=...`), so it covers a merge, a rebase, a cherry-pick, a stash and an
annotated tag as well as a commit. Nothing is written into a configuration
file on the server, and the next person to log in finds it as they left it.
The name and address are visible in that server's process list for as long as
the command runs, which is true of any Git command given `-c`.

The profile's token and private key are never sent. The key proves who you
are to the SSH server and stays on this machine; pushing from that checkout
uses whatever the server itself is set up with.

## Connecting

Choose **Open remote checkout** in the repository picker, then select an SSH
host or enter its user, host and port. On macOS and Linux, gitrust offers
aliases from `~/.ssh/config`; on Windows it reads
`%USERPROFILE%\\.ssh\\config`.

SSH-agent authentication is used when available. A password can be entered for
servers that permit it, but it is cleared immediately after connecting and is
never stored in settings, known repositories, descriptors, logs or errors.

When a profile is in use and has a key, that key is tried first and the SSH
agent after it; a password that was typed still goes before both. The line
under the Connect button says which profile is about to connect and with what.
A saved checkout that is already filed under a profile connects as that
profile, whichever one is in use, and so does a tab that reconnects.

The first connection to a host requires checking its host key. A new or changed
key is rejected unless it is explicitly confirmed for that connection attempt.
After authentication, choose the checkout from the remote folder browser.
Only folders are shown and gitrust verifies that the selected folder is a Git
working tree before opening it.

## Working with a remote checkout

Remote tabs support the same repository operations as local tabs, including
working-tree edits, staging, commits, branches, tags, stash, fetch, pull,
push, merge, rebase and conflict resolution. The tab reconnects from its saved
non-secret descriptor; opening it again requires authentication because no
password is retained.

SSH and SFTP operations currently run synchronously on the UI thread and have
a 15-second timeout. A slow or unreachable server can therefore make the
window temporarily unresponsive until that operation returns or times out.

## Limits and safety

- A remote checkout is never cloned, mounted or silently mirrored locally.
- Remote paths must stay inside the checkout; absolute paths and `..` segments
  are refused for direct file access.
- If a connection drops, gitrust reports the command failure rather than
  claiming that an operation succeeded.
- A paused merge, rebase, cherry-pick or revert is rediscovered when the tab
  reconnects. Every unmerged porcelain state is shown in the conflict screen.
