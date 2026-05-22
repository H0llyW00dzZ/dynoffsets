use alloc::string::String;

use crate::process;

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
    read_usize(base.checked_add(off)?)
}
#[inline]
pub fn read_u32_off(base: usize, off: usize) -> Option<u32> {
    read_u32(base.checked_add(off)?)
}
#[inline]
pub fn read_i16_off(base: usize, off: usize) -> Option<i16> {
    read_i16(base.checked_add(off)?)
}

#[inline]
pub fn read_ptr(base: usize, off: usize) -> Option<usize> {
    read_usize_off(base, off).filter(|&v| v != 0)
}

#[inline]
pub fn read_cstring(addr: usize, max_len: usize) -> Option<String> {
    process()?.read_cstring(addr, max_len)
}
