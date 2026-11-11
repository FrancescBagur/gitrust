# Security

## Reporting

Mail <francesc.bagur@coffeesoftware.io>, or open a
[private advisory](https://github.com/FrancescBagur/gitrust/security/advisories/new)
on GitHub. Not a public issue, not at first.

You get an acknowledgement within three days and an honest estimate of when
it will be fixed. If the answer is "not soon", you get told that too rather
than being left waiting.

## What counts here

gitrust runs on your machine, on your repositories, with your credentials.
So the ones that matter are:

- **Credentials leaking** -- anything that writes a token or a passphrase
  somewhere it can be read, including a log or a panic message.
- **A repository that attacks the program that opens it.** Cloning something
  hostile and opening it here should be as safe as `git log` is. Crafted ref
  names, paths that climb out of the working tree, enormous objects.
- **Running something the user did not ask for.** gitrust shells out to `gh`
  for pull requests; anything that turns repository contents into a command
  is a hole.
- **Writing outside the repository, `~/.local` and `~/.config/gitrust`**,
  which are the only places anything is ever written.

## Where the tokens are

A profile can hold a personal access token for GitHub or GitLab, and that
token sits in `~/.config/gitrust/profiles` **in plain text**. The file is
written `rw-------` before it is moved into place, so no other account on the
machine can read it, and that is the whole of the protection: there is no
keyring here, and encrypting it would only move the question to where the key
to it is kept. It is the same bargain `~/.netrc` and `gh`'s own `hosts.yml`
make, and it is written down here rather than left to be discovered. On
Windows there is no mode to set and the file is protected by the permissions
of the folder it is in and by nothing this program does.

Leaving the token empty is a supported answer and not a half-configured one:
then `gh` and `glab` are run as they are logged in, and an HTTPS remote gets
its password from the credential helper, exactly as before profiles existed.

An SSH key is never copied into that file -- only the path to it. A key with
a passphrase cannot be opened from here at all, because there is nowhere to
ask for the passphrase and nowhere safe to keep it; that key belongs in the
SSH agent, which is what gitrust falls back to.

A profile also lists the repositories filed under it: folders on this disk,
and checkouts on other machines as `ssh://user@host:port/path`. Those lines
are addresses and nothing else -- no password is ever among them -- but they
do say which servers you work on and as whom, and they are in the same
`rw-------` file as the tokens for that reason too.

When a profile is used on a checkout on another machine, two things leave
this one and two do not. Its name and address go along on the command line of
each Git command run there. Its private key is used to sign the SSH
authentication and never leaves; its tokens are not used at all.

A key made from the profiles screen is made by `ssh-keygen`, as ed25519, and
**with no passphrase** -- said plainly because it is the one thing about it
worth knowing. It is not a shortcut: a key with a passphrase has to be
unlocked by something, and the only thing here that could is the agent, which
this program does not drive. So the key is as safe as the file it is in, which
is `rw-------` in `~/.ssh`, and no safer. A key that should have a passphrase
should be made with `ssh-keygen` by hand and pointed at with Browse. Nothing
here ever writes over a key that already exists: being asked twice gets a
refusal, not a new pair, because the public half of the old one is on a server
somewhere and overwriting it locks you out of it.

The preferences screen has a row that forgets every token at once. It
empties the token of every account in every profile and writes the file
again; the accounts, the keys and whatever Git's credential helper holds are
not touched. It asks before it does it and it cannot be undone from here: a
token is written down nowhere else.

## What the window does by itself

Three things happen without being asked for, and each can be turned off on
the preferences screen.

**It fetches**, every minute by default, from every remote of the repository
in front. That is a connection to those servers at that interval, with the
same credentials a fetch you asked for would use -- the profile's key and
token, the agent, the credential helper. gitrust itself never asks for
anything: a key that needs a passphrase the agent does not have fails, and
is tried again at the next interval. But what it asks can ask you. An agent
holding a key that wants a touch or a confirmation wants one each time, and
a credential helper that opens a window when it has nothing stored opens it
each time; with either of those, set the interval to nought or to something
you can live with. A checkout on another machine is never fetched this
way.

**It moves submodules** to the commit the repository records for them after
a pull, a switch or a clone made here, cloning one that was never brought
down. That is a fetch against the submodule's own address, which a
`.gitmodules` file in the repository names -- so it is never done for a
repository having merely been opened, only after something you asked for
has moved it. Nothing is checked out over work that is in the way; that
submodule is left and named.

**It tries the merge** into the target branch in memory. That reads the
repository and writes nothing: no file, no index, no reference.

The tabs that were open are written into `~/.config/gitrust/settings` as
paths, and remote checkouts as `ssh://user@host:port/path`. No password is
among them, and a remote tab that comes back does not connect until asked.

## What does not

- Losing uncommitted work through a destructive operation the program warned
  about and you confirmed. That is a bug if the warning is wrong, and the
  warning being wrong is worth reporting -- as a bug.
- Anything in libgit2 itself: that goes
  [to libgit2](https://github.com/libgit2/libgit2/security), and tell us too
  so the pinned version can move.

## Versions

Only the newest release. There is one of them, and there is no branch anyone
is being asked to stay on.

## About the downloads

Nothing published here is signed -- there is no paid certificate from Apple
or Microsoft behind it. Every release carries `SHA256SUMS`, which tells you
the file is the one that was built and tells you nothing about who built it.
That is said on every release page rather than left to be discovered.
