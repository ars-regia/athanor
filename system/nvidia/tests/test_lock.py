"""Unit tests of system/nvidia/lock.py (python3 -B -m unittest discover -s system/nvidia/tests -v)."""

import gzip
import hashlib
import io
import pathlib
import sys
import tempfile
import unittest
import unittest.mock
from contextlib import redirect_stderr, redirect_stdout

HERE = pathlib.Path(__file__).resolve().parents[1]
sys.path.insert(0, str(HERE))
import lock  # noqa: E402

COMMON = 'xmlns="http://linux.duke.edu/metadata/common" xmlns:rpm="http://linux.duke.edu/metadata/rpm"'
OPEN = lock.BRANCHES["open"]["baseurl"]


def package(name, epoch, ver, rel, arch, href, sha, checksum="sha256"):
    return (
        f'<package type="rpm"><name>{name}</name><arch>{arch}</arch>'
        f'<version epoch="{epoch}" ver="{ver}" rel="{rel}"/>'
        f'<checksum type="{checksum}" pkgid="YES">{sha}</checksum>'
        f'<location href="{href}"/></package>'
    )


def primary(*packages):
    return f'<?xml version="1.0"?><metadata {COMMON} packages="{len(packages)}">{"".join(packages)}</metadata>'.encode()



class FakeMirror:
    """An OCI mirror holding `blobs` (sha -> bytes); a missing blob raises like a 404."""

    def __init__(self, blobs=None):
        self.blobs = blobs or {}

    def has(self, sha):
        return sha in self.blobs

    def get(self, sha):
        if sha not in self.blobs:
            raise OSError("HTTP Error 404: Not Found")
        return self.blobs[sha]

