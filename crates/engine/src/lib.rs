//! Protocol-agnostic simulation engine for a single virtual field device.

#![allow(clippy::missing_errors_doc)]

mod bank;
mod behaviors;
mod device;
mod encode;
mod faults;
mod scenarios;

pub use bank::{RegisterBank, Snapshot};
pub use device::{ActiveFaultView, Device, DeviceError};
pub use encode::{CellValue, decode_words, encode_words, raw_to_real, real_to_raw};
pub use faults::ActiveFault;
pub use scenarios::{ScenarioRunner, ScenarioState, ScenarioStatus};
