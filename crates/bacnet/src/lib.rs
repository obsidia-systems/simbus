//! BACnet/IP server backed by a simbus device.
//!
//! Objects and instances come from YAML `export`. See `docs/bacnet.md`.

use std::io;
use std::net::Ipv4Addr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use bacnet_objects::analog::{AnalogInputObject, AnalogOutputObject, AnalogValueObject};
use bacnet_objects::binary::{BinaryInputObject, BinaryOutputObject, BinaryValueObject};
use bacnet_objects::database::ObjectDatabase;
use bacnet_objects::device::{DeviceConfig, DeviceObject};
use bacnet_objects::traits::BACnetObject;
use bacnet_server::server::BACnetServer;
use bacnet_types::enums::{ObjectType, PropertyIdentifier};
use bacnet_types::primitives::{ObjectIdentifier, PropertyValue};
use engine::Device;
use spec::{BacnetExportEntry, BacnetObjectType, BindingSpec, PointKind};
use tokio::sync::RwLock;
use tracing::info;

/// ASHRAE EngineeringUnits `no-units`.
const NO_UNITS: u32 = 95;

/// Listen for BACnet/IP on `0.0.0.0` until `shutdown` resolves.
pub async fn serve(
    device: Arc<Device>,
    port: u16,
    device_instance: u32,
    ready: Arc<AtomicBool>,
    shutdown: impl std::future::Future<Output = ()> + Send + 'static,
) -> io::Result<()> {
    serve_on(
        device,
        Ipv4Addr::UNSPECIFIED,
        port,
        Ipv4Addr::BROADCAST,
        device_instance,
        ready,
        shutdown,
    )
    .await
}

/// Same as [`serve`] with an explicit interface and broadcast address.
///
/// The runtime always binds `0.0.0.0`; tests bind loopback so they do not
/// depend on enumerating host interfaces.
pub async fn serve_on(
    device: Arc<Device>,
    interface: Ipv4Addr,
    port: u16,
    broadcast: Ipv4Addr,
    device_instance: u32,
    ready: Arc<AtomicBool>,
    shutdown: impl std::future::Future<Output = ()> + Send + 'static,
) -> io::Result<()> {
    let db = build_database(&device, device_instance).map_err(io::Error::other)?;
    let mut server = BACnetServer::bip_builder()
        .database(db)
        .interface(interface)
        .port(port)
        .broadcast_address(broadcast)
        .build()
        .await
        .map_err(|err| io::Error::other(err.to_string()))?;

    ready.store(true, Ordering::SeqCst);
    info!(port, "bacnet listening");

    let sync_db = server.database().clone();
    let sync_device = device.clone();
    let mut ticks = tokio::time::interval(Duration::from_millis(100));
    let mut stop = std::pin::pin!(shutdown);
    loop {
        tokio::select! {
            () = &mut stop => break,
            _ = ticks.tick() => {
                sync_bank(&sync_device, &sync_db).await;
            }
        }
    }

    ready.store(false, Ordering::SeqCst);
    server
        .stop()
        .await
        .map_err(|err| io::Error::other(err.to_string()))
}

fn build_database(device: &Device, device_instance: u32) -> Result<ObjectDatabase, String> {
    let spec = device.spec();
    let mut db = ObjectDatabase::new();
    let mut cfg = DeviceConfig {
        instance: device_instance,
        name: spec.name.clone(),
        ..DeviceConfig::default()
    };
    if !spec.identity.vendor.is_empty() {
        cfg.vendor_name = spec.identity.vendor.clone();
    }
    if !spec.identity.product.is_empty() {
        cfg.model_name = spec.identity.product.clone();
    }
    if !spec.identity.revision.is_empty() {
        cfg.firmware_revision = spec.identity.revision.clone();
    }
    let mut device_obj = DeviceObject::new(cfg).map_err(|err| err.to_string())?;
    device_obj.set_description(spec.description.clone());
    db.add(Box::new(device_obj))
        .map_err(|err| err.to_string())?;

    for (id, entry) in bacnet_export(spec) {
        let Some(point) = spec.point(&id) else {
            continue;
        };
        let mut object = make_object(entry.object, entry.instance, &id, &point.description)?;
        seed_object(object.as_mut(), entry.object, point.kind, device, &id);
        db.add(object).map_err(|err| err.to_string())?;
    }
    Ok(db)
}

fn bacnet_export(spec: &spec::DeviceSpec) -> Vec<(String, BacnetExportEntry)> {
    for binding in &spec.bindings {
        if let BindingSpec::BacnetIp { export, .. } = binding {
            return export
                .iter()
                .map(|(id, entry)| (id.clone(), entry.clone()))
                .collect();
        }
    }
    Vec::new()
}