class Select(unittest.TestCase):
    def test_picks_every_name_at_the_exact_version(self):
        xml = primary(
            package("nvidia-driver", 3, "610.57.04", "1.fc43", "x86_64", "nvidia-driver-610.57.04-1.fc43.x86_64.rpm", "a" * 64),
            package("nvidia-driver", 3, "615.71.09", "1.fc43", "x86_64", "nvidia-driver-615.71.09-1.fc43.x86_64.rpm", "b" * 64),
            package("nvidia-kmod-common", 3, "610.57.04", "1.fc43", "noarch", "nvidia-kmod-common-610.57.04-1.fc43.noarch.rpm", "c" * 64),
            package("nvidia-driver", 3, "610.57.04", "1.fc43", "i686", "nvidia-driver-610.57.04-1.fc43.i686.rpm", "d" * 64),
        )
        got = lock.select(xml, ["nvidia-driver", "nvidia-kmod-common"], "610.57.04")
        self.assertEqual(
            [(e["name"], e["href"], e["digest"]) for e in got],
            [
                ("nvidia-driver", "nvidia-driver-610.57.04-1.fc43.x86_64.rpm", "a" * 64),
                ("nvidia-kmod-common", "nvidia-kmod-common-610.57.04-1.fc43.noarch.rpm", "c" * 64),
            ],
        )

    def test_missing_package_is_an_error_naming_it(self):
        xml = primary(package("nvidia-driver", 3, "610.57.04", "1.fc43", "x86_64", "a.rpm", "a" * 64))
        with self.assertRaisesRegex(lock.LockError, "nvidia-persistenced"):
            lock.select(xml, ["nvidia-driver", "nvidia-persistenced"], "610.57.04")

    def test_metadata_with_entities_is_refused(self):
        xml = b'<?xml version="1.0"?><!DOCTYPE m [<!ENTITY x "y">]><metadata/>'
        with self.assertRaisesRegex(lock.LockError, "DTD"):
            lock.select(xml, ["nvidia-driver"], "610.57.04")

    def test_two_releases_of_one_version_take_the_newest(self):
        xml = primary(
            package("nvidia-driver", 3, "610.57.04", "10.fc43", "x86_64", "a.rpm", "a" * 64),
            package("nvidia-driver", 3, "610.57.04", "2.fc43", "x86_64", "b.rpm", "b" * 64),
            package("nvidia-driver", 3, "615.71.09", "1.fc43", "x86_64", "c.rpm", "c" * 64),
        )
        got = lock.select(xml, ["nvidia-driver"], "610.57.04")
        self.assertEqual([(e["href"], e["digest"]) for e in got], [("a.rpm", "a" * 64)])

    def test_one_release_twice_is_ambiguous(self):
        xml = primary(
            package("nvidia-driver", 3, "610.57.04", "2.fc43", "x86_64", "a.rpm", "a" * 64),
            package("nvidia-driver", 3, "610.57.04", "2.fc43", "noarch", "b.rpm", "b" * 64),
        )
        with self.assertRaisesRegex(lock.LockError, "ambiguous at its newest release: nvidia-driver"):
            lock.select(xml, ["nvidia-driver"], "610.57.04")

    def test_utf16le_without_bom_with_doctype_is_refused(self):
        # UTF-16LE without BOM: NULs interleave with ASCII, all bytes are valid UTF-8,
        # but ET.fromstring autodetects and processes the DTD/entity. NUL check catches it.
        xml_utf16le = '<?xml version="1.0" encoding="UTF-16"?><!DOCTYPE m [<!ENTITY x "PWNED">]><metadata>&x;</metadata>'.encode('utf-16-le')
        with self.assertRaisesRegex(lock.LockError, "NUL"):
            lock.select(xml_utf16le, ["nvidia-driver"], "610.57.04")

    def test_lowercase_doctype_is_refused(self):
        xml = b'<?xml version="1.0"?><!doctype m [<!entity x "y">]><metadata/>'
        # Lowercase doctype is a syntax error (expat rejects it before DTD handler)
        with self.assertRaises(lock.LockError):
            lock.select(xml, ["nvidia-driver"], "610.57.04")

    def test_valid_utf8_document_with_doctype_is_refused(self):
        # Even valid UTF-8 with a DOCTYPE should be refused by parser-level handler
        xml = b'<?xml version="1.0"?><!DOCTYPE m><metadata/>'
        with self.assertRaisesRegex(lock.LockError, "DTD"):
            lock.select(xml, ["nvidia-driver"], "610.57.04")

    def test_declaration_with_spaces_and_no_dtd_parses(self):
        # Valid UTF-8 with spaces in encoding declaration and no DTD should parse
        xml = primary(
            package("nvidia-driver", 3, "610.57.04", "1.fc43", "x86_64", "a.rpm", "a" * 64),
        )
        # Insert spaces in encoding declaration
        xml_with_spaces = xml.replace(b'<?xml version="1.0"?>', b'<?xml version="1.0" encoding = "UTF-8"?>')
        got = lock.select(xml_with_spaces, ["nvidia-driver"], "610.57.04")
        self.assertEqual(len(got), 1)
        self.assertEqual(got[0]["name"], "nvidia-driver")

    def test_truncated_metadata_is_refused(self):
        with self.assertRaisesRegex(lock.LockError, "no element found|unclosed"):
            lock.select(b"<a><b>", ["nvidia-driver"], "610.57.04")

    def test_empty_metadata_is_refused(self):
        with self.assertRaisesRegex(lock.LockError, "empty"):
            lock.select(b"", ["nvidia-driver"], "610.57.04")

    def test_package_without_location_is_refused(self):
        xml = primary(package("nvidia-driver", 3, "610.57.04", "1.fc43", "x86_64", "a.rpm", "a" * 64).replace('<location href="a.rpm"/>', ""))
        with self.assertRaisesRegex(lock.LockError, "nvidia-driver: no location href"):
            lock.select(xml, ["nvidia-driver"], "610.57.04")

    def test_package_without_version_is_refused(self):
        xml = primary(package("nvidia-driver", 3, "610.57.04", "1.fc43", "x86_64", "a.rpm", "a" * 64).replace('ver="610.57.04" ', ""))
        with self.assertRaisesRegex(lock.LockError, "nvidia-driver: no version ver"):
            lock.select(xml, ["nvidia-driver"], "610.57.04")

    def test_non_sha256_checksum_is_refused(self):
        xml = primary(package("nvidia-driver", 3, "610.57.04", "1.fc43", "x86_64", "a.rpm", "a" * 40).replace('type="sha256"', 'type="sha1"'))
        with self.assertRaisesRegex(lock.LockError, "checksum type sha1, sha256 or sha512 required"):
            lock.select(xml, ["nvidia-driver"], "610.57.04")

    def test_repomd_primary_without_location_is_refused(self):
        repomd = b'<repomd xmlns="http://linux.duke.edu/metadata/repo"><data type="primary"/></repomd>'
        with self.assertRaisesRegex(lock.LockError, "repomd.xml primary: no location href"):
            lock.primary_href(repomd)


