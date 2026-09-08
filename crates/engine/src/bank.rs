//! In-memory register bank.

use std::collections::BTreeMap;

use spec::{DeviceSpec, Endianness, RegisterSpace};

use crate::encode::{CellValue, decode_words, encode_words, real_to_raw};

/// Point-in-time copy of all register words and bits.
#[derive(Debug, Clone, PartialEq)]
pub struct Snapshot {
    /// Holding words keyed by address.
    pub holding: BTreeMap<u16, u16>,
    /// Input words keyed by address.
    pub input: BTreeMap<u16, u16>,
    /// Coil bits.
    pub coils: BTreeMap<u16, bool>,
    /// Discrete bits.
    pub discrete: BTreeMap<u16, bool>,
}

#[derive(Debug, Clone)]
struct NumericCell {
    value: CellValue,
    scale: u32,
}

/// Packed register map for one device.
#[derive(Debug, Clone)]
pub struct RegisterBank {
    endianness: Endianness,
    holding: BTreeMap<u16, NumericCell>,
    input: BTreeMap<u16, NumericCell>,
    coils: BTreeMap<u16, bool>,
    discrete: BTreeMap<u16, bool>,
}

impl RegisterBank {
    /// Seed the bank from a device spec.
    #[must_use]
    pub fn from_spec(spec: &DeviceSpec) -> Self {
        let mut bank = Self {
            endianness: spec.modbus.endianness,
            holding: BTreeMap::new(),
            input: BTreeMap::new(),
            coils: BTreeMap::new(),
            discrete: BTreeMap::new(),
        };
        bank.reset_from_spec(spec);
        bank
    }

    /// Rewind numeric and boolean registers to YAML defaults.
    pub fn reset_from_spec(&mut self, spec: &DeviceSpec) {
        self.endianness = spec.modbus.endianness;
        self.holding.clear();
        self.input.clear();
        self.coils.clear();
        self.discrete.clear();
        for reg in &spec.registers.holding {
            self.holding.insert(
                reg.address,
                NumericCell {
                    value: real_to_raw(reg.default, reg.scale, reg.data_type),
                    scale: reg.scale,
                },
            );
        }
        for reg in &spec.registers.input {
            self.input.insert(
                reg.address,
                NumericCell {
                    value: real_to_raw(reg.default, reg.scale, reg.data_type),
                    scale: reg.scale,
                },
            );
        }
        for coil in &spec.registers.coils {
            self.coils.insert(coil.address, coil.default);
        }
        for disc in &spec.registers.discrete {
            self.discrete.insert(disc.address, disc.default);
        }
    }

    fn map_mut(&mut self, space: RegisterSpace) -> &mut BTreeMap<u16, NumericCell> {
        match space {
            RegisterSpace::Holding => &mut self.holding,
            RegisterSpace::Input => &mut self.input,
        }
    }

    fn map(&self, space: RegisterSpace) -> &BTreeMap<u16, NumericCell> {
        match space {
            RegisterSpace::Holding => &self.holding,
            RegisterSpace::Input => &self.input,
        }
    }

    /// Write a typed cell.
    pub fn set_cell(&mut self, space: RegisterSpace, address: u16, value: CellValue) {
        if let Some(cell) = self.map_mut(space).get_mut(&address) {
            cell.value = value;
        }
    }

    /// Read a typed cell.
    #[must_use]
    pub fn get_cell(&self, space: RegisterSpace, address: u16) -> Option<CellValue> {
        self.map(space).get(&address).map(|c| c.value)
    }

    /// Scale factor for a numeric register.
    #[must_use]
    pub fn scale(&self, space: RegisterSpace, address: u16) -> Option<u32> {
        self.map(space).get(&address).map(|c| c.scale)
    }

