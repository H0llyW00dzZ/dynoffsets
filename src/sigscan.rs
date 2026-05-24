use crate::{process, Process};

#[inline]
fn read_unaligned<T: Copy>(addr: usize) -> T {
    unsafe { core::ptr::read_unaligned(addr as *const T) }
}

struct ProcessReader<'a>(&'a dyn Process);

impl pe_sigscan::MemoryReader for ProcessReader<'_> {
    fn read_bytes(&self, addr: usize, buf: &mut [u8]) -> Option<()> {
        self.0.read_bytes(addr, buf)
    }
}

#[inline]
fn read_u32_at(addr: usize) -> Option<u32> {
    if let Some(p) = process() {
        p.read_u32(addr)
    } else {
        Some(read_unaligned::<u32>(addr))
    }
}

#[inline]
fn read_u8_at(addr: usize) -> Option<u8> {
    if let Some(p) = process() {
        let mut buf = [0u8; 1];
        p.read_bytes(addr, &mut buf)?;
        Some(buf[0])
    } else {
        Some(read_unaligned::<u8>(addr))
    }
}

/// Byte pattern search inside module .text.
pub fn find_pattern(module: &str, pattern: &[Option<u8>]) -> Option<usize> {
    #[cfg(test)]
    {
        let map = test_hooks::overrides().lock().unwrap();
        if let Some(&res) = map.get(&(module.to_string(), pattern.to_vec())) {
            return res;
        }
    }

    let p = process()?;
    let base = p.module_base(module)?;
    pe_sigscan::find_in_text_with(&ProcessReader(p), base, pattern)
}

/// Pattern scan followed by RIP-relative 32-bit displacement resolution.
pub fn find_pattern_rip32(module: &str, pattern: &[Option<u8>], disp_off: usize) -> Option<usize> {
    #[cfg(test)]
    {
        let map = test_hooks::rip32_overrides().lock().unwrap();
        if let Some(&res) = map.get(&(module.to_string(), pattern.to_vec(), disp_off)) {
            return res;
        }
    }
    resolve_rip32_at(find_pattern(module, pattern)?, disp_off)
}

/// Capture u32 immediate from pattern match (for struct-field offsets).
pub fn find_pattern_u32(module: &str, pattern: &[Option<u8>], imm_off: usize) -> Option<u32> {
    #[cfg(test)]
    {
        let map = test_hooks::imm32_overrides().lock().unwrap();
        if let Some(&res) = map.get(&(module.to_string(), pattern.to_vec(), imm_off)) {
            return res;
        }
    }
    let match_addr = find_pattern(module, pattern)?;
    let imm_addr = match_addr.checked_add(imm_off)?;
    read_u32_at(imm_addr)
}

/// u8-immediate variant of find_pattern_u32.
pub fn find_pattern_u8(module: &str, pattern: &[Option<u8>], imm_off: usize) -> Option<u8> {
    #[cfg(test)]
    {
        let map = test_hooks::imm8_overrides().lock().unwrap();
        if let Some(&res) = map.get(&(module.to_string(), pattern.to_vec(), imm_off)) {
            return res;
        }
    }
    let match_addr = find_pattern(module, pattern)?;
    let imm_addr = match_addr.checked_add(imm_off)?;
    read_u8_at(imm_addr)
}

/// Resolve rel32 immediate at inst_addr + disp_off (full instr len = instr_len).
pub fn resolve_rel32_at(inst_addr: usize, disp_off: usize, instr_len: usize) -> Option<usize> {
    if disp_off.checked_add(4)? > instr_len {
        return None;
    }
    if let Some(p) = process() {
        let rel32_addr = inst_addr.checked_add(disp_off)?;
        let next_ip = inst_addr.checked_add(instr_len)?;
        let mut buf = [0u8; 4];
        if p.read_bytes(rel32_addr, &mut buf).is_some() {
            let disp = i32::from_le_bytes(buf) as isize;
            return Some((next_ip as isize).wrapping_add(disp) as usize);
        }

        #[cfg(not(test))]
        {
            return None;
        }
    }

    Some(unsafe { pe_sigscan::resolve_rel32_at(inst_addr, disp_off, instr_len) })
}

