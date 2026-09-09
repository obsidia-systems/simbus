//! OPC UA server backed by a simbus device.
//!
//! The address space is generated from the YAML register map. See `docs/opcua.md`.

use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use engine::Device;
use spec::{RegisterSpace, RegisterSpec};
use tracing::info;
use ua::crypto::SecurityPolicy;
use ua::server::address_space::{AddressSpace, VariableBuilder};
use ua::server::diagnostics::NamespaceMetadata;
use ua::server::node_manager::memory::{
    InMemoryNodeManager, SimpleNodeManager, SimpleNodeManagerImpl, simple_node_manager,
};
use ua::server::{ANONYMOUS_USER_TOKEN_ID, ServerBuilder};
use ua::types::{
    BuildInfo, DataTypeId, DataValue, DateTime, MessageSecurityMode, NodeId, StatusCode, Variant,
};

/// Listen for OPC UA until `shutdown` resolves.
pub async fn serve(
    device: Arc<Device>,
    port: u16,
    ready: Arc<AtomicBool>,
    shutdown: impl std::future::Future<Output = ()> + Send + 'static,
) -> io::Result<()> {
    let spec = device.spec();
    let ns_uri = format!("urn:simbus:{}", spec.device_type);
    let app_uri = "urn:simbus".to_owned();
    let app_name = spec.name.clone();
    let manufacturer = spec.identity.vendor.clone();
    let product = if spec.identity.product.is_empty() {
        spec.name.clone()
    } else {
        spec.identity.product.clone()
    };
    let software = spec.identity.revision.clone();

    let (server, handle) = ServerBuilder::new()
        .application_name(app_name)
        .application_uri(app_uri)
        .host("0.0.0.0")
        .port(port)
        .discovery_urls(vec![
            format!("opc.tcp://0.0.0.0:{port}/"),
            format!("opc.tcp://127.0.0.1:{port}/"),
        ])
        .pki_dir(PathBuf::from("/dev/null"))
        .build_info(BuildInfo {
            product_uri: ns_uri.clone().into(),
            manufacturer_name: manufacturer.into(),
            product_name: product.into(),
            software_version: software.into(),
            build_number: env!("CARGO_PKG_VERSION").into(),
            build_date: DateTime::now(),
        })
        .add_endpoint(
            "none",
            (
                "/",
                SecurityPolicy::None,
                MessageSecurityMode::None,
                &[ANONYMOUS_USER_TOKEN_ID] as &[&str],
            ),
        )
        .with_node_manager(simple_node_manager(
            NamespaceMetadata {
                namespace_uri: ns_uri.clone(),
                ..Default::default()
            },
            "simbus",
        ))
        .build()
        .map_err(|err| io::Error::other(err.to_string()))?;

    let node_manager = handle
        .node_managers()
        .get_of_type::<SimpleNodeManager>()
        .ok_or_else(|| io::Error::other("simple node manager missing"))?;
    let ns = handle
        .get_namespace_index(&ns_uri)
        .ok_or_else(|| io::Error::other("simbus namespace missing"))?;

    populate(device, ns, &node_manager);

    ready.store(true, Ordering::SeqCst);
    info!(port, "opcua listening");

    let handle_c = handle.clone();
    tokio::spawn(async move {
        shutdown.await;
        handle_c.cancel();
    });

    let run_result = server.run().await;
    ready.store(false, Ordering::SeqCst);
    run_result.map_err(|err| io::Error::other(err.to_string()))
}

fn populate(
    device: Arc<Device>,
    ns: u16,
    manager: &Arc<InMemoryNodeManager<SimpleNodeManagerImpl>>,
) {
    let spec = device.spec().clone();
    let holding_folder = NodeId::new(ns, "Holding");
    let input_folder = NodeId::new(ns, "Input");
    let coils_folder = NodeId::new(ns, "Coils");
    let discrete_folder = NodeId::new(ns, "Discrete");

    {
        let address_space = manager.address_space();
        let mut space = address_space.write();
        space.add_folder(
            &holding_folder,
            "Holding",
            "Holding",
            &NodeId::objects_folder_id(),
        );
        space.add_folder(
            &input_folder,
            "Input",
            "Input",
            &NodeId::objects_folder_id(),
        );
        space.add_folder(
            &coils_folder,
            "Coils",
            "Coils",
            &NodeId::objects_folder_id(),
        );
        space.add_folder(
            &discrete_folder,
            "Discrete",
            "Discrete",
            &NodeId::objects_folder_id(),
        );

        for reg in &spec.registers.holding {
            insert_numeric(&mut space, ns, "holding", reg, &holding_folder, true);
        }
        for reg in &spec.registers.input {
            insert_numeric(&mut space, ns, "input", reg, &input_folder, false);
        }
        for coil in &spec.registers.coils {
            insert_bit(&mut space, ns, "coils", &coil.name, &coils_folder, true);
        }
        for coil in &spec.registers.discrete {
            insert_bit(
                &mut space,
                ns,
                "discrete",
                &coil.name,
                &discrete_folder,
                false,
            );
        }
    }

    for reg in spec.registers.holding {
        wire_numeric(
            device.clone(),
            manager,
            ns,
            "holding",
            RegisterSpace::Holding,
            &reg,
            true,
        );
    }
    for reg in spec.registers.input {
        wire_numeric(
            device.clone(),
            manager,
            ns,
            "input",
            RegisterSpace::Input,
            &reg,
            false,
        );
    }
    for coil in spec.registers.coils {
        wire_bit(
            device.clone(),
            manager,
            ns,
            "coils",
            &coil.name,
            coil.address,
            true,
        );
    }
    for coil in spec.registers.discrete {
        wire_bit(
            device.clone(),
            manager,
            ns,
            "discrete",
            &coil.name,
            coil.address,
            false,
        );
    }
}

