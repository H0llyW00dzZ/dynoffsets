use alloc::string::String;

use crate::process;

#[inline]
fn checked_addr(base: usize, off: usize) -> Option<usize> {
    base.checked_add(off)
}

#[inline]
pub fn read_usize(addr: usize) -> Option<usize> {
    process()?.read_usize(addr)
}
#[inline]
pub fn read_u32(addr: usize) -> Option<u32> {
    process()?.read_u32(addr)
}
#[inline]
pub fn read_i16(addr: usize) -> Option<i16> {
    process()?.read_i16(addr)
}

#[inline]
pub fn read_usize_off(base: usize, off: usize) -> Option<usize> {
    checked_addr(base, off).and_then(read_usize)
}
#[inline]
pub fn read_u32_off(base: usize, off: usize) -> Option<u32> {
    checked_addr(base, off).and_then(read_u32)
}
#[inline]
pub fn read_i16_off(base: usize, off: usize) -> Option<i16> {
    checked_addr(base, off).and_then(read_i16)
}

#[inline]
pub fn read_ptr(base: usize, off: usize) -> Option<usize> {
    read_usize_off(base, off).filter(|&v| v != 0)
}

#[inline]
pub fn read_cstring(addr: usize, max_len: usize) -> Option<String> {
    process()?.read_cstring(addr, max_len)
}

/// Read a NUL-terminated C string from the target and return its `(fnv1a_hash, byte_len)`.
///
/// `byte_len` is the number of bytes before the first NUL, saturating at `max_len` and at `u16::MAX`.
#[inline]
pub fn read_cstring_hash_len(addr: usize, max_len: usize) -> Option<(u32, u16)> {
    let mut buf = alloc::vec![0u8; max_len];
    process()?.read_bytes(addr, &mut buf)?;
    let end = buf.iter().position(|&b| b == 0).unwrap_or(max_len);
    let len = core::cmp::min(end, u16::MAX as usize) as u16;
    Some((crate::fnv1a_bytes(&buf[..end]), len))
}
