#!/usr/bin/env python3
"""The alert of a scheduled workflow run (PL51, docs/architecture/doc_pipeline.md): one issue
labelled `ci-alert` per workflow and job, opened when the job fails or is cancelled, commented
on when it fails again, closed when it next succeeds. A skipped job changes nothing.

It runs in an alert job, on a GitHub-hosted runner, after the jobs it reports whatever their
result (`.github/actions/alert`): a job whose runner was lost never runs a last step.

Usage: ci_alert.py
Environment: NEEDS, the workflow's `needs` context as JSON (toJSON(needs)); GITHUB_REPOSITORY,
GITHUB_WORKFLOW, GITHUB_SERVER_URL, GITHUB_RUN_ID; GH_TOKEN with issues: write.
A failing `gh` call is reported and the other jobs are still handled; the script then exits 1,
so an alert that could not be raised is not silent either.
"""

import json
import os
import subprocess
import sys

LABEL = "ci-alert"
FAILED = ("failure", "cancelled")


def title(workflow: str, job: str) -> str:
    return f"{workflow} / {job}: scheduled run failed"


def alert(needs: dict, workflow: str, run_url: str, gh) -> list[str]:
    """Opens, updates or closes one issue per job in NEEDS; GH runs one `gh` command. Returns the
    jobs whose issue could not be updated, each reported on stderr."""
    listed = json.loads(
        gh(
            "issue",
            "list",
            "--label",
            LABEL,
            "--state",
            "open",
            "--limit",
            "200",
            "--json",
            "number,title",
        )
    )
    open_issues = {issue["title"]: issue["number"] for issue in listed}
    label_ready = False
    unraised = []
    for job, state in sorted(needs.items()):
        result = state["result"]
        name = title(workflow, job)
        number = open_issues.get(name)
        try:
            if result in FAILED and number is not None:
                gh(
                    "issue",
                    "comment",
                    str(number),
                    "--body",
                    f"Failed again: {run_url} (`{result}`).",
                )
            elif result in FAILED:
                if not label_ready:
                    gh(
                        "label",
                        "create",
                        LABEL,
                        "--force",
                        "--color",
                        "B60205",
                        "--description",
                        "A scheduled workflow job failed (PL51)",
                    )
                    label_ready = True
                gh(
                    "issue",
                    "create",
                    "--title",
                    name,
                    "--label",
                    LABEL,
                    "--body",
                    f"The scheduled run {run_url} ended with `{result}` for job `{job}` of "
                    f"`{workflow}`.\n\nEvery failed scheduled run of the job comments here, and the "
                    "next successful one closes the issue (PL51, docs/architecture/doc_pipeline.md).",
                )
            elif result == "success" and number is not None:
                gh(
                    "issue",
                    "close",
                    str(number),
                    "--comment",
                    f"Succeeded again: {run_url}.",
                )
        except subprocess.CalledProcessError as error:
            # One job's failed call must not cost the next jobs their alert.
            print(f"ci_alert: {name}: {error}", file=sys.stderr)
            unraised.append(job)
    return unraised


def main() -> int:
    repository = os.environ["GITHUB_REPOSITORY"]
    run_url = (
        f"{os.environ['GITHUB_SERVER_URL']}/{repository}/actions/runs/"
        f"{os.environ['GITHUB_RUN_ID']}"
    )

    def gh(*args: str) -> str:
        return subprocess.run(
            ["gh", *args, "-R", repository], check=True, stdout=subprocess.PIPE, text=True
        ).stdout

    unraised = alert(json.loads(os.environ["NEEDS"]), os.environ["GITHUB_WORKFLOW"], run_url, gh)
    return 1 if unraised else 0


if __name__ == "__main__":
    sys.exit(main())
