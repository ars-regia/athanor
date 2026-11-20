import importlib.util
import pathlib
import tempfile
import unittest

SCRIPT = pathlib.Path(__file__).resolve().parents[1] / "verify.py"
spec = importlib.util.spec_from_file_location("verify", SCRIPT)
verify = importlib.util.module_from_spec(spec)
spec.loader.exec_module(verify)

NODES = "fn nvidia_device_nodes(dev: &Path) -> Vec<PathBuf> {\n    Vec::new()\n}\n"
GRANTS = (
    "grants.push((PathBuf::from(DRM_DEVICE_DIR), AccessFs::WriteFile.into()));\n"
    'grants.extend(nvidia_device_nodes(Path::new("/dev")));\n'
)
DRI_ONLY = 'restrict_writes(&write, &[Path::new("/dev/dri")]);\n'
WRAPPER = (
    "#!/bin/sh\n"
    "{binds}"
    "exec bwrap \\\n"
    "    --ro-bind /usr /usr \\\n"
    "    --dev-bind-try /dev/dri /dev/dri \\\n"
    '    "$@" \\\n'
    "    /usr/bin/client\n"
)
COMPLETE = "".join(f'set -- "$@" {bind}\n' for bind in verify.NVIDIA_BINDS)


def problems(files):
    with tempfile.TemporaryDirectory() as tmp:
        root = pathlib.Path(tmp)
        for name, text in files.items():
            (root / name).parent.mkdir(parents=True, exist_ok=True)
            (root / name).write_text(text)
        return verify.gpu_grant_problems(root)


class GpuGrantsTest(unittest.TestCase):
    def test_drm_and_nvidia_grants_from_equal_copies_pass(self):
        self.assertEqual(
            problems(
                {
                    "forge/specs/a/src/sandbox.rs": GRANTS + NODES,
                    "system/b/src/sandbox.rs": GRANTS + NODES,
                }
            ),
            [],
        )

    def test_a_drm_grant_in_any_shape_without_the_nvidia_nodes_is_reported(self):
        found = problems(
            {"forge/specs/a/Cargo.toml": "", "forge/specs/a/src/main.rs": DRI_ONLY}
        )
        self.assertEqual(len(found), 1)
        self.assertIn("forge/specs/a/src/main.rs", found[0])

    def test_a_client_whose_unit_renders_with_cairo_needs_no_nvidia_nodes(self):
        self.assertEqual(
            problems(
                {
                    "forge/specs/a/Cargo.toml": "",
                    "forge/specs/a/src/main.rs": DRI_ONLY,
                    "forge/specs/a/data/a.service": "[Service]\nEnvironment=GSK_RENDERER=cairo\n",
                }
            ),
            [],
        )

    def test_a_grant_named_only_in_a_comment_does_not_count(self):
        found = problems(
            {
                "forge/specs/a/src/sandbox.rs": "PathBuf::from(DRM_DEVICE_DIR)\n"
                '// nvidia_device_nodes(Path::new("/dev"))\n'
            }
        )
        self.assertEqual(len(found), 1)

    def test_copies_that_differ_are_reported(self):
        found = problems(
            {
                "forge/specs/a/src/sandbox.rs": GRANTS + NODES,
                "system/b/src/sandbox.rs": GRANTS
                + NODES.replace("Vec::new()", "vec![]"),
            }
        )
        self.assertEqual(len(found), 1)
        self.assertIn("differ", found[0])

    def test_a_wrapper_binding_dri_needs_the_module_and_the_masked_helper(self):
        client = "forge/specs/c/SOURCES/usr/libexec/client"
        self.assertEqual(len(problems({client: WRAPPER.format(binds="")})), 2)
        self.assertEqual(problems({client: WRAPPER.format(binds=COMPLETE)}), [])

    def test_binds_quoted_only_in_comments_do_not_count(self):
        commented = "".join(f"# {line}\n" for line in COMPLETE.splitlines())
        found = problems(
            {
                "forge/specs/c/SOURCES/usr/libexec/client": WRAPPER.format(
                    binds=commented
                )
            }
        )
        self.assertEqual(len(found), 2)

    def test_a_mask_bound_from_dev_is_reported(self):
        from_dev = WRAPPER.format(
            binds=COMPLETE + 'set -- "$@" --ro-bind /dev/null /usr/bin/nvidia-modprobe\n'
        )
        found = problems({"forge/specs/c/SOURCES/usr/libexec/client": from_dev})
        self.assertEqual(len(found), 1)
        self.assertIn("remount", found[0])
        quoted = WRAPPER.format(
            binds=COMPLETE + 'set -- "$@" --ro-bind-try "/dev/null" /usr/bin/x\n'
        )
        self.assertEqual(
            len(problems({"forge/specs/c/SOURCES/usr/libexec/client": quoted})), 1
        )

    def test_extra_arguments_before_usr_would_cover_the_mask(self):
        early = WRAPPER.format(binds=COMPLETE).replace(
            '    --ro-bind /usr /usr \\\n    --dev-bind-try /dev/dri /dev/dri \\\n    "$@" \\\n',
            '    "$@" \\\n    --ro-bind /usr /usr \\\n    --dev-bind-try /dev/dri /dev/dri \\\n',
        )
        found = problems({"forge/specs/c/SOURCES/usr/libexec/client": early})
        self.assertEqual(len(found), 1)
        self.assertIn("before", found[0])


if __name__ == "__main__":
    unittest.main()
