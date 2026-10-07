# docs/superpowers

Implementation plans and dated reviews.

- Only plans with open tasks live here. Dated reviews are removed on the same rule once the state they describe has been superseded.
- A plan whose every task is executed is removed when its pull request merges. Its content stays in git history.
- To find an old plan: `git log --diff-filter=D --name-only -- docs/superpowers/plans`, then `git show <commit>^:<path>`.
- Specifications are not plans: they live in `docs/architecture/`.