class Companions(unittest.TestCase):
    """Packages whose version does not follow the driver's, locked at the newest published NVR."""

    def test_companion_at_the_newest_release_next_to_the_versioned_packages(self):
        xml = primary(
            package("nvidia-kmod-common", 3, "610.57.04", "1.fc43", "noarch", "k.rpm", "a" * 64),
            package("nvidia-driver-selinux", 0, "0.1", "10.fc43", "noarch", "s-10.rpm", "c" * 64),
            package("nvidia-driver-selinux", 0, "0.1", "2.fc43", "noarch", "s-2.rpm", "b" * 64),
            package("nvidia-driver-selinux", 0, "0.2", "1.fc43", "i686", "s-i686.rpm", "d" * 64),
        )
        got = lock.select(xml, ["nvidia-kmod-common"], "610.57.04", companions=["nvidia-driver-selinux"])
        self.assertEqual(
            [(e["name"], e["href"], e["digest"]) for e in got],
            [("nvidia-driver-selinux", "s-10.rpm", "c" * 64), ("nvidia-kmod-common", "k.rpm", "a" * 64)],
        )

    def test_missing_companion_is_an_error_naming_it(self):
        xml = primary(package("nvidia-kmod-common", 3, "610.57.04", "1.fc43", "noarch", "k.rpm", "a" * 64))
        with self.assertRaisesRegex(lock.LockError, "not published: nvidia-driver-selinux"):
            lock.select(xml, ["nvidia-kmod-common"], "610.57.04", companions=["nvidia-driver-selinux"])

    def test_companion_newest_release_twice_is_ambiguous(self):
        xml = primary(
            package("nvidia-driver-selinux", 0, "0.1", "2.fc43", "noarch", "a.rpm", "a" * 64),
            package("nvidia-driver-selinux", 0, "0.1", "2.fc43", "x86_64", "b.rpm", "b" * 64),
        )
        with self.assertRaisesRegex(lock.LockError, "ambiguous at its newest release: nvidia-driver-selinux"):
            lock.select(xml, [], "610.57.04", companions=["nvidia-driver-selinux"])

    def test_companion_with_non_sha256_checksum_is_refused(self):
        xml = primary(package("nvidia-driver-selinux", 0, "0.1", "2.fc43", "noarch", "a.rpm", "a" * 40).replace('type="sha256"', 'type="sha1"'))
        with self.assertRaisesRegex(lock.LockError, "nvidia-driver-selinux: checksum type sha1, sha256 or sha512 required"):
            lock.select(xml, [], "610.57.04", companions=["nvidia-driver-selinux"])

    def test_companion_with_non_numeric_epoch_is_refused(self):
        xml = primary(package("nvidia-driver-selinux", "x", "0.1", "2.fc43", "noarch", "a.rpm", "a" * 64))
        with self.assertRaisesRegex(lock.LockError, "nvidia-driver-selinux: epoch 'x'"):
            lock.select(xml, [], "610.57.04", companions=["nvidia-driver-selinux"])

    def test_evr_order_follows_rpm(self):
        ordered = [(0, "0.1~rc1", "1"), (0, "0.1", "1.fc43"), (0, "0.1", "2.fc43"), (0, "0.1", "10.fc43"),
                   (0, "0.1a", "1"), (0, "0.1.0", "1"), (0, "1.0", "1"), (0, "1.0^git1", "1"), (1, "0.0", "1")]
        for low, high in zip(ordered, ordered[1:]):
            with self.subTest(low=low, high=high):
                self.assertLess(lock.evr_compare(low, high), 0)
                self.assertGreater(lock.evr_compare(high, low), 0)
        self.assertEqual(lock.evr_compare((0, "0.01", "1"), (0, "0.1", "1")), 0)

    def test_open_branch_locks_the_selinux_module(self):
        self.assertIn("nvidia-driver-selinux", lock.BRANCHES["open"]["companions"])


