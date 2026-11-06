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
- **Writing outside the repository and `~/.local`**, which are the only two
  places anything is ever written.

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
