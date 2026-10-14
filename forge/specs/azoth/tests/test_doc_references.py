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


class DocReferences(unittest.TestCase):
    def test_stages_and_jobs_cited_exist(self):
        known = stages()
        self.assertTrue(known)
        missing = []
        for doc in DOCS:
            text = doc.read_text()
            where = doc.relative_to(ROOT)
            for stage in re.findall(r"nvidia\.sh `?([a-z][\w-]*)", text):
                if stage not in known:
                    missing.append(f"{where}: nvidia.sh {stage}")
            # "job `sign` of nvidia-kmod" / "job `sign` di `nvidia-kmod.yml`"
            for job, wf in re.findall(r"job `([\w-]+)` (?:di|of) `?([\w-]+)(?:\.yml)?", text):
                if (WORKFLOWS / f"{wf}.yml").exists() and job not in jobs(wf):
                    missing.append(f"{where}: job {job} of {wf}")
            # "the workflow `x.yml`: `job` (what it does), `job` (...)", to the paragraph end
            for wf, para in re.findall(r"[Ii]l workflow `([\w-]+)\.yml`: ([^\n]*(?:\n[^\n]+)*)", text):
                for job in re.findall(r"`([\w-]+)` \(", para):
                    if job not in jobs(wf):
                        missing.append(f"{where}: job {job} of {wf}.yml")
        self.assertEqual(missing, [])


if __name__ == "__main__":
    unittest.main()