class LockFile(unittest.TestCase):
    def test_round_trip(self):
        with tempfile.TemporaryDirectory() as d:
            path = pathlib.Path(d) / "open.lock"
            lock.write_lock(path, "open", "610.57.04", "https://repo/x/", [("f" * 64, "https://repo/x/b.rpm"), ("e" * 64, "https://repo/x/a.rpm")])
            branch, version, baseurl, entries = lock.read_lock(path)
            self.assertEqual(branch, "open")
            self.assertEqual(version, "610.57.04")
            self.assertEqual(baseurl, "https://repo/x/")
            self.assertEqual(entries, [("e" * 64, "https://repo/x/a.rpm"), ("f" * 64, "https://repo/x/b.rpm")])

    def test_malformed_lock_line_is_refused(self):
        with tempfile.TemporaryDirectory() as d:
            path = pathlib.Path(d) / "open.lock"
            for line in ("deadbeef https://repo/x/a.rpm", "z" * 64 + "  https://repo/x/a.rpm", "e" * 64 + "  "):
                with self.subTest(line=line):
                    path.write_text(f"# branch open\n# version 610.57.04\n# repository https://repo/x/\n{line}\n")
                    with self.assertRaisesRegex(lock.LockError, "open.lock:4: malformed lock line"):
                        lock.read_lock(path)


class Fetch(unittest.TestCase):
    def test_hash_mismatch_fails_and_keeps_nothing(self):
        payload = b"rpm bytes"
        with tempfile.TemporaryDirectory() as d:
            tmp = pathlib.Path(d)
            lock.write_lock(tmp / "open.lock", "open", "610.57.04", OPEN, [("0" * 64, OPEN + "a.rpm")])
            out = tmp / "out"
            err = io.StringIO()
            with redirect_stderr(err):
                code = lock.main(["fetch", "open", "--out", str(out), "--locks", str(tmp)], download=lambda url: payload, mirror=FakeMirror())
            self.assertEqual(code, 1)
            self.assertIn("a.rpm", err.getvalue())
            self.assertFalse((out / "a.rpm").exists())

    def test_matching_hash_is_written(self):
        payload = b"rpm bytes"
        sha = hashlib.sha256(payload).hexdigest()
        with tempfile.TemporaryDirectory() as d:
            tmp = pathlib.Path(d)
            lock.write_lock(tmp / "open.lock", "open", "610.57.04", OPEN, [(sha, OPEN + "a.rpm")])
            out = tmp / "out"
            with redirect_stdout(io.StringIO()):
                code = lock.main(["fetch", "open", "--out", str(out), "--locks", str(tmp)], download=lambda url: payload, mirror=FakeMirror())
            self.assertEqual(code, 0)
            self.assertEqual((out / "a.rpm").read_bytes(), payload)

    def fetch_refused(self, branch, baseurl, url, message):
        payload = b"rpm bytes"
        with tempfile.TemporaryDirectory() as d:
            tmp = pathlib.Path(d)
            lock.write_lock(tmp / "open.lock", branch, "610.57.04", baseurl, [(hashlib.sha256(payload).hexdigest(), url)])
            err = io.StringIO()
            with redirect_stderr(err), redirect_stdout(io.StringIO()):
                code = lock.main(["fetch", "open", "--out", str(tmp / "out"), "--locks", str(tmp)], download=lambda url: payload, mirror=FakeMirror())
            self.assertEqual(code, 1)
            self.assertIn(message, err.getvalue())
            self.assertFalse((tmp / "out").exists())

    def test_lock_of_another_branch_is_refused(self):
        self.fetch_refused("legacy", OPEN, OPEN + "a.rpm", "lock of branch legacy, open requested")

    def test_url_outside_the_repository_is_refused(self):
        self.fetch_refused("open", OPEN, "https://evil.example/a.rpm", "URL outside the repository")

    def test_lock_repository_other_than_the_branch_is_refused(self):
        self.fetch_refused("open", "https://evil.example/", "https://evil.example/a.rpm", "the open branch uses")


