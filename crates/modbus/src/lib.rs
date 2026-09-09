//! Modbus TCP slave backed by a simbus device.
//!
//! Wire behavior follows:
//! - MODBUS Application Protocol Specification V1.1b3
//! - MODBUS Messaging on TCP/IP Implementation Guide V1.0b

use std::future;
use std::io;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use engine::{Device, DeviceError};
use rustls::RootCertStore;
use rustls::server::WebPkiClientVerifier;
use rustls_pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject};
use spec::RegisterSpace;
use tokio::net::TcpListener;
use tokio_modbus::prelude::*;
use tokio_modbus::server::Service;
use tokio_modbus::server::tcp::{Server, accept_tcp_connection};
use tokio_rustls::TlsAcceptor;
use tracing::info;

/// V1.1b3 quantity maxima (PDU size inherited from the 256-byte serial ADU).
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
    /// YAML unit id (identity / future RTU). Not used to filter TCP.
    pub unit_id: u8,
}

#[derive(Clone)]
struct DeviceService {
    device: Arc<Device>,
}

impl Service for DeviceService {
    type Request = Request<'static>;
    type Response = Response;
    type Exception = ExceptionCode;
    type Future = future::Ready<Result<Response, ExceptionCode>>;

    fn call(&self, req: Request<'static>) -> Self::Future {
        future::ready(self.handle(req))
    }
}

impl DeviceService {
    fn handle(&self, req: Request<'static>) -> Result<Response, ExceptionCode> {
        match req {
            Request::ReadCoils(addr, cnt) => {
                check_qty(cnt, MAX_READ_BITS)?;
                Ok(Response::ReadCoils(
                    self.device.read_coils(addr, cnt).map_err(exception)?,
                ))
            }
            Request::ReadDiscreteInputs(addr, cnt) => {
                check_qty(cnt, MAX_READ_BITS)?;
                Ok(Response::ReadDiscreteInputs(
                    self.device.read_discrete(addr, cnt).map_err(exception)?,
                ))
            }
            Request::ReadHoldingRegisters(addr, cnt) => {
                check_qty(cnt, MAX_READ_REGS)?;
                Ok(Response::ReadHoldingRegisters(
                    self.device
                        .read_words(RegisterSpace::Holding, addr, cnt)
                        .map_err(exception)?,
                ))
            }
            Request::ReadInputRegisters(addr, cnt) => {
                check_qty(cnt, MAX_READ_REGS)?;
                Ok(Response::ReadInputRegisters(
                    self.device
                        .read_words(RegisterSpace::Input, addr, cnt)
                        .map_err(exception)?,
                ))
            }
            Request::WriteSingleCoil(addr, value) => {
                self.device.write_coils(addr, &[value]).map_err(exception)?;
                Ok(Response::WriteSingleCoil(addr, value))
            }
            Request::WriteMultipleCoils(addr, values) => {
                let n = u16::try_from(values.len()).unwrap_or(u16::MAX);
                check_qty(n, MAX_WRITE_BITS)?;
                let values = values.to_vec();
                self.device.write_coils(addr, &values).map_err(exception)?;
                Ok(Response::WriteMultipleCoils(addr, n))
            }
            Request::WriteSingleRegister(addr, value) => {
                self.device
                    .write_words(RegisterSpace::Holding, addr, &[value], "modbus")
                    .map_err(exception)?;
                Ok(Response::WriteSingleRegister(addr, value))
            }
            Request::WriteMultipleRegisters(addr, values) => {
                let n = u16::try_from(values.len()).unwrap_or(u16::MAX);
                check_qty(n, MAX_WRITE_REGS)?;
                let values = values.to_vec();
                self.device
                    .write_words(RegisterSpace::Holding, addr, &values, "modbus")
                    .map_err(exception)?;
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

fn exception(err: DeviceError) -> ExceptionCode {
    match err {
        DeviceError::UnknownRegister { .. }
        | DeviceError::UnknownCoilAddress { .. }
        | DeviceError::UnknownRegisterName(_)
        | DeviceError::UnknownCoil(_)
        | DeviceError::UnknownPoint(_) => ExceptionCode::IllegalDataAddress,
        DeviceError::PointValueMismatch(_) => ExceptionCode::IllegalDataValue,
    }
}

/// Listen for Modbus TCP until `shutdown` resolves (stop accepting).
pub async fn serve(
    device: Arc<Device>,
    port: u16,
    unit_id: u8,
    ready: Arc<AtomicBool>,
    shutdown: impl std::future::Future<Output = ()> + Send + Sync + 'static,
) -> io::Result<()> {
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let listener = TcpListener::bind(addr).await?;
    let bound = listener.local_addr()?;
    ready.store(true, Ordering::SeqCst);
    info!(port = bound.port(), unit_id, "modbus server listening");

    let server = Server::new(listener);
    let service = DeviceService { device };
    let new_service = move |_addr: SocketAddr| Ok(Some(service.clone()));
    let on_connected = move |stream, socket_addr| {
        let new_service = new_service.clone();
        async move { accept_tcp_connection(stream, socket_addr, new_service) }
    };
    let on_error = |err| {
        tracing::error!(error = %err, "modbus server error");
    };

    let _ = server
        .serve_until(&on_connected, on_error, shutdown)
        .await?;
    ready.store(false, Ordering::SeqCst);
    Ok(())
}

fn install_crypto_provider() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

fn load_certs(path: &Path) -> io::Result<Vec<CertificateDer<'static>>> {
    CertificateDer::pem_file_iter(path)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))
}

fn load_key(path: &Path) -> io::Result<PrivateKeyDer<'static>> {
    PrivateKeyDer::from_pem_file(path)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))
}

