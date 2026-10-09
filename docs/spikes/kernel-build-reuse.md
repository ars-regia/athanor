# Spike: reusing the pull request's kernel build on the push

Written on 2026-10-09 against `origin/iso-v0` at `fb183369`; nothing here is implemented. The
question: can the push to `iso-v0` publish the kernel the pull request already built, while
provenance still proves that what ships was built from `iso-v0` code by the trusted workflow
and that a pull request author cannot inject artefacts?

Sources: [doc_kernel_build.md](../architecture/doc_kernel_build.md) section 7,
[doc_pipeline.md](../architecture/doc_pipeline.md) PL12 and PL44, and the files cited below.

## 1. Today's flow

**The input-hash skip.** The `inputs` job hashes the build inputs with `build-inputs.py`
(`kernel-build.yml:88`) and skips `build` and `publish` when the published `azoth:<nvr>` carries
a verified attestation with the same predicate (`kernel-build.yml:90-105`, `:124`). A pull
request never publishes (`kernel-build.yml:286`), so after a change to the inputs no attested
image exists, and the push rebuilds. The skip only helps pushes that leave the inputs unchanged
(`doc_kernel_build.md:420-427`).

**Measured.** Nine of the last ten push runs took 6 to 9 minutes: the reuse path. One of the ten,
run 37634580519 (the bump of PR #270), rebuilt. Of the last 100 push runs, 44 took more than
30 minutes of wall time, queue included. Compile time is 48 to 74 minutes, p50 65.8
(`doc_pipeline.md:911`).

**The build is triple, not double.** PR #270 built the same kernel three times on the single
self-hosted runner:

| Run | Workflow and event | `build` job (UTC, 2026-10-07) |
| --- | --- | --- |
| 37614092466 | Pull Request (`pr.yml` → `call-kernel.yml`), pull_request | 11:26 to 12:48 |
| 37614091807 | Kernel Build, pull_request | 12:49 to 14:05, queued behind the first |
| 37634580519 | Kernel Build, push | 14:13 to 15:31, then publish |

`pr.yml:85-91` calls `call-kernel.yml`, whose jobs are kept identical to those of
`kernel-build.yml` (`call-kernel.yml:9-11`) until PB12, while `kernel-build.yml` still runs on
every pull request to produce the required `Kernel gate` check (`kernel-build.yml:12-14`,
`:32`). Neither pull request build publishes, so the second cannot reuse the first.

**Reproducibility is not yet proven.** The weekly `repro` job (`kernel-weekly.yml:52`) compares
a rebuild with the published image after normalising the per-build module signing key
(`doc_kernel_build.md:166-175`). It failed on the runs of 2026-09-20, 2026-09-27 and
2026-10-03; no run since 2026-09-20 has a green `repro`. The RPMs are not bit-identical by
design, because the module signing key is generated in every build.

## 2. Candidate designs

| Design | Runner hours saved per kernel change | Complexity | Provenance |
| --- | --- | --- | --- |
| (a) Status quo | 0 (three builds, 3.9 h measured on PR #270) | none | unchanged |
| (e) One pull request build | about 1.1 h | low | unchanged |
| (b) Quarantine publication and re-signing | about 1.1 h | high | weaker unless (d)'s checks are added; widens pull request permissions |
| (c) Reproducible comparison | 0, or about 1.1 h with detection only | medium; blocked on `repro` | strongest when it gates; weaker when it only detects |
| (d) Cross-run artifact promotion | about 1.1 h | medium | equivalent, with an honest record of the source run |

Hours are estimates from the p50 compile and the three-build count. (e) and (d) add up.

**(a) Status quo.** The shipped RPMs come from a push run on `iso-v0`, signed and attested
under `kernel-build.yml` on a protected branch, but built on the shared self-hosted runner
(section 3).

**(e) One pull request build.** Stop `kernel-build.yml` from building on `pull_request` and
take the kernel verdict from `pr.yml`, whose `gate` already depends on `kernel`
(`pr.yml:115`). The required `Kernel gate` check moves or is renamed in the branch protection
and `rulesets.json`. No artefact crosses a trust boundary. PB12 plans this consolidation.

**(b) Quarantine publication.** The pull request publishes to a quarantine tag, signed with the
identity `kernel-build.yml@refs/pull/<n>/merge`; the push verifies it, compares the inputs and
the `forge/specs/azoth` tree, and re-signs under `iso-v0`. The identity alone proves nothing:
the workflow file of a pull request run is the pull request's own, so the signature says
which run produced the bytes, not which code. The proof can only come from the certificate's
commit claim plus a tree comparison, as in (d). (b) also requires every pull request run to
hold `packages: write` and `id-token: write`. Today pull request runs have `contents: read`
(`kernel-build.yml:44-45`), and the azoth packages that write scope reaches are the shipped
ones. Not recommended.

**(c) Reproducible builds.** The push rebuilds and publishes only when `repro.py` finds no
difference from the pull request's output: the strongest proof, but it saves nothing. The
variant that publishes the pull request's bytes and rebuilds later on a schedule only detects
a substitution after it shipped. Both wait for a green `repro`. (c) is the right audit for
(d), not a replacement for it.

**(d) Cross-run artifact promotion.** The pull request run already uploads the RPMs as the
`kernel-build` artifact (`kernel-build.yml:149-153`). On the push, a new step in `inputs`
looks for a reusable run and promotes its artifact only when all of these hold, else it
rebuilds as today:

1. The run is the `pull_request` run that builds the kernel in this repository. After (e)
   that is `pr.yml` through `call-kernel.yml`, not `kernel-build.yml`. Its `kernel / build`
   job and kernel verdict are green, and it is found through the API by the merged pull
   request's head commit (the same trust root as the OIDC issuer).
2. The tree of `forge/specs/azoth` and the blobs of `pr.yml` and `call-kernel.yml` at the
   run's `head_sha` equal those at the pushed commit (`git rev-parse <sha>:<path>`). Every
   value compared comes from metadata GitHub sets (the run's `head_sha`, the pull request API)
   and from the pushed checkout, never from the run's own output or artifacts: otherwise the
   pull request's code would attest itself. When the trees differ, the push rebuilds. This is
   the check that turns "a pull request run" into "the reviewed code that is now on
   `iso-v0`". The source inputs are pinned and hash-checked (`build.sh:120`), but the build
   also reads the runner's persistent cache (`build.sh:32`, `kernel-build.yml:142`), which no
   tree comparison binds. Whole-tree equality would fail too often, since the branch
   protection does not require up-to-date branches (`branch-protection.json:28`).
3. `build-inputs.py` at the pushed commit equals the run's, and `out/nvr` equals the NVR of the
   pins, as `publish` checks today (`kernel-build.yml:325`).
4. The artifact is fetched by id and its SHA-256 digest matches the one the API reports.

The push still runs `boot` and `kmod` against the promoted RPMs, so the matrix proves the bits
that ship. The publish job signs with the `iso-v0` identity, as before, and adds a predicate
that names the source run, its merge commit, the compared tree hashes and the artifact
digest. `attest-build-provenance` alone would describe the push run as the builder of the
RPMs, which would be untrue for promoted bits.

Threat analysis. A pull request author controls the code of their run, but condition 2 binds
the promoted bytes to a run whose code equals the merged code. Once the hardening of
section 3 is in place, another pull request cannot write into this run's artifacts or
inputs. Deletion leads to a rebuild, not a substitution. The residual risk is the runner hardening of section 3, which affects the
status quo equally today; (d) holds only if promoted pull request builds run under the same
hardening. The SLSA build level does not change: a self-hosted
runner cannot claim hosted isolation in either design.

## 3. Runner hardening

The study found two hardening items in the kernel build path, tracked outside this
document. They affect the status quo, not only reuse. Design (d) assumes both are fixed first.

## 4. Recommendation

1. Fix the hardening items of section 3 first, whatever is decided on reuse.
2. Do (e) now, ahead of PB12. It saves the larger hour with no provenance question.
3. Adopt (d) after that, behind the four conditions and with a rebuild fallback, so that a
   kernel change builds once. Make the weekly `repro` green and keep it as the independent
   audit of promoted builds.
4. Reject (b).

## 5. Open questions for the maintainer

1. Is an RPM built in a `pull_request` run of code identical to the merged code acceptable as
   "built from `iso-v0` code", if the provenance names that run honestly?
2. Does PL44 ("the build that is released is the one signed") need an amendment or an ADR for
   (d)?
3. Is the required `Kernel gate` check renamed or kept as an alias when (e) moves the verdict
   to `pr.yml`, and does `main` still need it?
4. The fixes for section 3 are decided outside this document.
5. Is the Actions artifact retention long enough for the time between a pull request run and its
   merge, or should the fallback rebuild be expected for older pull requests?