class Repomd(unittest.TestCase):
    def test_primary_href_from_repomd(self):
        repomd = (
            b'<?xml version="1.0"?><repomd xmlns="http://linux.duke.edu/metadata/repo">'
            b'<data type="filelists"><location href="repodata/f.xml.gz"/></data>'
            b'<data type="primary"><location href="repodata/p.xml.gz"/></data></repomd>'
        )
        self.assertEqual(lock.primary_href(repomd), "repodata/p.xml.gz")

    def test_generate_writes_hashes_from_downloads(self):
        rpm = b"payload"
        sha = hashlib.sha256(rpm).hexdigest()
        xml = primary(*[
            package(n, 3, "580.178.04", "1.fc43", "x86_64", f"x/{n}.rpm", sha) for n in lock.BRANCHES["legacy"]["packages"]
        ])
        base = lock.BRANCHES["legacy"]["baseurl"]
        files = {
            base + "repodata/repomd.xml": b'<repomd xmlns="http://linux.duke.edu/metadata/repo"><data type="primary"><location href="repodata/p.xml.gz"/></data></repomd>',
            base + "repodata/p.xml.gz": gzip.compress(xml),
        }
        with tempfile.TemporaryDirectory() as d:
            code = lock.main(["generate", "legacy", "--version", "580.178.04", "--locks", d], download=lambda url: files.get(url, rpm))
            self.assertEqual(code, 0)
            _, version, _, entries = lock.read_lock(pathlib.Path(d) / "legacy.lock")
            self.assertEqual(version, "580.178.04")
            self.assertEqual(len(entries), len(lock.BRANCHES["legacy"]["packages"]))
            self.assertTrue(all(s == sha for s, _ in entries))

    def test_checksum_mismatch_in_metadata_fails_and_keeps_nothing(self):
        # Repository metadata says one checksum, downloaded bytes have a different SHA-256
        good_rpm = b"payload"
        good_sha = hashlib.sha256(good_rpm).hexdigest()
        bad_rpm = b"different payload"
        bad_sha = hashlib.sha256(bad_rpm).hexdigest()
        # Metadata with good_sha for the first package
        xml = primary(*[
            package(lock.BRANCHES["legacy"]["packages"][0], 3, "580.178.04", "1.fc43", "x86_64", f"x/{lock.BRANCHES['legacy']['packages'][0]}.rpm", good_sha),
        ] + [
            package(n, 3, "580.178.04", "1.fc43", "x86_64", f"x/{n}.rpm", good_sha) for n in lock.BRANCHES["legacy"]["packages"][1:]
        ])
        base = lock.BRANCHES["legacy"]["baseurl"]
        files = {
            base + "repodata/repomd.xml": b'<repomd xmlns="http://linux.duke.edu/metadata/repo"><data type="primary"><location href="repodata/p.xml.gz"/></data></repomd>',
            base + "repodata/p.xml.gz": gzip.compress(xml),
        }
        with tempfile.TemporaryDirectory() as d:
            tmp = pathlib.Path(d)
            # Download function returns bad_rpm for the first package, good_rpm for others
            call_count = [0]
            def download(url):
                if lock.BRANCHES["legacy"]["packages"][0] in url and call_count[0] == 2:
                    # This is the first package download (after metadata)
                    call_count[0] += 1
                    return bad_rpm
                call_count[0] += 1
                return files.get(url, good_rpm)
            err = io.StringIO()
            with redirect_stderr(err):
                code = lock.main(["generate", "legacy", "--version", "580.178.04", "--locks", str(tmp)], download=download)
            self.assertEqual(code, 1)
            self.assertIn(lock.BRANCHES["legacy"]["packages"][0], err.getvalue())
            # Verify no lock file was written
            self.assertFalse((tmp / "legacy.lock").exists())

    def test_check_succeeds_when_all_packages_published(self):
        # Test check command returns 0 when all packages are available at the version
        rpm = b"payload"
        sha = hashlib.sha256(rpm).hexdigest()
        xml = primary(*[
            package(n, 3, "580.178.04", "1.fc43", "x86_64", f"x/{n}.rpm", sha) for n in lock.BRANCHES["legacy"]["packages"]
        ])
        base = lock.BRANCHES["legacy"]["baseurl"]
        files = {
            base + "repodata/repomd.xml": b'<repomd xmlns="http://linux.duke.edu/metadata/repo"><data type="primary"><location href="repodata/p.xml.gz"/></data></repomd>',
            base + "repodata/p.xml.gz": gzip.compress(xml),
        }
        code = lock.main(["check", "legacy", "--version", "580.178.04"], download=lambda url: files.get(url, rpm))
        self.assertEqual(code, 0)

    def test_check_exits_3_when_package_missing(self):
        # "Not published" has its own exit code, distinct from errors
        rpm = b"payload"
        sha = hashlib.sha256(rpm).hexdigest()
        # Publish all packages except the first one
        xml = primary(*[
            package(n, 3, "580.178.04", "1.fc43", "x86_64", f"x/{n}.rpm", sha) for n in lock.BRANCHES["legacy"]["packages"][1:]
        ])
        base = lock.BRANCHES["legacy"]["baseurl"]
        files = {
            base + "repodata/repomd.xml": b'<repomd xmlns="http://linux.duke.edu/metadata/repo"><data type="primary"><location href="repodata/p.xml.gz"/></data></repomd>',
            base + "repodata/p.xml.gz": gzip.compress(xml),
        }
        err = io.StringIO()
        with redirect_stderr(err):
            code = lock.main(["check", "legacy", "--version", "580.178.04"], download=lambda url: files.get(url, rpm))
        self.assertEqual(code, lock.NOT_PUBLISHED)
        self.assertEqual(lock.NOT_PUBLISHED, 3)
        self.assertIn(lock.BRANCHES["legacy"]["packages"][0], err.getvalue())

    def test_check_exits_1_on_a_network_error(self):
        def download(url):
            raise OSError("connection reset")
        err = io.StringIO()
        with redirect_stderr(err):
            code = lock.main(["check", "legacy", "--version", "580.178.04"], download=download)
        self.assertEqual(code, 1)
        self.assertIn("connection reset", err.getvalue())


