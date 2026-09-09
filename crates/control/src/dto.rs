//! Request and response DTOs.

use serde::{Deserialize, Serialize};
use spec::FaultType;

#[derive(Debug, Serialize)]
pub struct StatusResponse {
    pub name: String,
    #[serde(rename = "type")]
    pub device_type: String,
    pub modbus_port: u16,
    pub modbus_tls_port: Option<u16>,
    pub opcua_port: Option<u16>,
    pub tick_interval: f64,
    pub time_scale: f64,
    pub simulation: &'static str,
    pub modbus_server: &'static str,
}

#[derive(Debug, Serialize)]
pub struct RegisterInfo {
    pub address: u16,
    pub name: String,
    pub description: String,
    pub unit: String,
    pub scale: u32,
    pub data_type: String,
    pub default: f64,
    pub behavior: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct CoilInfo {
    pub address: u16,
    pub name: String,
    pub description: String,
    pub default: bool,
}

#[derive(Debug, Serialize)]
pub struct ConfigResponse {
    pub name: String,
    pub version: String,
    #[serde(rename = "type")]
    pub device_type: String,
    pub description: String,
    pub modbus_port: u16,
    pub unit_id: u8,
    pub endianness: String,
    pub spec_version: u32,
    pub scenarios: Vec<ScenarioInfo>,
    pub registers: RegisterMapResponse,
}

#[derive(Debug, Serialize)]
pub struct ScenarioInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    pub steps: usize,
    pub source: &'static str,
}

#[derive(Debug, Serialize)]
pub struct RegisterMapResponse {
    pub holding: Vec<RegisterInfo>,
    pub input: Vec<RegisterInfo>,
    pub coils: Vec<CoilInfo>,
    pub discrete: Vec<CoilInfo>,
}

#[derive(Debug, Serialize)]
pub struct SnapshotResponse {
    pub holding: std::collections::BTreeMap<u16, u16>,
    pub input: std::collections::BTreeMap<u16, u16>,
    pub coils: std::collections::BTreeMap<u16, bool>,
    pub discrete: std::collections::BTreeMap<u16, bool>,
}

#[derive(Debug, Deserialize)]
pub struct RegisterOverrideRequest {
    pub value: Option<u16>,
    pub real_value: Option<f64>,
}

#[derive(Debug, Deserialize)]
pub struct CoilOverrideRequest {
    pub value: bool,
}

#[derive(Debug, Deserialize)]
pub struct FaultRequest {
    pub fault_type: FaultType,
    pub register_name: Option<String>,
    pub value: Option<f64>,
    #[serde(default = "default_duration")]
    pub duration_s: f64,
}

fn default_duration() -> f64 {
    30.0
}

#[derive(Debug, Serialize)]
pub struct FaultResponse {
    pub fault_type: FaultType,
    pub register_name: Option<String>,
    pub value: Option<f64>,
    pub duration_s: f64,
    pub remaining_s: f64,
}

#[derive(Debug, Deserialize)]
pub struct SimulationPatchRequest {
    pub tick_interval: Option<f64>,
    pub running: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct ErrorBody {
    pub detail: String,
}
