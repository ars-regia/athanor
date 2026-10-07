---
paths:
  - "system/**"
---

# system/

- After adding files under `system/`, run `git status --porcelain --ignored -- <dir>` and add a narrow negation to `system/.gitignore` for anything it hides. Why: it ignores `output`, `test_*/`, `*.log`, `logs_*`, `mnt_*/` and `ctr_id` at any depth, and a fixture once vanished from a commit while local tests passed.