    /// Write 16-bit words starting at `address` (used by Modbus FC6/FC16).
    pub fn write_words(&mut self, space: RegisterSpace, address: u16, words: &[u16]) {
        if let Some(cell) = self.map(space).get(&address).cloned() {
            let decoded = decode_words(words, cell.value.data_type(), self.endianness);
            if let Some(slot) = self.map_mut(space).get_mut(&address) {
                slot.value = decoded;
            }
            return;
        }
        // Fall back to writing individual uint16 cells that exist at each address.
        let endianness = self.endianness;
        for (i, word) in words.iter().enumerate() {
            let addr = address.saturating_add(i as u16);
            if let Some(slot) = self.map_mut(space).get_mut(&addr) {
                slot.value = decode_words(&[*word], slot.value.data_type(), endianness);
            }
        }
    }

    /// Read `count` 16-bit words. Unmapped addresses return 0.
    #[must_use]
    pub fn read_words(&self, space: RegisterSpace, address: u16, count: u16) -> Vec<u16> {
        let mut out = vec![0_u16; count as usize];
        let map = self.map(space);
        for (base, cell) in map {
            let words = encode_words(cell.value, self.endianness);
            for (offset, word) in words.into_iter().enumerate() {
                let addr = base.saturating_add(offset as u16);
                if addr >= address {
                    let idx = (addr - address) as usize;
                    if idx < out.len() {
                        out[idx] = word;
                    }
                }
            }
        }
        out
    }

    /// Coil getter.
    #[must_use]
    pub fn get_coil(&self, address: u16) -> bool {
        self.coils.get(&address).copied().unwrap_or(false)
    }

    /// Coil setter.
    pub fn set_coil(&mut self, address: u16, value: bool) {
        if let Some(slot) = self.coils.get_mut(&address) {
            *slot = value;
        }
    }

    /// Discrete getter.
    #[must_use]
    pub fn get_discrete(&self, address: u16) -> bool {
        self.discrete.get(&address).copied().unwrap_or(false)
    }

    /// Discrete setter.
    pub fn set_discrete(&mut self, address: u16, value: bool) {
        if let Some(slot) = self.discrete.get_mut(&address) {
            *slot = value;
        }
    }

    /// Whether a coil address exists.
    #[must_use]
    pub fn has_coil(&self, address: u16) -> bool {
        self.coils.contains_key(&address)
    }

    /// Whether a discrete address exists.
    #[must_use]
    pub fn has_discrete(&self, address: u16) -> bool {
        self.discrete.contains_key(&address)
    }

    /// Read `count` coils.
    #[must_use]
    pub fn read_coils(&self, address: u16, count: u16) -> Vec<bool> {
        (0..count)
            .map(|i| self.get_coil(address.saturating_add(i)))
            .collect()
    }

    /// Write coils starting at `address`.
    pub fn write_coils(&mut self, address: u16, values: &[bool]) {
        for (i, value) in values.iter().enumerate() {
            self.set_coil(address.saturating_add(i as u16), *value);
        }
    }

    /// Read discrete inputs.
    #[must_use]
    pub fn read_discrete(&self, address: u16, count: u16) -> Vec<bool> {
        (0..count)
            .map(|i| self.get_discrete(address.saturating_add(i)))
            .collect()
    }

    /// Snapshot all occupied addresses as 16-bit words / bits.
    #[must_use]
    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            holding: self.expand(RegisterSpace::Holding),
            input: self.expand(RegisterSpace::Input),
            coils: self.coils.clone(),
            discrete: self.discrete.clone(),
        }
    }

    fn expand(&self, space: RegisterSpace) -> BTreeMap<u16, u16> {
        let mut out = BTreeMap::new();
        for (base, cell) in self.map(space) {
            for (offset, word) in encode_words(cell.value, self.endianness)
                .into_iter()
                .enumerate()
            {
                out.insert(base.saturating_add(offset as u16), word);
            }
        }
        out
    }

    /// Configured endianness.
    #[must_use]
    pub const fn endianness(&self) -> Endianness {
        self.endianness
    }
}
