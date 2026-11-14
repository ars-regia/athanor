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