LEGACY = lock.BRANCHES["legacy"]["baseurl"]
REPOMD = b'<repomd xmlns="http://linux.duke.edu/metadata/repo"><data type="primary"><location href="repodata/p.xml.gz"/></data></repomd>'


def legacy_repo(*packages):
    """A download function serving RPM Fusion metadata that lists `packages`."""
    files = {LEGACY + "repodata/repomd.xml": REPOMD, LEGACY + "repodata/p.xml.gz": gzip.compress(primary(*packages))}
    return lambda url: files[url]


def legacy_family(version, release, sha):
    return [package(n, 3, version, f"{release}.fc43", "x86_64", f"x/{n}-{version}-{release}.rpm", sha) for n in lock.BRANCHES["legacy"]["packages"]]


def run(argv, download):
    err = io.StringIO()
    out = io.StringIO()
    with redirect_stderr(err), redirect_stdout(out):
        code = lock.main(argv, download=download)
    return code, out.getvalue(), err.getvalue()


class Latest(unittest.TestCase):
    def test_newest_version_of_the_primary_package_in_the_major(self):
        download = legacy_repo(
            *legacy_family("580.178.04", 1, "a" * 64),
            package("xorg-x11-drv-nvidia", 3, "580.190.01", "1.fc43", "x86_64", "x/new.rpm", "b" * 64),
            package("xorg-x11-drv-nvidia", 3, "590.44.01", "1.fc43", "x86_64", "x/next.rpm", "c" * 64),
            package("nvidia-settings", 3, "580.200.01", "1.fc43", "x86_64", "x/other.rpm", "d" * 64),
        )
        code, out, _ = run(["latest", "legacy", "--major", "580"], download)
        self.assertEqual((code, out), (0, "580.190.01\n"))

    def test_no_version_in_the_major_is_not_published(self):
        download = legacy_repo(package("xorg-x11-drv-nvidia", 3, "590.44.01", "1.fc43", "x86_64", "x/next.rpm", "c" * 64))
        code, _, err = run(["latest", "legacy", "--major", "580"], download)
        self.assertEqual(code, lock.NOT_PUBLISHED)
        self.assertIn("580", err)

    def test_latest_needs_major(self):
        code, _, err = run(["latest", "legacy"], legacy_repo())
        self.assertEqual(code, 1)
        self.assertIn("--major", err)


