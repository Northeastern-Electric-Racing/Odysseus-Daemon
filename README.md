# Odysseus-Daemon
System state daemon for the Odysseus on-car telemetry OS

This handles a variety of functions on the car, including security guarrantees, etc.
These functions are considered modules.

Core principles:
- Each module should not bring down another (do not unwrap or expect, return error)
- Each module should use tracing when needed
- Each module should have an enable flag at a minimum, and be off by default


Modules
- `visual`: Camera process manager and writer.  Status: Beta
- `lockdown`: Feature disabler and modifier upon HV enablement.  Status: Alpha
- `audible`: Call feature trigger and monitor.  Status: Alpha
- `numerical`: Telemetry scraper and sender (tpu-telemetry python replacement).  Status: Incomplete
- `logger`: MQTT receiver and disk logger. Status: Beta


**This program will only run on Odysseus**