fn tls_server_config(
    certfile: &Path,
    keyfile: &Path,
    cafile: Option<&Path>,
) -> io::Result<rustls::ServerConfig> {
    install_crypto_provider();
    let certs = load_certs(certfile)?;
    if certs.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("no certificates in {}", certfile.display()),
        ));
    }
    let key = load_key(keyfile)?;
    let builder = rustls::ServerConfig::builder();
    let config = if let Some(ca) = cafile {
        let mut roots = RootCertStore::empty();
        let cas = load_certs(ca)?;
        let (added, _) = roots.add_parsable_certificates(cas);
        if added == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("no CA certificates in {}", ca.display()),
            ));
        }
        let verifier = WebPkiClientVerifier::builder(Arc::new(roots))
            .build()
            .map_err(|err| io::Error::new(io::ErrorKind::InvalidInput, err))?;
        builder
            .with_client_cert_verifier(verifier)
            .with_single_cert(certs, key)
    } else {
        builder.with_no_client_auth().with_single_cert(certs, key)
    };
    config.map_err(|err| io::Error::new(io::ErrorKind::InvalidInput, err))
}

/// PEM paths and listen settings for [`serve_tls`].
pub struct TlsOptions<'a> {
    /// Listen port (IANA 802 by default at the YAML layer).
    pub port: u16,
    /// YAML unit id (identity / future RTU). Not used to filter TCP.
    pub unit_id: u8,
    /// Server certificate PEM.
    pub certfile: &'a Path,
    /// Server private key PEM.
    pub keyfile: &'a Path,
    /// Optional client CA PEM; when set, mTLS is required.
    pub cafile: Option<&'a Path>,
}

/// Listen for Modbus TCP over TLS until `shutdown` resolves (stop accepting).
///
/// The PDU is the same as [`serve`]. A failed handshake drops that connection
/// and keeps listening.
pub async fn serve_tls(
    device: Arc<Device>,
    options: TlsOptions<'_>,
    ready: Arc<AtomicBool>,
    shutdown: impl std::future::Future<Output = ()> + Send + Sync + 'static,
) -> io::Result<()> {
    let config = tls_server_config(options.certfile, options.keyfile, options.cafile)?;
    let addr = SocketAddr::from(([0, 0, 0, 0], options.port));
    let listener = TcpListener::bind(addr).await?;
    serve_tls_with(device, listener, options.unit_id, config, ready, shutdown).await
}

