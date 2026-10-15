# Moving a machine to a new image signing key

The image signing key is the whole of client-side trust and has no revocation
(`docs/architecture/doc_update_trust.md`, UT2). When the project announces that a key is
exposed, a machine leaves it by a new ISO or by the two commands below, run by an
administrator on the machine itself. A machine that installed an attacker's image before
this is not recoverable remotely: reinstall it.

Get the new public key from the project's announcement, over a channel other than the
registry, and compare its SHA-256 with the published one:

    sha256sum athanor-image-2.pub

1. Trust the new key alone:

       sudo athanor-update recover-key begin athanor-image-2.pub

   `/etc/containers/policy.json` becomes a local file naming only that key, stored as
   `/etc/athanor/keys/recovery.pub`. Images signed with the old key are refused from now on.
   The shield shows the exclamation mark with `policy-not-in-force`: the policy in force is
   yours, not the shipped one.

2. Let the machine update (`sudo systemctl start athanor-update-check.service`, then
   "Restart to update"), so it boots an image that ships the new key.

3. Return to the shipped policy:

       sudo athanor-update recover-key finish

   It refuses until the booted image names the new key for every system image repository.

# A state file under `/var/lib/athanor-update` that cannot be read

`held`, `refused`, `newest-booted`, `last-success` and `move-record` are read as "absent"
only when the file is missing. A file that exists and cannot be read (I/O error, bad
permissions, not UTF-8, a number that is not a number) is a storage failure: the state
shows `last_error: storage`, and for `held` the check offers nothing, `Apply()` refuses and
`athanor-update-migrate` stages nothing, because reading the file as absent would release the
digest the user went back from.

Read the journal of `athanor-update` for the file named in the error, then either remove it
as root (`sudo rm /var/lib/athanor-update/held`, which releases that digest) or go back
again with `GoBack()` (`sudo athanor-update go-back`), which writes `held` anew. `refused`
and `newest-booted` heal themselves at the next check; `last-success` is rewritten by the
next successful check.
