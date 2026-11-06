<!-- One idea per pull request. Two unrelated fixes are two pull requests. -->

## What this changes

<!-- What is now true that was not before, in a sentence. -->

## What was decided

<!-- The alternative you did not take, and why. This is usually the
     interesting half of a review and it is the one that gets skipped. -->

## Checked

- [ ] `cargo test` passes here, not just in CI
- [ ] It was run -- the window was opened and the thing was done by hand
- [ ] Anything that writes (index, stash, branches, remote) or touches the
      graph layout carries a test
- [ ] Commits are signed off (`git commit -s`) and read as sentences
