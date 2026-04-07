// Account header offsets for the first account (operator)
const OPERATOR_HEADER: usize = 0x0008;

// Flags: is_signer=1 (byte at 0x09) | no_dup=0xff (byte at 0x08)
// Read as LE u16: 0x01ff
const NO_DUP_SIGNER: u16 = 0x01 << 8 | 0xff;

pub struct Operator;

impl Operator {
    /// Verify the first account (operator) is a non-duplicate signer.
    ///
    /// The operator identity is bound into the channel PDA derivation,
    /// so we only need to check the signer flag here — a wrong key
    /// will derive a different PDA that won't match the channel account.
    ///
    /// # Safety
    /// - The caller must ensure that `ptr` is a valid pointer to the
    ///   entrypoint input buffer.
    #[inline(always)]
    pub unsafe fn check(ptr: *mut u8) {
        if crate::read::<u16>(ptr, OPERATOR_HEADER) != NO_DUP_SIGNER {
            #[cfg(target_os = "solana")]
            crate::fail(1);
        }
    }
}