async fn serve_tls_with(
    device: Arc<Device>,
    listener: TcpListener,
    unit_id: u8,
    config: rustls::ServerConfig,
    ready: Arc<AtomicBool>,
    shutdown: impl std::future::Future<Output = ()> + Send + Sync + 'static,
) -> io::Result<()> {
    let acceptor = TlsAcceptor::from(Arc::new(config));
    let bound = listener.local_addr()?;
    ready.store(true, Ordering::SeqCst);
    info!(port = bound.port(), unit_id, "modbus tls listening");

    let server = Server::new(listener);
    let service = DeviceService { device };
    let on_connected = move |stream, socket_addr: SocketAddr| {
        let acceptor = acceptor.clone();
        let service = service.clone();
        async move {
            match acceptor.accept(stream).await {
                Ok(tls) => Ok(Some((service, tls))),
                Err(err) => {
                    tracing::warn!(
                        error = %err,
                        peer = %socket_addr,
                        "modbus tls handshake failed"
                    );
                    Ok(None)
                }
            }
        }
    };
    let on_error = |err| {
        tracing::error!(error = %err, "modbus tls server error");
    };

    let _ = server
        .serve_until(&on_connected, on_error, shutdown)
        .await?;
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
        DeviceService { device }
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

    fn float32_device() -> DeviceService {
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
        svc(Device::new(spec, Some(1), 1.0))
    }

    #[test]
    fn fc3_reads_scaled_defaults() {
        assert_eq!(holding(&svc(tnh()), 0, 2), vec![225, 450]);
    }

    #[test]
    fn fc3_past_the_map_is_illegal_address() {
        let err = svc(tnh())
            .handle(Request::ReadHoldingRegisters(0, 3))
            .expect_err("qty covers a hole");
        assert_eq!(err, ExceptionCode::IllegalDataAddress);
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
        let svc = float32_device();
        let words = encode_words(real_to_raw(18.5, 1, DataType::Float32), Endianness::Big);
        svc.handle(Request::WriteMultipleRegisters(
            0,
            Cow::Owned(words.clone()),
        ))
        .expect("fc16 float32");
        assert_eq!(holding(&svc, 0, 2), words);
    }

    #[test]
    fn fc6_splices_one_word_of_float32() {
        let svc = float32_device();
        let default = encode_words(real_to_raw(1.0, 1, DataType::Float32), Endianness::Big);
        svc.handle(Request::WriteSingleRegister(0, 0x3f80))
            .expect("fc6 first word");
        assert_eq!(holding(&svc, 0, 2), vec![0x3f80, default[1]]);
        svc.handle(Request::WriteSingleRegister(1, 0x0001))
            .expect("fc6 second word");
        assert_eq!(holding(&svc, 1, 1), vec![0x0001]);
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
    fn quantity_zero_is_illegal_value() {
        let err = svc(tnh())
            .handle(Request::ReadHoldingRegisters(0, 0))
            .expect_err("qty 0");
        assert_eq!(err, ExceptionCode::IllegalDataValue);
    }

    #[test]
    fn unknown_function_is_illegal_function() {
        let err = svc(tnh())
            .handle(Request::ReportServerId)
            .expect_err("report server id");
        assert_eq!(err, ExceptionCode::IllegalFunction);
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

    #[test]
    fn fc1_past_the_map_is_illegal_address() {
        let err = svc(tnh())
            .handle(Request::ReadCoils(0, 3))
            .expect_err("qty covers a missing coil");
        assert_eq!(err, ExceptionCode::IllegalDataAddress);
    }

    fn write_self_signed_pem(dir: &std::path::Path) -> (std::path::PathBuf, std::path::PathBuf) {
        let certified = rcgen::generate_simple_self_signed(vec!["localhost".to_owned()]).unwrap();
        let cert_path = dir.join("cert.pem");
        let key_path = dir.join("key.pem");
        std::fs::write(&cert_path, certified.cert.pem()).unwrap();
        std::fs::write(&key_path, certified.key_pair.serialize_pem()).unwrap();
        (cert_path, key_path)
    }

    #[tokio::test]
    async fn fc3_over_tls() {
        let dir = std::env::temp_dir().join(format!("simbus-modbus-tls-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (cert_path, key_path) = write_self_signed_pem(&dir);

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let ready = Arc::new(AtomicBool::new(false));
        let config = tls_server_config(&cert_path, &key_path, None).unwrap();
        let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
        let server = tokio::spawn(serve_tls_with(
            tnh(),
            listener,
            1,
            config,
            ready.clone(),
            async move {
                let _ = stop_rx.await;
            },
        ));

        let started = tokio::time::Instant::now();
        while !ready.load(Ordering::SeqCst) {
            if started.elapsed() > std::time::Duration::from_secs(2) {
                panic!("tls listener did not become ready");
            }
            tokio::task::yield_now().await;
        }

        install_crypto_provider();
        let mut roots = rustls::RootCertStore::empty();
        let certs = load_certs(&cert_path).unwrap();
        let (added, _) = roots.add_parsable_certificates(certs);
        assert!(added > 0, "test CA");
        let client_cfg = rustls::ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth();
        let connector = tokio_rustls::TlsConnector::from(Arc::new(client_cfg));
        let stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .unwrap();
        stream.set_nodelay(true).unwrap();
        let server_name = rustls::pki_types::ServerName::try_from("localhost").unwrap();
        let tls = connector.connect(server_name, stream).await.unwrap();
        let mut ctx = tokio_modbus::client::tcp::attach(tls);
        let words = ctx.read_holding_registers(0, 2).await.unwrap().unwrap();
        assert_eq!(words, vec![225, 450]);
        ctx.disconnect().await.unwrap();

        let _ = stop_tx.send(());
        let _ = server.await;
        std::fs::remove_dir_all(&dir).ok();
    }
}
