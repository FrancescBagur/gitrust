# Contributing

The short version: open an issue before writing anything large, sign your
commits off with `-s`, and write the message as a sentence rather than a
label. Everything below is the long version of those three.

## What help is most wanted

In the order it would make the most difference:

1. **Running it on a Mac or on Windows.** It is *built* on both by CI and has
   never been run on either. The first person to open it there and say what
   is broken is doing the single most valuable thing available.
2. **The three-way conflict editor**, which is the one thing the conflict
   screen still cannot do: picking line by line rather than side by side.
3. **Submodules, bisect, LFS and signing** -- four holes, each of them its
   own piece of work, none of them started.
4. **Telling us where it lies.** A panel that shows something that is not
   true of your repository is a worse bug than one that crashes, because
   nobody finds out. Those get fixed first.

Issues marked `good first issue` are the ones that are genuinely small and
have no hidden dependency on the rest.

## Building and testing

```bash
cargo build
cargo test
```

The system libraries you need are in the [README](README.md#building).
Two hundred and ninety-one tests, most of them against real repositories
made in `/tmp`, so they are slow and they are honest. CI runs the same ones
on every pull request and on Linux, macOS and Windows.

A change to the graph layout or to anything that writes -- index, stash,
branches, remote -- comes with a test. Those are the parts that touch
somebody's files.

## Signing off: the DCO

Every commit carries a `Signed-off-by:` line, which `git commit -s` adds for
you:

```bash
git commit -s -m "fix: the thing that was wrong is no longer wrong"
```

That line means what the
[Developer Certificate of Origin](https://developercertificate.org/) says it
means: that you wrote this, or have the right to submit it, and that you are
content for it to go out under the project's licence. It is one line and it
is the whole of the paperwork.

**There is no CLA**, and there is not going to be one. Nobody signs away the
right to their own work here, and that includes signing it to me. The
consequence is deliberate and worth saying out loud: once your code is in,
this project cannot be relicensed or sold as a closed thing, because a part
of it would no longer be mine to sell. That is the point.

## The licence, and what it means for a contribution

AGPL-3.0-only. What you send goes out under it, same as everything else.

In practice that protects you more than it binds you: anybody who builds a
business on top of this -- including a hosted one, which is what the Affero
part is for -- hands the source back out, yours along with the rest.

## Commit messages

Look at `git log` before writing one. The convention here is a
[conventional-commit](https://www.conventionalcommits.org/) prefix and then
**a sentence in prose about what is now true**, not a label describing the
patch:

```
feat: a branch says whether it is here, on a remote, or on both
fix: the clone test brings its own name instead of borrowing the machine's
fix: both sides of a conflict stay inside the window
```

Not `feat: add remote status to BranchItem` and not `fix: bug in clone`. The
log is meant to be readable a year later by somebody who has never seen the
code, and it is.

## Comments

The same rule in a different place: a comment says **why**, and what the
code gets wrong, and what it cannot know. A comment that restates the line
below it is noise and will be asked about in review. There are several in
this repository that admit a limitation plainly; those are the model.

## Written with an assistant

This project is written with Claude Code and says so on its front page. So:
a contribution written with an assistant is welcome here and needs no
apology. Two conditions, and they are the same two that apply to one written
by hand:

- **You understand what you are sending.** If a reviewer asks why a line is
  there, "the model put it there" is not an answer. You are signing the DCO
  on it.
- **You ran it.** Code that compiles is not code that works, and the tests
  are there to be run before the pull request, not after it.

## Pull requests

- One idea per pull request. Two unrelated fixes are two pull requests.
- Say what you changed and, more usefully, what you *decided* -- the
  alternative you did not take is often the interesting part of a review.
- CI has to be green. If it is red on something you believe you did not
  break, say so in the thread rather than forcing it through; it has been
  wrong before.
- Expect an answer within the day. If a week goes by, ping the thread: it
  was dropped, not ignored.

## Reporting a bug

What makes a Git bug fixable: the version (`gitrust --version` or the tag you
downloaded), the system, and **the shape of the repository** -- how many
commits, how many branches, whether there was a merge in flight, whether the
remote was reachable. A screenshot of a wrong graph is worth more than a
paragraph describing it.

If it ate work, say that in the first line. Those go to the front of the
queue.