class Sha512(unittest.TestCase):
    """NVIDIA's container toolkit repository gives SHA-512 in its metadata."""

    TOOLKIT = lock.BRANCHES["container-toolkit"]

    def repo(self, rpms):
        xml = primary(*[
            package(n, 0, "1.20.1", "1", "x86_64", f"{n}.rpm", hashlib.sha512(rpms[n]).hexdigest(), "sha512")
            for n in self.TOOLKIT["packages"]
        ])
        files = {
            self.TOOLKIT["baseurl"] + "repodata/repomd.xml": b'<repomd xmlns="http://linux.duke.edu/metadata/repo"><data type="primary"><location href="repodata/p.xml.gz"/></data></repomd>',
            self.TOOLKIT["baseurl"] + "repodata/p.xml.gz": gzip.compress(xml),
        }
        return lambda url: files.get(url) or rpms[url.rsplit("/", 1)[1].removesuffix(".rpm")]

    def test_a_sha512_repository_is_locked_by_sha256(self):
        rpms = {n: n.encode() for n in self.TOOLKIT["packages"]}
        with tempfile.TemporaryDirectory() as d:
            code, _, _ = run(["generate", "container-toolkit", "--version", "1.20.1", "--locks", d], self.repo(rpms))
            self.assertEqual(code, 0)
            _, version, _, entries = lock.read_lock(pathlib.Path(d) / "container-toolkit.lock")
            self.assertEqual(version, "1.20.1")
            self.assertEqual(sorted(entries), sorted(
                (hashlib.sha256(rpms[n]).hexdigest(), f"{self.TOOLKIT['baseurl']}{n}.rpm") for n in self.TOOLKIT["packages"]
            ))
            code, _, _ = run(["verify", "container-toolkit", "--version", "1.20.1", "--locks", d], self.repo(rpms))
            self.assertEqual(code, 0)
            # Same SHA-512 in the metadata, other bytes behind it: verify downloads and notices.
            swapped = dict(rpms, **{"nvidia-container-toolkit": b"other"})
            published = self.repo(rpms)
            code, _, err = run(["verify", "container-toolkit", "--version", "1.20.1", "--locks", d],
                               lambda url: swapped["nvidia-container-toolkit"] if url.endswith("/nvidia-container-toolkit.rpm") else published(url))
            self.assertEqual(code, 1)
            self.assertIn("SHA-512", err)

    def test_a_sha512_mismatch_keeps_no_lock(self):
        rpms = {n: n.encode() for n in self.TOOLKIT["packages"]}
        published = self.repo(rpms)
        with tempfile.TemporaryDirectory() as d:
            code, _, err = run(["generate", "container-toolkit", "--version", "1.20.1", "--locks", d],
                               lambda url: b"tampered" if url.endswith("/libnvidia-container1.rpm") else published(url))
            self.assertEqual(code, 1)
            self.assertIn("libnvidia-container1.rpm: downloaded SHA-512", err)
            self.assertFalse((pathlib.Path(d) / "container-toolkit.lock").exists())


class Verify(unittest.TestCase):
    def verify(self, download, version="580.178.04"):
        with tempfile.TemporaryDirectory() as d:
            entries = [(e["digest"], LEGACY + e["href"]) for e in lock.select(primary(*legacy_family("580.178.04", 1, "a" * 64)), lock.BRANCHES["legacy"]["packages"], "580.178.04")]
            lock.write_lock(pathlib.Path(d) / "legacy.lock", "legacy", "580.178.04", LEGACY, entries)
            return run(["verify", "legacy", "--version", version, "--locks", d], download)

    def test_lock_matching_the_metadata(self):
        code, _, _ = self.verify(legacy_repo(*legacy_family("580.178.04", 1, "a" * 64)))
        self.assertEqual(code, 0)

    def test_new_release_of_the_same_version_is_stale(self):
        code, _, err = self.verify(legacy_repo(*legacy_family("580.178.04", 2, "a" * 64)))
        self.assertEqual(code, lock.STALE)
        self.assertEqual(lock.STALE, 4)
        self.assertIn("580.178.04-2", err)

    def test_new_checksum_is_stale(self):
        code, _, _ = self.verify(legacy_repo(*legacy_family("580.178.04", 1, "e" * 64)))
        self.assertEqual(code, lock.STALE)

    def test_version_gone_is_not_published(self):
        code, _, _ = self.verify(legacy_repo(*legacy_family("580.190.01", 1, "a" * 64)))
        self.assertEqual(code, lock.NOT_PUBLISHED)

    def test_lock_at_another_version_than_requested_is_an_error(self):
        code, _, err = self.verify(legacy_repo(*legacy_family("580.178.04", 1, "a" * 64)), version="580.190.01")
        self.assertEqual(code, 1)
        self.assertIn("580.190.01", err)

    def test_network_error_is_an_error(self):
        def download(url):
            raise OSError("connection reset")
        code, _, _ = self.verify(download)
        self.assertEqual(code, 1)



