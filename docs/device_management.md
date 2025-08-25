# FSCT Device Management Architecture

This document describes the architecture of the device management components in the FSCT Host library.

## Overview

The device management system is responsible for discovering, initializing, and managing FSCT-compatible USB devices. The system has been refactored into two main components:

1. **Device Manager** - Responsible for device ID management, storing device mappings, and providing a unified API for device operations.
2. **USB Device Watch** - Responsible for listening to USB events, initializing devices, and registering them with the Device Manager.

This separation of concerns allows for better modularity, testability, and maintainability of the codebase.

## Component Responsibilities

### FsctDevice

`FsctDevice` is a core component that represents a physical FSCT-compatible USB device. Its responsibilities include:

- Initializing the device with appropriate descriptors
- Managing device state (enabled/disabled)
- Handling time synchronization between the host and device
- Providing methods for setting device status, progress, and text
- Encoding text data in the appropriate format (UTF-8 or UCS-2) for the device
- Converting between Rust types and device-specific data structures
- Providing a unified API for device operations

`FsctDevice` directly interacts with `FsctUsbInterface` to send and receive data from the physical device.

### FsctUsbInterface

`FsctUsbInterface` is responsible for low-level USB communication with FSCT devices. Its responsibilities include:

- Sending and receiving USB control transfers
- Implementing the FSCT USB protocol
- Handling device-specific error conditions
- Converting between Rust low-level data structures and raw USB data

This component abstracts away the details of USB communication, allowing higher-level components to work with more convenient Rust types.

### Device Manager

The `DeviceManager` component is responsible for:

- Assigning unique IDs to FSCT devices
- Maintaining a mapping between USB device IDs and managed device IDs
- Storing references to active FSCT devices
- Providing a unified API for device operations that wraps `FsctDevice`
- Implementing the `DeviceManagement` and `DeviceControl` traits 
- Notifying listeners when fsct-capable devices are added or removed

The Device Manager acts as a facade for device operations, allowing clients to work with device IDs rather than directly with `FsctDevice` instances.

### USB Device Watch

The `UsbDeviceWatch` component is responsible for:

- Listening for USB device connection and disconnection events
- Initializing new FSCT devices when they are connected
- Registering devices with the Device Manager
- Handling device initialization retries and error conditions

This component handles the dynamic nature of USB devices, ensuring that the system can respond appropriately when devices are connected or disconnected.

## Interaction Flow

1. The `UsbDeviceWatch` listens for USB device events.
2. When a device is connected, `UsbDeviceWatch` attempts to initialize it as an FSCT device.
3. If initialization is successful, the device is registered with the `DeviceManager`, which assigns it a unique ID.
4. Clients interact with devices through the `DeviceManager` using the assigned IDs.
5. The `DeviceManager` forwards operations to the appropriate `FsctDevice` instance.
6. The `FsctDevice` uses its `FsctUsbInterface` to communicate with the physical device.
7. When a device is disconnected, `UsbDeviceWatch` notifies the `DeviceManager` to remove it.

## API Usage Example

The following example demonstrates how to use the device management API:

```
// Create a device manager
let device_manager = Arc::new(DeviceManager::new());

// Start watching for USB devices
let device_watch_handle = run_usb_device_watch(device_manager.clone(), None).await?;

// Later, interact with a device using its managed ID
if let Some(device_id) = get_device_id_from_somewhere() {
    // Set device status
    device_manager.set_status(device_id, FsctStatus::Playing).await?;
    
    // Set device text
    device_manager.set_current_text(device_id, FsctTextMetadata::Title, Some("Song Title")).await?;
    
    // Set playback progress
    let progress = TimelineInfo {
        position: Duration::from_secs(30),
        duration: Duration::from_secs(180),
        rate: 1.0,
        update_time: SystemTime::now(),
    };
    device_manager.set_progress(device_id, Some(progress)).await?;
}

// Shutdown device watching when done
device_watch_handle.shutdown().await?;
```

## Error Handling

The system uses Rust's `Result` type for error handling. Errors from the USB layer are propagated up through the stack, with appropriate context added at each level. The `DeviceManager` adds an additional error case for when a device with a given ID is not found.

## Thread Safety

Both the `DeviceManager` and `UsbDeviceWatch` components are designed to be thread-safe, using `Arc` and `Mutex` for shared state. This allows them to be used safely in asynchronous contexts with Tokio.

## Future Considerations

- Adding support for bulk transfers for larger data payloads
- Adding support for device filtering based on vendor/product IDs
