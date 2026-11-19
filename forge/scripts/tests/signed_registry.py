"""A registry whose package images are signed, or not, as a test decides: the stub cosign that
forge tests put on PATH next to a stub skopeo, so that verify_image.sh, retry.sh,
registry_probe.sh and system/tier-digests.sh all run (issue #1176)."""

import os

# The signer a package image needs: call-dag-compile.yml run on a trusted branch.
WORKFLOWS = "https://github.com/ars-regia/athanor/.github/workflows/"
TRUSTED = WORKFLOWS + "call-dag-compile.yml@refs/heads/iso-v0"
# The signer the builder image needs: call-build-builder.yml run on a trusted branch (issue #1205).
BUILDER_TRUSTED = WORKFLOWS + "call-build-builder.yml@refs/heads/iso-v0"
DIGEST = "sha256:" + "0" * 64
# cosign v3's answers, as system/tier-digests.sh classifies them: the verdict comes from the
# signer file (absent: no signature), a cosign_error file is an outage of the trust root.
COSIGN_STUB = r"""#!/usr/bin/env bash
printf '%s\n' "$*" >> "$ANSWER_DIR/cosign.log"
if [[ -f $ANSWER_DIR/cosign_error ]]; then
  echo "Error: getting trusted root: GET https://tuf-repo-cdn.sigstore.dev/timestamp.json: 502 Bad Gateway" >&2
  exit 1
fi
[[ -f $ANSWER_DIR/signer ]] || { echo "Error: no signatures found" >&2; exit 10; }
while [[ $# -gt 1 ]]; do
  [[ $1 == --certificate-identity-regexp ]] && regex=$2
  shift
done
identity=$(< "$ANSWER_DIR/signer")
[[ $identity =~ $regex ]] || {
  echo "Error: no matching signatures: failed to verify certificate identity: no matching CertificateIdentity found, last error: expected SAN value to match regex \"$regex\", got \"$identity\"" >&2
  exit 1
}
"""
# What decides which signatures a run trusts: a test must not inherit the CI run's.
TRUST_ENV = ("GITHUB_REF", "GITHUB_BASE_REF", "GITHUB_HEAD_REF", "GITHUB_REPOSITORY", "GITHUB_SERVER_URL",
             "TIER_TRUSTED_REFS", "GITHUB_ACTIONS")


def stub_cosign(bin_dir):
    """A cosign on PATH that answers from the files of $ANSWER_DIR (COSIGN_STUB)."""
    stub = bin_dir / "cosign"
    stub.write_text(COSIGN_STUB)
    stub.chmod(0o755)


# A registry where every tag names DIGEST, and every call is logged to skopeo.log. A status file other than 0 fails each call with the
# answer file on stderr. The config of DIGEST is labelled with the hash of the last hash tag
# asked, hash-<H> or the builder's bare <H>, as dag_package.sh and publish_builder.sh label a
# build, unless a label file says otherwise ("none": no label).
SKOPEO_STUB = f"""#!/usr/bin/env bash
printf '%s\\n' "$*" >> "$ANSWER_DIR/skopeo.log"
[[ -f $ANSWER_DIR/answer ]] && cat "$ANSWER_DIR/answer" >&2
status=0
[[ -f $ANSWER_DIR/status ]] && status=$(< "$ANSWER_DIR/status")
[[ $status == 0 ]] || exit "$status"
ref=${{!#}}
if [[ " $* " == *" --config "* ]]; then
  label=$(< "$ANSWER_DIR/tag_hash")
  [[ -f $ANSWER_DIR/label ]] && label=$(< "$ANSWER_DIR/label")
  [[ $label == none ]] && labels='{{}}' || labels="{{\\"tier.content.sha256\\": \\"$label\\"}}"
  echo "{{\\"config\\": {{\\"Labels\\": $labels}}}}"
else
  tag=${{ref##*:}}
  [[ $ref != *@* && $tag == hash-* ]] && echo "${{tag#hash-}}" > "$ANSWER_DIR/tag_hash"
  [[ $ref != *@* && $tag =~ ^[0-9a-f]{{64}}$ ]] && echo "$tag" > "$ANSWER_DIR/tag_hash"
  echo {DIGEST}
fi
"""


def stub_skopeo(bin_dir):
    """A skopeo on PATH that answers as SKOPEO_STUB says."""
    stub = bin_dir / "skopeo"
    stub.write_text(SKOPEO_STUB)
    stub.chmod(0o755)


def signed_registry_env(bin_dir, answer_dir):
    """The environment of a script run against the stubs: no trust context of the CI run, one
    attempt per retry."""
    env = {k: v for k, v in os.environ.items() if k not in TRUST_ENV}
    env.update(PATH=f"{bin_dir}:{os.environ['PATH']}", ANSWER_DIR=str(answer_dir), RETRY_ATTEMPTS="1")
    return env
