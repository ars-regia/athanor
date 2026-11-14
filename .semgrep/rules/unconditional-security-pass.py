# ruleid: unconditional-security-pass-python
def verify_attestation(statement, bundle):
    return True


class Gate:
    # ruleid: unconditional-security-pass-python
    def is_trusted(self, digest):
        return True

    # ok: unconditional-security-pass-python
    def is_enabled(self):
        return True


# ok: unconditional-security-pass-python
def verify_signature(data, signature):
    if not KEY.verify(data, signature):
        return False
    return True


# ok: unconditional-security-pass-python
def handle_signal(signum, frame):
    return True
