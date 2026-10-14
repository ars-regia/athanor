"""Every `nvidia.sh <stage>` and every workflow job that the documents of the kernel area
cite must exist: a stage in nvidia.sh's dispatch, a job in the named workflow
(python3 -B -m unittest discover -s forge/specs/azoth/tests -v)."""

import pathlib
import re
import unittest

AZOTH = pathlib.Path(__file__).resolve().parents[1]
ROOT = AZOTH.parents[2]
WORKFLOWS = ROOT / ".github" / "workflows"
DOCS = [*AZOTH.rglob("*.md"), ROOT / "docs" / "architecture" / "doc_kernel_build.md"]


def jobs(workflow):
    """Names of the jobs of a workflow file (`name` or `name.yml`)."""
    text = (WORKFLOWS / (workflow.removesuffix(".yml") + ".yml")).read_text()
    return set(re.findall(r"^  ([\w-]+):\s*$", text.split("\njobs:\n", 1)[1], re.M))


def stages():
    """The stages in the final `case $STAGE in ... esac` dispatch of nvidia.sh."""
    dispatch = re.findall(r"^case \$STAGE in (.*) esac$", (AZOTH / "nvidia.sh").read_text(), re.M)[-1]
    return set(re.findall(r"([\w-]+)\) \w+ ;;", dispatch))


ORCHESTRATOR = "athanor-forge-orchestrator"
LIST = r"((?:`[\w-]+`(?:, | e )?)+)"
# job `x` of/in/nel [workflow] `w.yml` | dell'Orchestrator | of the Orchestrator
JOB_OF = re.compile(
    r"job\s+`([\w-]+)`\s+(?:(?:di|of|del|nel|in)\s+(?:workflow\s+)?(?:riusabile\s+)?(?:`([\w-]+)(?:\.yml)?`|([\w-]+)\.yml)"
    r"|(dell'Orchestrator|of the Orchestrator))"
)
# the workflow `w.yml` ha due job: `a` (...), `b` (...) | ha i job `a`, `b` e `c`
JOBS_OF = re.compile(r"[Ww]orkflow\s+`([\w-]+)\.yml`\s+ha\s+(?:due|i)\s+job:?\s*" + LIST + r"([^\n]*(?:\n[^\n]+)*)")


class DocReferences(unittest.TestCase):
    def problems(self, text, where):
        known = stages()
        out = []

        def check(wf, job):
            path = WORKFLOWS / f"{wf}.yml"
            if not path.exists():
                out.append(f"{where}: workflow {wf}.yml does not exist")
            elif job not in jobs(wf):
                out.append(f"{where}: job {job} is not in {wf}.yml")

        for stage in re.findall(r"(?:`|bash |\./)\S*nvidia\.sh ([a-z][\w-]*)", text):
            if stage not in known:
                out.append(f"{where}: nvidia.sh {stage}")
        for job, wf, bare, orch in JOB_OF.findall(text):
            check(ORCHESTRATOR if orch else wf or bare, job)
        for wf, first, rest in JOBS_OF.findall(text):
            for job in [*re.findall(r"`([\w-]+)`", first), *re.findall(r"`([a-z][a-z0-9-]*)` \(", rest)]:
                check(wf, job)
        return out

    def test_stages_and_jobs_cited_exist(self):
        self.assertTrue(stages())
        missing = []
        for doc in DOCS:
            missing += self.problems(doc.read_text(), doc.relative_to(ROOT))
        self.assertEqual(missing, [])

    def test_the_check_catches_a_wrong_citation(self):
        for text in [
            "job `sign` dell'Orchestrator",
            "job `sign` of the Orchestrator",
            "il job `sign` di `nvidia-kmod.yml`",
            "il job `sign` nel workflow `nvidia-kmod.yml`",
            "il job `x` di `nowhere.yml`",
            "Il workflow `nvidia-kmod.yml` ha due job: `boot` (a), `sign` (b)",
            "il workflow `call-nvidia-kmod-prepare.yml` ha i job `prepare`, `sign` e `build`",
            "`nvidia.sh sign --driver open`",
        ]:
            self.assertTrue(self.problems(text, "x"), text)
        self.assertEqual(self.problems("nvidia.sh costruisce; job `nvidia-kmod-sign` dell'Orchestrator", "x"), [])


if __name__ == "__main__":
    unittest.main()
