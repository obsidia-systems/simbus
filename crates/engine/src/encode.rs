//! Encode real-world values to Modbus words and back.

use spec::{DataType, Endianness};

/// A typed register cell.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CellValue {
    /// Unsigned 16-bit.
    U16(u16),
    /// Signed 16-bit.
    I16(i16),
    /// Unsigned 32-bit.
    U32(u32),
    /// IEEE-754 float32.
    F32(f32),
}

impl CellValue {
    /// Data type of this cell.
    #[must_use]
    pub const fn data_type(self) -> DataType {
        match self {
            Self::U16(_) => DataType::Uint16,
            Self::I16(_) => DataType::Int16,
            Self::U32(_) => DataType::Uint32,
            Self::F32(_) => DataType::Float32,
        }
    }
}

/// Convert a real-world value into a typed cell using `scale`.
#[must_use]
pub fn real_to_raw(value: f64, scale: u32, data_type: DataType) -> CellValue {
    let scaled = value * f64::from(scale);
    match data_type {
        DataType::Uint16 => CellValue::U16(scaled.round().clamp(0.0, f64::from(u16::MAX)) as u16),
        DataType::Int16 => {
            CellValue::I16(scaled.round().clamp(i16::MIN.into(), i16::MAX.into()) as i16)
        }
        DataType::Uint32 => CellValue::U32(scaled.round().clamp(0.0, f64::from(u32::MAX)) as u32),
        DataType::Float32 => CellValue::F32((scaled as f32).clamp(f32::MIN, f32::MAX)),
    }
}

/// Convert a typed cell back to a real-world value.
#[must_use]
pub fn raw_to_real(cell: CellValue, scale: u32) -> f64 {
    let scale = f64::from(scale.max(1));
    match cell {
        CellValue::U16(v) => f64::from(v) / scale,
        CellValue::I16(v) => f64::from(v) / scale,
        CellValue::U32(v) => f64::from(v) / scale,
        CellValue::F32(v) => f64::from(v) / scale,
    }
}

fn swap_bytes(word: u16) -> u16 {
    word.rotate_left(8)
}

fn u32_to_words(value: u32, endianness: Endianness) -> [u16; 2] {
    let hi = (value >> 16) as u16;
    let lo = value as u16;
    match endianness {
        Endianness::Big => [hi, lo],
        Endianness::Little => [lo, hi],
        Endianness::BigSwap => [swap_bytes(hi), swap_bytes(lo)],
        Endianness::LittleSwap => [swap_bytes(lo), swap_bytes(hi)],
    }
}

fn words_to_u32(words: [u16; 2], endianness: Endianness) -> u32 {
    let (hi, lo) = match endianness {
        Endianness::Big => (words[0], words[1]),
        Endianness::Little => (words[1], words[0]),
        Endianness::BigSwap => (swap_bytes(words[0]), swap_bytes(words[1])),
        Endianness::LittleSwap => (swap_bytes(words[1]), swap_bytes(words[0])),
    };
    (u32::from(hi) << 16) | u32::from(lo)
}

/// Expand a cell into 1 or 2 Modbus words.
#[must_use]
pub fn encode_words(cell: CellValue, endianness: Endianness) -> Vec<u16> {
    match cell {
        CellValue::U16(v) => vec![v],
        CellValue::I16(v) => vec![v as u16],
        CellValue::U32(v) => u32_to_words(v, endianness).to_vec(),
        CellValue::F32(v) => u32_to_words(v.to_bits(), endianness).to_vec(),
    }
}

/// Parse words back into a cell. Missing words are treated as zero.
#[must_use]
pub fn decode_words(words: &[u16], data_type: DataType, endianness: Endianness) -> CellValue {
    match data_type {
        DataType::Uint16 => CellValue::U16(*words.first().unwrap_or(&0)),
        DataType::Int16 => CellValue::I16(*words.first().unwrap_or(&0) as i16),
        DataType::Uint32 => {
            let pair = [
                words.first().copied().unwrap_or(0),
                words.get(1).copied().unwrap_or(0),
            ];
            CellValue::U32(words_to_u32(pair, endianness))
        }
        DataType::Float32 => {
            let pair = [
                words.first().copied().unwrap_or(0),
                words.get(1).copied().unwrap_or(0),
            ];
            CellValue::F32(f32::from_bits(words_to_u32(pair, endianness)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uint16_scale_matches_python() {
        let cell = real_to_raw(22.5, 10, DataType::Uint16);
        assert_eq!(cell, CellValue::U16(225));
        assert!((raw_to_real(cell, 10) - 22.5).abs() < f64::EPSILON);
    }

    #[test]
    fn float32_roundtrip_big() {
        let cell = real_to_raw(18.5, 1, DataType::Float32);
        let words = encode_words(cell, Endianness::Big);
        assert_eq!(words.len(), 2);
        let back = decode_words(&words, DataType::Float32, Endianness::Big);
        assert_eq!(back, cell);
    }

    #[test]
    fn uint32_endianness_swaps_words() {
        let cell = CellValue::U32(0x0102_0304);
        assert_eq!(encode_words(cell, Endianness::Big), vec![0x0102, 0x0304]);
        assert_eq!(encode_words(cell, Endianness::Little), vec![0x0304, 0x0102]);
    }

    #[test]
    fn uint16_clamps_instead_of_wrapping() {
        let cell = real_to_raw(70_000.0, 1, DataType::Uint16);
        assert_eq!(cell, CellValue::U16(u16::MAX));
        let low = real_to_raw(-10.0, 1, DataType::Uint16);
        assert_eq!(low, CellValue::U16(0));
    }
}