fn node_id(ns: u16, space: &str, name: &str) -> NodeId {
    NodeId::new(ns, format!("{space}/{name}"))
}

fn insert_numeric(
    space: &mut AddressSpace,
    ns: u16,
    space_name: &str,
    reg: &RegisterSpec,
    parent: &NodeId,
    writable: bool,
) {
    let id = node_id(ns, space_name, &reg.name);
    let mut builder = VariableBuilder::new(&id, &reg.name, &reg.name)
        .data_type(DataTypeId::Float)
        .value(0_f32)
        .organized_by(parent);
    if writable {
        builder = builder.writable();
    }
    if !builder.insert(space) {
        tracing::error!("failed to insert OPC UA variable");
    }
}

fn insert_bit(
    space: &mut AddressSpace,
    ns: u16,
    space_name: &str,
    name: &str,
    parent: &NodeId,
    writable: bool,
) {
    let id = node_id(ns, space_name, name);
    let mut builder = VariableBuilder::new(&id, name, name)
        .data_type(DataTypeId::Boolean)
        .value(false)
        .organized_by(parent);
    if writable {
        builder = builder.writable();
    }
    if !builder.insert(space) {
        tracing::error!("failed to insert OPC UA variable");
    }
}

fn wire_numeric(
    device: Arc<Device>,
    manager: &Arc<InMemoryNodeManager<SimpleNodeManagerImpl>>,
    ns: u16,
    space_name: &str,
    space: RegisterSpace,
    reg: &RegisterSpec,
    writable: bool,
) {
    let id = node_id(ns, space_name, &reg.name);
    let address = reg.address;
    let read_device = device.clone();
    manager
        .inner()
        .add_read_callback(id.clone(), move |_, _, _| {
            match read_device.register_real(space, address) {
                Some(real) => Ok(DataValue::new_now(real as f32)),
                None => Err(StatusCode::BadNodeIdUnknown),
            }
        });
    if writable {
        manager.inner().add_write_callback(id, move |value, _| {
            let Some(real) = variant_to_real(&value.value) else {
                return StatusCode::BadTypeMismatch;
            };
            match device.override_register(space, address, None, Some(real), "opcua") {
                Ok(_) => StatusCode::Good,
                Err(_) => StatusCode::BadUserAccessDenied,
            }
        });
    }
}

fn wire_bit(
    device: Arc<Device>,
    manager: &Arc<InMemoryNodeManager<SimpleNodeManagerImpl>>,
    ns: u16,
    space_name: &str,
    name: &str,
    address: u16,
    writable: bool,
) {
    let id = node_id(ns, space_name, name);
    let read_device = device.clone();
    let coil_space = space_name == "coils";
    manager
        .inner()
        .add_read_callback(id.clone(), move |_, _, _| {
            let bit = if coil_space {
                read_device
                    .read_coils(address, 1)
                    .ok()
                    .and_then(|v| v.into_iter().next())
            } else {
                read_device
                    .read_discrete(address, 1)
                    .ok()
                    .and_then(|v| v.into_iter().next())
            };
            match bit {
                Some(b) => Ok(DataValue::new_now(b)),
                None => Err(StatusCode::BadNodeIdUnknown),
            }
        });
    if writable {
        manager.inner().add_write_callback(id, move |value, _| {
            let Some(b) = variant_to_bool(&value.value) else {
                return StatusCode::BadTypeMismatch;
            };
            match device.override_coil(address, b) {
                Ok(_) => StatusCode::Good,
                Err(_) => StatusCode::BadUserAccessDenied,
            }
        });
    }
}

fn variant_to_real(value: &Option<Variant>) -> Option<f64> {
    match value {
        Some(Variant::Float(n)) => Some(f64::from(*n)),
        Some(Variant::Double(n)) => Some(*n),
        Some(Variant::UInt16(n)) => Some(f64::from(*n)),
        Some(Variant::Int16(n)) => Some(f64::from(*n)),
        Some(Variant::UInt32(n)) => Some(f64::from(*n)),
        Some(Variant::Int32(n)) => Some(f64::from(*n)),
        _ => None,
    }
}

fn variant_to_bool(value: &Option<Variant>) -> Option<bool> {
    match value {
        Some(Variant::Boolean(b)) => Some(*b),
        _ => None,
    }
}
