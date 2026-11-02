# An assistant inside gitrust

A design note, written on 2 November 2026. **None of this is built, and it
may never be.** It is written down because the thinking was done and thinking
that is not written down has to be done again.

Everything here is about Claude Code, because that is what this repository is
written with. Most of it would hold for any agent that runs as a command and
keeps a conversation.

## The question to answer before any of the others

Not "where does the chat go". That one has an answer only once this one does:

> What does gitrust know that a terminal does not?

If the answer is nothing, then putting an assistant inside gitrust is
decoration, and the terminal — which already exists and is already good —
wins on every count.

It is not nothing. gitrust knows four things a `claude` in a folder does not:

- **which branch is this task.** A terminal knows only its working directory.
- **what it was cut from**, and what was cut from it: the parent, the
  siblings, the subtasks.
- **which column it is in**, and whether a pull request is open on it.
- **which folder it is checked out in**, when it has one of its own.

Today you tell an assistant all of that by hand, every time. That is where
the value is. It is not in drawing a chat.

## The hole that is already there, with no assistant in sight

**A task in gitrust holds no text.** It has a branch name — `fix/footer` —
and that is the whole of the human intention recorded anywhere.

It is worse than that. `backlog_start` *throws the title away*: you write
"the footer rides over the content on a narrow phone", and what survives is
`fix-the-footer-rides-over-the-content-on-a-narrow-phone`, a slug. The one
sentence written in your own words is gone the moment the work starts.

To an agent, a branch name is not a brief. "Develop `fix/footer`" is not a
request anybody could act on.

So the first step is not an assistant at all. **A task should be able to hold
text**: the real title, and a paragraph saying what is to be done. Kept the
way the backlog is kept — a reference of its own, one file per branch, pushed
and fetched by name — because that mechanism already works and already merges
without anybody noticing.

This pays whether or not an assistant ever arrives: the card shows what you
wrote instead of a slug, and everyone with the repository can read what a
branch is *for*. Today nobody can.

## What was checked about the `claude` CLI

Against version 2.1.310, on 2 November 2026. Two findings change the design:

**`--session-id <uuid>` exists.** gitrust can choose the conversation's
identity itself, before there is a conversation. Generate a UUID when the task
is created, keep it, and `--resume <uuid>` later. Nothing has to be launched
to find out an id, nothing has to be scraped from output, nothing has to be
parsed.

**Conversations are stored per folder.** `~/.claude/projects/` is keyed on a
slug of the working directory, and a worktree gets its own:

```
-home-kinnikuman-Documentos-Proyectos-maat--maat-worktrees-CORE1
```

So a branch opened in a folder of its own already has a conversation space of
its own. And because `worktree_path_for` is deterministic, closing a folder
and opening it again lands on the same slug: the conversation survives.

Which means **task = branch = folder = conversation** is not a metaphor that
would have to be built. It is how the pieces already fit.

Also present and relevant: `--restricted` (removes Bash and the other tools
that run code), `--permission-mode` (`acceptEdits`, `auto`, `manual`,
`dontAsk`, `plan`), `--bg` (runs in the background, returns an id),
`--mcp-config`, and `--print --input-format stream-json --output-format
stream-json` for driving it as a child process.

One thing that was *not* confirmed: `--permission-prompt-tool` is referred to
in the help text for `--permission-prompts` but is not listed as an option of
its own in this version. Anything that leans on it should check first.

## Where the conversation could live

Three honest answers, with what each costs.

**A — outside.** The card gets "Continue with Claude", which opens a terminal
in that folder with `--resume <uuid>`. Costs an afternoon. Permissions are
Claude Code's, which already work. Coming back to a task a week later and
landing in the same conversation with one press is already half of what this
is for.

**B — inside, as a panel on the card.** Drive `claude` as a child process over
`stream-json` and draw the conversation. Costs weeks, and not because of the
chat: because of the markdown, the code blocks, the diffs, the scrollback and
the tool calls. Drawing tool output well *is* the hard part of Claude Code's
interface. For months it would be worse than the terminal.

**C — inside, with no chat at all.** `claude --bg` in the folder, and the card
shows a state. Three things gitrust can know without parsing anything: whether
there is a conversation, whether a process is alive, and how many files have
changed in that folder — `worktree_changes` already counts them. The card
would say "working · 7 files". Costs little.

A single global Claude console — the first thing anybody asks for — is the one
to refuse. A console with no address is the terminal again. If the premise is
that a conversation belongs to a task, then it lives **on the card**.

## Permissions

This is the part with no good answer yet, and `--dangerously-skip-permissions`
is not one. It is not a policy, it is giving up.

There is a way through that is not giving up. **`--restricted`** removes Bash
and everything else that runs code. A restricted agent reads and edits but
does not execute. In a worktree that is a blast radius of one folder and one
branch, and git is the undo. For a first version it is defensible: no
permission dialogs at all, because there is nothing to ask about.

Beyond that, `--permission-prompt-tool` would route the question to gitrust as
an MCP call, to be drawn over the card. See the caveat above: confirm it
exists before building on it.

## What an MCP server would be for

Not the obvious thing. The agent already has `git` and `gh`; it does not need
gitrust to read it a log.

What only gitrust knows:

- `board()` — the columns, which are a reading of the graph nothing else makes
- `subtasks_of(branch)` and `parent_of(branch)` — the derived parentage
- `note_of(branch)` — the brief, once briefs exist

And one thing the agent should be able to **do**, which is what closes the
loop:

- `add_subtask(title, description)` — cut a branch off this one, with its brief

The server only pays once there is an agent running. It comes after, not
before.

## The loop this is all for

1. You write "export the report to PDF" in the backlog.
2. You drag it into In progress: a branch, a folder of its own, and the note
   travels with it.
3. You go into the card. "Hand it to Claude."
4. Claude reads the note, looks at the repository, and **creates three
   subtasks with briefs of their own**. They appear as cards inside the card.
5. Each has its folder and its conversation. Three can run at once: three
   branches, three folders, nothing standing on anything.
6. Each one that finishes goes into its parent — which is already what "done"
   means for a subtask.
7. The parent finishes when all of its children have gone in.

That is a board that fills itself, and it is the thing a terminal cannot do,
because a terminal has no board.

## What this note concludes

**It is worth doing, and the value is not in the chat.** The chat is the one
item on the list that already exists and is better elsewhere. What exists
nowhere is a conversation with an *address*: one that belongs to a branch
rather than to a terminal tab that was closed on Tuesday.

If it is ever built, this order:

1. **The note.** No assistant, no network, no new dependency. The only step
   that pays even if the rest never happens.
2. **A conversation per task.** A UUID per branch, kept in a reference that is
   never pushed — session ids are local to a machine and mean nothing on
   anybody else's. "Continue with Claude" opens a terminal in that folder.
3. **The MCP server**, once there is an agent to use it.
4. **The panel inside the app**, last, if it still feels missing — and by then
   it is only a renderer, because the brief, the folder, the conversation and
   the permission channel all already exist.

Steps 1 and 2 are small, and neither of them needs any of this to be true.
