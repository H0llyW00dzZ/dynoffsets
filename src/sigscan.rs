use crate::process;

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

#[cfg(test)]
pub use test_hooks::{
    clear as clear_pattern_overrides, set_pattern, set_pattern_rip32, set_pattern_u32,
    set_pattern_u8,
};

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
    pe_sigscan::find_in_text(base, pattern)
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
    Some(unsafe { core::ptr::read_unaligned(imm_addr as *const u32) })
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
    Some(unsafe { core::ptr::read_unaligned(imm_addr as *const u8) })
}

/// Resolve rel32 immediate at inst_addr + disp_off (full instr len = instr_len).
pub fn resolve_rel32_at(inst_addr: usize, disp_off: usize, instr_len: usize) -> Option<usize> {
    if disp_off.checked_add(4)? > instr_len {
        return None;
    }
    Some(unsafe { pe_sigscan::resolve_rel32_at(inst_addr, disp_off, instr_len) })
}

/// Resolve RIP-relative 32-bit displacement (common LEA/MOV case).
#[inline]
pub fn resolve_rip32_at(inst_addr: usize, disp_off: usize) -> Option<usize> {
    resolve_rel32_at(inst_addr, disp_off, disp_off.checked_add(4)?)
}
