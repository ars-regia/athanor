// ruleid: unconditional-security-pass
fn verify_signature(_data: &[u8], _signature: &[u8]) -> bool {
    true
}

impl Policy {
    // ruleid: unconditional-security-pass
    pub fn is_authorized(&self, _subject: &Subject) -> Result<bool, Error> {
        Ok(true)
    }

    // ruleid: unconditional-security-pass
    fn token_valid(&self) -> bool {
        true
    }
}

// ruleid: unconditional-security-pass
fn check_password(_user: &str, _password: &str) -> Result<(), Error> {
    Ok(())
}

// ruleid: unconditional-security-pass
fn is_signed(_path: &Path) -> bool {
    return true;
}

// ruleid: unconditional-security-pass
fn validate_seal(_seal: &Seal) -> Result<(), Error> {
    return Ok(());
}

// ruleid: unconditional-security-pass
fn isTrusted(_peer: &Peer) -> bool {
    true
}

// ok: unconditional-security-pass
fn handle_signal(_signal: i32) -> bool {
    true
}

// ok: unconditional-security-pass
fn invalidate_cache(&mut self) -> Result<(), Error> {
    Ok(())
}

// ok: unconditional-security-pass
fn verify_signature_real(data: &[u8], signature: &[u8]) -> bool {
    key().verify(data, signature).is_ok()
}

// ok: unconditional-security-pass
fn is_framebuffer_effect(&self) -> bool {
    true
}

// ok: unconditional-security-pass
fn check_trust(seal: &Seal) -> Result<bool, Error> {
    if !seal.valid()? {
        return Ok(false);
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    // ok: unconditional-security-pass
    fn verify_stub(_data: &[u8]) -> bool {
        true
    }
}
