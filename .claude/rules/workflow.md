# Working in this repository

<!-- No paths: loaded in every session. Keep it to a handful of lines. -->

- Run `git remote set-head origin --auto` once, and base every worktree on `origin/iso-v0` and report its commit. Why: a local `origin/HEAD` can still name `main`, the pre-rename tree, and work built on it looks plausible and is wrong.
- Change `scripts/verify.py`, `forge/config/packages.json` and Markdown under `docs/` with an exact-match replacement script, never through a format-on-save hook, then check `git diff --numstat`. Why: these files are not formatter-clean, and a whole-file reformat buries the real change.
- Inside the Claude Code sandbox, run git writes and `gh` outside it, then check `git branch --show-current`. Why: `git switch -c` can move the index and worktree without HEAD, and `gh` answers 401 without keyring access.
- Never `git add` untracked `.bashrc`, `.gitconfig`, `.mcp.json`, `.idea`, `.vscode` or the agents directory under `.claude` seen inside the sandbox. Why: they are the sandbox's `/dev/null` mounts, not files of the checkout.
- On Athanor hosts put cargo targets, toolchains and large scratch under `/var/tmp`. Why: `/tmp` is tmpfs, and a multi-gigabyte target there has exhausted host memory.
