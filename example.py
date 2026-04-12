"""
Emulador de dispositivo Modbus TCP para pruebas de integración.
- Simula un sensor de temperatura que actualiza su valor cada segundo.
- Configurable por variables de entorno:
    - DEVICE_NAME: Nombre del dispositivo (default: "Generic_Sensor")
  - UPDATE_INTERVAL: Intervalo de actualización en segundos (default: 1.0)
    - DEVICE_ID: ID Modbus del dispositivo (default: 1)
Uso:
1. Construir la imagen Docker:
   docker build -t modbus-emulator .
2. Ejecutar el contenedor:
    docker run -d --name modbus_emulator -p 502:502 \
      -e DEVICE_NAME="Temp_Sensor_01" \
      -e UPDATE_INTERVAL=1.0 \
            -e DEVICE_ID=1 \
      modbus-emulator
3. Conectar un cliente Modbus TCP a la IP del host en el puerto 502 para leer el registro 40001.
"""
import os
import time
import threading
import random

from pymodbus import ModbusDeviceIdentification
from pymodbus.server import StartTcpServer
from pymodbus.datastore import ModbusSequentialDataBlock, ModbusDeviceContext, ModbusServerContext

# Configuración por variables de entorno (inyectadas por Docker)
DEVICE_NAME = os.getenv("DEVICE_NAME", "Generic_Sensor")
UPDATE_INTERVAL = float(os.getenv("UPDATE_INTERVAL", "1.0"))
DEVICE_ID = int(os.getenv("DEVICE_ID", "1"))
MODBUS_PORT = int(os.getenv("MODBUS_PORT", "502"))


def update_sensors(context):
    """Simula el comportamiento físico del sensor"""
    register_address = 0x00  # Registro 40001

    while True:
        # Aquí puedes meter lógica según el tipo de dispositivo
        # Por ahora, simulamos una temperatura que fluctúa
        val = int(220 + random.uniform(-5, 5))  # 22.0°C aprox

        context[DEVICE_ID].setValues(3, register_address, [val])
        print(f"[{DEVICE_NAME}] Registro 40001 actualizado a: {val}")
        time.sleep(UPDATE_INTERVAL)


def run_server():
    # Store de datos: Coils, Discrete Inputs, Holding Registers, Input Registers
    # Usamos ModbusSequentialDataBlock para inicializar con ceros
    store = ModbusDeviceContext(
        hr=ModbusSequentialDataBlock(0, [0]*100)
    )
    context = ModbusServerContext(devices=store, single=True)

    identity = ModbusDeviceIdentification(
        info_name={
            "VendorName": "OBSIDIA",
            "ProductName": DEVICE_NAME,
            "ModelName": "Modbus TCP Emulator",
            "MajorMinorRevision": "1.0",
        }
    )

    # Hilo para simular datos en tiempo real
    thread = threading.Thread(target=update_sensors,
                              args=(context,), daemon=True)
    thread.start()

    print(f"Iniciando emulador: {DEVICE_NAME} en puerto {MODBUS_PORT}...")
    StartTcpServer(context=context, identity=identity,
                   address=("0.0.0.0", MODBUS_PORT))


if __name__ == "__main__":
    run_server()
