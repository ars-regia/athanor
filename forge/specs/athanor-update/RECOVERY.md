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