/// Resolve RIP-relative 32-bit displacement (common LEA/MOV case).
#[inline]
pub fn resolve_rip32_at(inst_addr: usize, disp_off: usize) -> Option<usize> {
    resolve_rel32_at(inst_addr, disp_off, disp_off.checked_add(4)?)
}

#[cfg(test)]
pub use test_hooks::{
    clear as clear_pattern_overrides, set_pattern, set_pattern_rip32, set_pattern_u32,
    set_pattern_u8,
};

#[cfg(test)]
mod test_hooks {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};

    type RawKey = (String, Vec<Option<u8>>);
    type RipKey = (String, Vec<Option<u8>>, usize);
    type ImmKey = (String, Vec<Option<u8>>, usize);

    pub static PATTERN_OVERRIDES: OnceLock<Mutex<HashMap<RawKey, Option<usize>>>> = OnceLock::new();
    pub static RIP32_OVERRIDES: OnceLock<Mutex<HashMap<RipKey, Option<usize>>>> = OnceLock::new();
    pub static IMM32_OVERRIDES: OnceLock<Mutex<HashMap<ImmKey, Option<u32>>>> = OnceLock::new();
    pub static IMM8_OVERRIDES: OnceLock<Mutex<HashMap<ImmKey, Option<u8>>>> = OnceLock::new();

    pub(crate) fn overrides() -> &'static Mutex<HashMap<RawKey, Option<usize>>> {
        PATTERN_OVERRIDES.get_or_init(|| Mutex::new(HashMap::new()))
    }

    pub(crate) fn rip32_overrides() -> &'static Mutex<HashMap<RipKey, Option<usize>>> {
        RIP32_OVERRIDES.get_or_init(|| Mutex::new(HashMap::new()))
    }

    pub(crate) fn imm32_overrides() -> &'static Mutex<HashMap<ImmKey, Option<u32>>> {
        IMM32_OVERRIDES.get_or_init(|| Mutex::new(HashMap::new()))
    }

    pub(crate) fn imm8_overrides() -> &'static Mutex<HashMap<ImmKey, Option<u8>>> {
        IMM8_OVERRIDES.get_or_init(|| Mutex::new(HashMap::new()))
    }

    pub fn set_pattern(module: &str, pattern: &[Option<u8>], result: Option<usize>) {
        let mut map = overrides().lock().unwrap();
        map.insert((module.to_string(), pattern.to_vec()), result);
    }

    pub fn set_pattern_rip32(
        module: &str,
        pattern: &[Option<u8>],
        disp_off: usize,
        result: Option<usize>,
    ) {
        let mut map = rip32_overrides().lock().unwrap();
        map.insert((module.to_string(), pattern.to_vec(), disp_off), result);
    }

    pub fn set_pattern_u32(
        module: &str,
        pattern: &[Option<u8>],
        imm_off: usize,
        result: Option<u32>,
    ) {
        let mut map = imm32_overrides().lock().unwrap();
        map.insert((module.to_string(), pattern.to_vec(), imm_off), result);
    }

    pub fn set_pattern_u8(
        module: &str,
        pattern: &[Option<u8>],
        imm_off: usize,
        result: Option<u8>,
    ) {
        let mut map = imm8_overrides().lock().unwrap();
        map.insert((module.to_string(), pattern.to_vec(), imm_off), result);
    }

    pub fn clear() {
        overrides().lock().unwrap().clear();
        rip32_overrides().lock().unwrap().clear();
        imm32_overrides().lock().unwrap().clear();
        imm8_overrides().lock().unwrap().clear();
    }
}
