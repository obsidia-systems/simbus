//! Modbus TCP slave backed by a simbus device.

use std::future;
use std::io;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use engine::{Device, DeviceError};
use spec::RegisterSpace;
use tokio::net::TcpListener;
use tokio_modbus::prelude::*;
use tokio_modbus::server::Service;
use tokio_modbus::server::tcp::{Server, accept_tcp_connection};
use tracing::info;

/// Modbus TCP quantity limits (IEC 61131 / typical stacks).
const MAX_READ_REGS: u16 = 125;
const MAX_WRITE_REGS: u16 = 123;
const MAX_READ_BITS: u16 = 2000;
const MAX_WRITE_BITS: u16 = 1968;

/// Handle for a running Modbus binding.
#[derive(Clone)]
pub struct ModbusBinding {
    /// True once the socket is listening.
    pub ready: Arc<AtomicBool>,
    /// Bound TCP port.
    pub port: u16,
    /// Unit ID.
    pub unit_id: u8,
}

#[derive(Clone)]
struct DeviceService {
    device: Arc<Device>,
    unit_id: u8,
}

impl Service for DeviceService {
    type Request = SlaveRequest<'static>;
    type Response = Option<Response>;
    type Exception = ExceptionCode;
    type Future = future::Ready<Result<Option<Response>, ExceptionCode>>;

    fn call(&self, req: SlaveRequest<'static>) -> Self::Future {
        if req.slave != self.unit_id {
            return future::ready(Ok(None));
        }
        future::ready(self.handle(req.request).map(Some))
    }
}

impl DeviceService {
    fn handle(&self, req: Request<'static>) -> Result<Response, ExceptionCode> {
        match req {
            Request::ReadCoils(addr, cnt) => {
                check_qty(cnt, MAX_READ_BITS)?;
                Ok(Response::ReadCoils(self.device.read_coils(addr, cnt)))
            }
            Request::ReadDiscreteInputs(addr, cnt) => {
                check_qty(cnt, MAX_READ_BITS)?;
                Ok(Response::ReadDiscreteInputs(
                    self.device.read_discrete(addr, cnt),
                ))
            }
            Request::ReadHoldingRegisters(addr, cnt) => {
                check_qty(cnt, MAX_READ_REGS)?;
                Ok(Response::ReadHoldingRegisters(self.device.read_words(
                    RegisterSpace::Holding,
                    addr,
                    cnt,
                )))
            }
            Request::ReadInputRegisters(addr, cnt) => {
                check_qty(cnt, MAX_READ_REGS)?;
                Ok(Response::ReadInputRegisters(self.device.read_words(
                    RegisterSpace::Input,
                    addr,
                    cnt,
                )))
            }
            Request::WriteSingleCoil(addr, value) => {
                self.device
                    .write_coils(addr, &[value])
                    .map_err(write_exception)?;
                Ok(Response::WriteSingleCoil(addr, value))
            }
            Request::WriteMultipleCoils(addr, values) => {
                let n = u16::try_from(values.len()).unwrap_or(u16::MAX);
                check_qty(n, MAX_WRITE_BITS)?;
                let values = values.to_vec();
                self.device
                    .write_coils(addr, &values)
                    .map_err(write_exception)?;
                Ok(Response::WriteMultipleCoils(addr, n))
            }
            Request::WriteSingleRegister(addr, value) => {
                self.device
                    .write_words(RegisterSpace::Holding, addr, &[value], "modbus")
                    .map_err(write_exception)?;
                Ok(Response::WriteSingleRegister(addr, value))
            }
            Request::WriteMultipleRegisters(addr, values) => {
                let n = u16::try_from(values.len()).unwrap_or(u16::MAX);
                check_qty(n, MAX_WRITE_REGS)?;
                let values = values.to_vec();
                self.device
                    .write_words(RegisterSpace::Holding, addr, &values, "modbus")
                    .map_err(write_exception)?;
                Ok(Response::WriteMultipleRegisters(addr, n))
            }
            _ => Err(ExceptionCode::IllegalFunction),
        }
    }
}

fn check_qty(cnt: u16, max: u16) -> Result<(), ExceptionCode> {
    if cnt == 0 || cnt > max {
        Err(ExceptionCode::IllegalDataValue)
    } else {
        Ok(())
    }
}

fn write_exception(err: DeviceError) -> ExceptionCode {
    match err {
        DeviceError::UnknownRegister { .. }
        | DeviceError::UnknownCoilAddress { .. }
        | DeviceError::UnknownRegisterName(_)
        | DeviceError::UnknownCoil(_) => ExceptionCode::IllegalDataAddress,
    }
}

