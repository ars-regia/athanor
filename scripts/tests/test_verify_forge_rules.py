"""Unit tests of the forge-rules check of scripts/verify.py: golden rules 2 and 3 of
docs/architecture/doc_forge_development_guide.md over forge/specs/*/*.spec
(python3 -B -m unittest discover -s scripts/tests -v)."""

import importlib.util
import pathlib
import tempfile
import unittest

SCRIPT = pathlib.Path(__file__).resolve().parents[1] / "verify.py"
spec = importlib.util.spec_from_file_location("verify", SCRIPT)
verify = importlib.util.module_from_spec(spec)
spec.loader.exec_module(verify)

HEADER = (
    "Name: x\nVersion: 1\nRelease: 1\nSummary: x\nLicense: MIT\n\n%description\nx\n\n"
)


class ForgeRulesTest(unittest.TestCase):
    def problems(self, body):
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            path = root / "forge/specs/x/x.spec"
            path.parent.mkdir(parents=True)
            path.write_text(HEADER + body)
            return verify.forge_rule_problems(root)

    def assertRule(self, body, rule):
        found = self.problems(body)
        self.assertEqual(len(found), 1, found)
        self.assertIn(f"rule {rule}", found[0])
        self.assertIn("forge/specs/x/x.spec:", found[0])

    # Rule 2: no mutation of /usr or /etc from a scriptlet.
    def test_a_copy_into_etc_from_post_is_reported(self):
        self.assertRule("%post\ncp /usr/share/x/a.conf /etc/a.conf\n", 2)

    def test_a_chmod_through_a_path_macro_is_reported(self):
        self.assertRule("%posttrans\nchmod 0600 %{_sysconfdir}/a.conf\n", 2)

    def test_an_append_to_etc_is_reported(self):
        self.assertRule("%post\ngetent group video >> /etc/group\n", 2)

    def test_a_directory_made_under_etc_is_reported(self):
        self.assertRule("%pre\nmkdir -p /etc/usbguard\n", 2)

    def test_install_and_harmless_scriptlets_pass(self):
        found = self.problems(
            "%install\ninstall -Dm0644 a.conf %{buildroot}%{_sysconfdir}/a.conf\n"
            "cp -a b %{buildroot}/usr/share/x/\n\n"
            "%post\n%systemd_post a.service\n# cp a /etc/b would break the rule\n"
            "grep -q x /etc/group > /dev/null 2>&1\n\n"
            "%files\n%{_sysconfdir}/a.conf\n"
        )
        self.assertEqual(found, [])

    # Rule 3: hardening and signature checks stay on.
    def test_fortify_undefined_is_reported(self):
        self.assertRule("%build\n%undefine _fortify_source\nmake\n", 3)

    def test_repo_gpgcheck_off_is_reported(self):
        self.assertRule("%install\necho 'repo_gpgcheck=0' > %{buildroot}/x.repo\n", 3)

    def test_stack_protector_off_is_reported(self):
        self.assertRule('%build\nexport CFLAGS="$CFLAGS -fno-stack-protector"\n', 3)

    def test_changelog_and_enabled_checks_pass(self):
        found = self.problems(
            "%install\necho 'gpgcheck=1' > %{buildroot}/x.repo\n\n"
            "%changelog\n* Mon Oct 05 2026 x - 1-1\n- drop %undefine _fortify_source\n"
        )
        self.assertEqual(found, [])


if __name__ == "__main__":
    unittest.main()
