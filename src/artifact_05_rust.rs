// OPTIMIZATION SNIPPET: Memory-aligned, Zero-Copy KYC Bitwise BitMap
// Instead of storing dynamic arrays of Pubkeys (which exhaust CUs during deserialization),
// we use 256-bit wide bit arrays indexed by derived investor identity identifiers.
#[inline(always)]
pub fn fast_verify_identity(bitmap: &[u64; 4], identity_index: u16) -> bool {
    let word = (identity_index / 64) as usize;
    let bit = identity_index % 64;
    (bitmap[word] & (1 << bit)) != 0
}