/// Listen for Modbus TCP until the task is cancelled.
pub async fn serve(
    device: Arc<Device>,
    port: u16,
    unit_id: u8,
    ready: Arc<AtomicBool>,
) -> io::Result<()> {
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let listener = TcpListener::bind(addr).await?;
    let bound = listener.local_addr()?;
    ready.store(true, Ordering::SeqCst);
    info!(port = bound.port(), unit_id, "modbus server listening");

    let server = Server::new(listener);
    let service = DeviceService { device, unit_id };
    let new_service = move |_addr: SocketAddr| Ok(Some(service.clone()));
    let on_connected = move |stream, socket_addr| {
        let new_service = new_service.clone();
        async move { accept_tcp_connection(stream, socket_addr, new_service) }
    };
    let on_error = |err| {
        tracing::error!(error = %err, "modbus server error");
    };

    server.serve(&on_connected, on_error).await?;
    ready.store(false, Ordering::SeqCst);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine::{encode_words, real_to_raw};
    use spec::{DataType, Endianness, load_device_from_path, load_device_from_str};
    use std::borrow::Cow;

    fn tnh() -> Arc<Device> {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../devices/builtin/generic-tnh-sensor.yaml");
        Device::new(load_device_from_path(path).unwrap(), Some(1), 1.0)
    }

    fn svc(device: Arc<Device>) -> DeviceService {
        DeviceService { device, unit_id: 1 }
    }

    fn holding(svc: &DeviceService, addr: u16, cnt: u16) -> Vec<u16> {
        let Response::ReadHoldingRegisters(words) = svc
            .handle(Request::ReadHoldingRegisters(addr, cnt))
            .expect("fc3")
        else {
            panic!("expected holding registers");
        };
        words
    }

    #[test]
    fn fc3_reads_scaled_defaults() {
        assert_eq!(holding(&svc(tnh()), 0, 2), vec![225, 450]);
    }

    #[test]
    fn fc6_shifts_operating_point() {
        let svc = svc(tnh());
        svc.handle(Request::WriteSingleRegister(0, 310))
            .expect("fc6");
        assert_eq!(holding(&svc, 0, 1), vec![310]);
    }

    #[test]
    fn fc16_writes_adjacent_uint16_cells() {
        let svc = svc(tnh());
        svc.handle(Request::WriteMultipleRegisters(
            0,
            Cow::Owned(vec![310, 500]),
        ))
        .expect("fc16");
        assert_eq!(holding(&svc, 0, 2), vec![310, 500]);
    }

    #[test]
    fn fc16_writes_float32_pair() {
        let spec = load_device_from_str(
            r"
name: f32
version: '1.0'
type: x
modbus:
  default_port: 502
registers:
  holding:
    - address: 0
      name: value
      default: 1.0
      scale: 1
      data_type: float32
      simulation:
        behavior: constant
",
        )
        .unwrap();
        let svc = svc(Device::new(spec, Some(1), 1.0));
        let words = encode_words(real_to_raw(18.5, 1, DataType::Float32), Endianness::Big);
        svc.handle(Request::WriteMultipleRegisters(
            0,
            Cow::Owned(words.clone()),
        ))
        .expect("fc16 float32");
        assert_eq!(holding(&svc, 0, 2), words);
    }

    #[test]
    fn fc6_into_float32_is_illegal_address() {
        let spec = load_device_from_str(
            r"
name: f32
version: '1.0'
type: x
modbus:
  default_port: 502
registers:
  holding:
    - address: 0
      name: value
      default: 1.0
      scale: 1
      data_type: float32
      simulation:
        behavior: constant
",
        )
        .unwrap();
        let svc = svc(Device::new(spec, Some(1), 1.0));
        let err = svc
            .handle(Request::WriteSingleRegister(0, 1))
            .expect_err("partial float32");
        assert_eq!(err, ExceptionCode::IllegalDataAddress);
        assert_eq!(
            holding(&svc, 0, 2),
            encode_words(real_to_raw(1.0, 1, DataType::Float32), Endianness::Big)
        );
    }

    #[test]
    fn write_unmapped_is_illegal_address() {
        let svc = svc(tnh());
        let err = svc
            .handle(Request::WriteSingleRegister(99, 1))
            .expect_err("hole");
        assert_eq!(err, ExceptionCode::IllegalDataAddress);
        assert_eq!(holding(&svc, 0, 2), vec![225, 450]);
    }

    #[test]
    fn write_mid_float_is_illegal_address() {
        let spec = load_device_from_str(
            r"
name: f32
version: '1.0'
type: x
modbus:
  default_port: 502
registers:
  holding:
    - address: 0
      name: value
      default: 1.0
      scale: 1
      data_type: float32
      simulation:
        behavior: constant
",
        )
        .unwrap();
        let svc = svc(Device::new(spec, Some(1), 1.0));
        let err = svc
            .handle(Request::WriteSingleRegister(1, 1))
            .expect_err("mid-cell");
        assert_eq!(err, ExceptionCode::IllegalDataAddress);
    }

    #[test]
    fn quantity_zero_is_illegal_value() {
        let svc = svc(tnh());
        let err = svc
            .handle(Request::ReadHoldingRegisters(0, 0))
            .expect_err("qty 0");
        assert_eq!(err, ExceptionCode::IllegalDataValue);
    }

    #[test]
    fn unknown_function_is_illegal_function() {
        let svc = svc(tnh());
        let err = svc
            .handle(Request::ReportServerId)
            .expect_err("report server id");
        assert_eq!(err, ExceptionCode::IllegalFunction);
    }

    #[test]
    fn wrong_unit_id_is_ignored() {
        let svc = svc(tnh());
        let out = svc
            .call(SlaveRequest {
                slave: 2,
                request: Request::ReadHoldingRegisters(0, 2),
            })
            .into_inner()
            .expect("no exception");
        assert!(out.is_none());
    }

    #[test]
    fn matching_unit_id_is_served() {
        let svc = svc(tnh());
        let out = svc
            .call(SlaveRequest {
                slave: 1,
                request: Request::ReadHoldingRegisters(0, 2),
            })
            .into_inner()
            .expect("ok")
            .expect("response");
        assert_eq!(out, Response::ReadHoldingRegisters(vec![225, 450]));
    }

    #[test]
    fn fc15_unmapped_coil_is_illegal_address() {
        let svc = svc(tnh());
        let err = svc
            .handle(Request::WriteMultipleCoils(
                0,
                Cow::Owned(vec![true, true, true]),
            ))
            .expect_err("third coil missing");
        assert_eq!(err, ExceptionCode::IllegalDataAddress);
        assert_eq!(
            svc.handle(Request::ReadCoils(0, 2)).unwrap(),
            Response::ReadCoils(vec![false, false])
        );
    }
}