fn make_object(
    object: BacnetObjectType,
    instance: u32,
    name: &str,
    description: &str,
) -> Result<Box<dyn BACnetObject>, String> {
    match object {
        BacnetObjectType::AnalogInput => {
            let mut obj =
                AnalogInputObject::new(instance, name, NO_UNITS).map_err(|err| err.to_string())?;
            obj.set_description(description.to_owned());
            Ok(Box::new(obj))
        }
        BacnetObjectType::AnalogValue => {
            let mut obj =
                AnalogValueObject::new(instance, name, NO_UNITS).map_err(|err| err.to_string())?;
            obj.set_description(description.to_owned());
            Ok(Box::new(obj))
        }
        BacnetObjectType::AnalogOutput => {
            let mut obj =
                AnalogOutputObject::new(instance, name, NO_UNITS).map_err(|err| err.to_string())?;
            obj.set_description(description.to_owned());
            Ok(Box::new(obj))
        }
        BacnetObjectType::BinaryInput => {
            let mut obj = BinaryInputObject::new(instance, name).map_err(|err| err.to_string())?;
            obj.set_description(description.to_owned());
            Ok(Box::new(obj))
        }
        BacnetObjectType::BinaryValue => {
            let mut obj = BinaryValueObject::new(instance, name).map_err(|err| err.to_string())?;
            obj.set_description(description.to_owned());
            Ok(Box::new(obj))
        }
        BacnetObjectType::BinaryOutput => {
            let mut obj = BinaryOutputObject::new(instance, name).map_err(|err| err.to_string())?;
            obj.set_description(description.to_owned());
            Ok(Box::new(obj))
        }
    }
}

fn seed_object(
    object: &mut dyn BACnetObject,
    object_type: BacnetObjectType,
    kind: PointKind,
    device: &Device,
    id: &str,
) {
    let Some(view) = device.point_view(id) else {
        return;
    };
    match kind {
        PointKind::Analog => {
            if let Some(value) = view.analog {
                apply_value(object, object_type, PropertyValue::Real(value as f32));
            }
        }
        PointKind::Binary => {
            if let Some(value) = view.binary {
                apply_value(
                    object,
                    object_type,
                    PropertyValue::Enumerated(u32::from(value)),
                );
            }
        }
    }
}

fn apply_value(object: &mut dyn BACnetObject, object_type: BacnetObjectType, value: PropertyValue) {
    if matches!(
        object_type,
        BacnetObjectType::AnalogInput | BacnetObjectType::BinaryInput
    ) {
        let _ = object.set_present_value_internal(value);
    } else {
        let _ = object.write_property(PropertyIdentifier::PRESENT_VALUE, None, value, None);
    }
}

async fn sync_bank(device: &Device, db: &RwLock<ObjectDatabase>) {
    let spec = device.spec();
    let export = bacnet_export(spec);
    {
        let guard = db.read().await;
        for (id, entry) in &export {
            if !writable(entry.object) {
                continue;
            }
            let Some(point) = spec.point(id) else {
                continue;
            };
            let Ok(oid) = object_id(entry.object, entry.instance) else {
                continue;
            };
            let Some(object) = guard.get(&oid) else {
                continue;
            };
            let Ok(pv) = object.read_property(PropertyIdentifier::PRESENT_VALUE, None) else {
                continue;
            };
            match point.kind {
                PointKind::Analog => {
                    if let Some(real) = property_real(&pv) {
                        let current = device.point_view(id).and_then(|v| v.analog);
                        if current.is_none_or(|c| (c - real).abs() > 1e-4) {
                            let _ = device.override_point(id, Some(real), None, "bacnet");
                        }
                    }
                }
                PointKind::Binary => {
                    if let Some(bit) = property_bool(&pv) {
                        let current = device.point_view(id).and_then(|v| v.binary);
                        if current != Some(bit) {
                            let _ = device.override_point(id, None, Some(bit), "bacnet");
                        }
                    }
                }
            }
        }
    }
    {
        let mut guard = db.write().await;
        for (id, entry) in &export {
            let Some(point) = spec.point(id) else {
                continue;
            };
            let Ok(oid) = object_id(entry.object, entry.instance) else {
                continue;
            };
            let Some(object) = guard.get_mut(&oid) else {
                continue;
            };
            seed_object(object.as_mut(), entry.object, point.kind, device, id);
        }
    }
}

fn writable(object: BacnetObjectType) -> bool {
    matches!(
        object,
        BacnetObjectType::AnalogValue
            | BacnetObjectType::AnalogOutput
            | BacnetObjectType::BinaryValue
            | BacnetObjectType::BinaryOutput
    )
}

fn object_id(object: BacnetObjectType, instance: u32) -> Result<ObjectIdentifier, String> {
    let ty = match object {
        BacnetObjectType::AnalogInput => ObjectType::ANALOG_INPUT,
        BacnetObjectType::AnalogValue => ObjectType::ANALOG_VALUE,
        BacnetObjectType::AnalogOutput => ObjectType::ANALOG_OUTPUT,
        BacnetObjectType::BinaryInput => ObjectType::BINARY_INPUT,
        BacnetObjectType::BinaryValue => ObjectType::BINARY_VALUE,
        BacnetObjectType::BinaryOutput => ObjectType::BINARY_OUTPUT,
    };
    ObjectIdentifier::new(ty, instance).map_err(|err| err.to_string())
}

fn property_real(value: &PropertyValue) -> Option<f64> {
    match value {
        PropertyValue::Real(n) => Some(f64::from(*n)),
        PropertyValue::Double(n) => Some(*n),
        _ => None,
    }
}

fn property_bool(value: &PropertyValue) -> Option<bool> {
    match value {
        PropertyValue::Boolean(b) => Some(*b),
        PropertyValue::Enumerated(n) => Some(*n != 0),
        PropertyValue::Unsigned(n) => Some(*n != 0),
        _ => None,
    }
}
