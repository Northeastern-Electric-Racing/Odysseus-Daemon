# Odysseus-Daemon
System state daemon for the Odysseus on-car telemetry OS

This handles a variety of functions on the car, including security guarrantees, etc.
These functions are considered modules.

Core principles:
- Each module should not bring down another (do not unwrap or expect, return error)
- Each module should use tracing when needed
- Each module should have an enable flag at a minimum, and be off by default
- Each module has documentation at the top of the file


Modules
- `visual`: Camera process manager and writer.  Status: Alpha
- `lockdown`: Feature disabler and modifier upon HV enablement.  Status: Alpha
- `audible`: Call feature trigger and monitor.  Status: Alpha
- `numerical`: Telemetry scraper and sender (tpu-telemetry python replacement).  Status: Beta
- `net`: Network statistics telemetry scraper and sender for both TPU and Base Station. Status: Beta
- `halow`: Hardware statistics telemetry scraper and sender for both TPU and Base Station. Status: Beta
- `logger`: MQTT receiver and disk logger. Status: Beta
- `color`: The wheel LED controller and API system. Status: Alpha
- `daq`: The Jack DAQ serial scraper. Status: Beta
- `can`: Diagnostics for the CANbus interface.  Status: Beta
- `gps`: Data scraper for the GPS. Status: Beta
- `sys_parser`: Read the SYS subsystem to understand mosquitto diagnostics. Status: Beta

Upload modules:
- `logger`: Upload from the logger module to scylla. Status: Beta
- `visual`: Camera video uploader to cloud platform. Status: Incomplete
- `serial`: (from `lockdown` module) Serial output uploader to cloud platform. Status Incomplete
- 


**This program will only run on Odysseus**