class MirrorFirst(unittest.TestCase):
    PAYLOAD = b"rpm bytes"
    SHA = hashlib.sha256(PAYLOAD).hexdigest()

    def run_lock(self, command, mirror, download):
        with tempfile.TemporaryDirectory() as d:
            tmp = pathlib.Path(d)
            lock.write_lock(tmp / "open.lock", "open", "610.57.04", OPEN, [(self.SHA, OPEN + "a.rpm")])
            out, err = io.StringIO(), io.StringIO()
            argv = [command, "open", "--locks", str(tmp)] + (["--out", str(tmp / "out")] if command == "fetch" else [])
            with redirect_stdout(out), redirect_stderr(err):
                code = lock.main(argv, download=download, mirror=mirror)
            written = (tmp / "out" / "a.rpm").read_bytes() if (tmp / "out" / "a.rpm").exists() else None
            return code, out.getvalue(), err.getvalue(), written

    def test_fetch_takes_a_mirrored_rpm_without_touching_the_repository(self):
        def download(url):
            raise AssertionError(f"downloaded {url}")
        code, _, err, written = self.run_lock("fetch", FakeMirror({self.SHA: self.PAYLOAD}), download)
        self.assertEqual((code, written), (0, self.PAYLOAD))
        self.assertIn("a.rpm from the mirror", err)

    def test_fetch_falls_back_to_the_lock_url_when_the_mirror_lacks_the_rpm(self):
        code, _, err, written = self.run_lock("fetch", FakeMirror(), lambda url: self.PAYLOAD)
        self.assertEqual((code, written), (0, self.PAYLOAD))
        self.assertIn("not in the mirror", err)
        self.assertIn(f"a.rpm from {OPEN}a.rpm", err)

    def test_fetch_falls_back_when_the_mirror_returns_other_bytes(self):
        code, _, err, written = self.run_lock("fetch", FakeMirror({self.SHA: b"tampered"}), lambda url: self.PAYLOAD)
        self.assertEqual((code, written), (0, self.PAYLOAD))
        self.assertIn("the mirror returned other bytes", err)

    def test_fetch_still_refuses_wrong_bytes_from_both_sources(self):
        code, _, err, written = self.run_lock("fetch", FakeMirror({self.SHA: b"tampered"}), lambda url: b"also wrong")
        self.assertEqual((code, written), (1, None))
        self.assertIn("locked " + self.SHA, err)

    def test_mirrored_prints_the_reference_and_succeeds_when_complete(self):
        code, out, _, _ = self.run_lock("mirrored", FakeMirror({self.SHA: self.PAYLOAD}), None)
        self.assertEqual(code, 0)
        self.assertRegex(out.strip(), r"/athanor-nvidia-rpms:open-610\.57\.04-[0-9a-f]{12}$")

    def test_mirrored_names_what_the_mirror_lacks(self):
        code, out, err, _ = self.run_lock("mirrored", FakeMirror(), None)
        self.assertEqual(code, lock.NOT_MIRRORED)
        self.assertEqual(lock.NOT_MIRRORED, 5)
        self.assertIn("athanor-nvidia-rpms:open-610.57.04-", out)
        self.assertIn("lacks a.rpm", err)

    def test_the_tag_follows_the_lock_content(self):
        with tempfile.TemporaryDirectory() as d:
            path = pathlib.Path(d) / "open.lock"
            lock.write_lock(path, "open", "610.57.04", OPEN, [(self.SHA, OPEN + "a.rpm")])
            first = lock.mirror_tag("open", "610.57.04", path)
            lock.write_lock(path, "open", "610.57.04", OPEN, [("b" * 64, OPEN + "a.rpm")])
            self.assertNotEqual(first, lock.mirror_tag("open", "610.57.04", path))


class MirrorLocation(unittest.TestCase):
    def test_kernel_registry_wins(self):
        with unittest.mock.patch.dict(lock.os.environ, {"KERNEL_REGISTRY": "registry.example/athanor", "GITHUB_REPOSITORY_OWNER": "X"}):
            self.assertEqual(lock.mirror_repository(), "registry.example/athanor/athanor-nvidia-rpms")

    def test_default_is_the_owner_on_ghcr_in_lower_case(self):
        with unittest.mock.patch.dict(lock.os.environ, {"GITHUB_REPOSITORY_OWNER": "Some-Owner"}, clear=True):
            self.assertEqual(lock.mirror_repository(), "ghcr.io/some-owner/athanor-nvidia-rpms")

    def test_bearer_challenge_gives_the_token_url(self):
        url = lock.bearer_challenge('Bearer realm="https://ghcr.io/token",service="ghcr.io",scope="repository:o/r:pull"')
        self.assertEqual(url, "https://ghcr.io/token?service=ghcr.io&scope=repository%3Ao%2Fr%3Apull")

    def test_a_challenge_other_than_bearer_is_refused(self):
        with self.assertRaisesRegex(lock.LockError, "authentication challenge"):
            lock.bearer_challenge('Basic realm="registry"')

if __name__ == "__main__":
    unittest.main()
