# FSCT IPC Protocol Reference

This document is the **authoritative specification** of the FSCT IPC protocol v1.0. 

---

## Table of Contents

1. [Overview](#overview)
2. [Transport](#transport)
3. [Framing](#framing)
4. [Message Format (JSON-RPC 2.0)](#message-format-json-rpc-20)
5. [Connection Lifecycle](#connection-lifecycle)
6. [Data Types](#data-types)
7. [Error Codes](#error-codes)
8. [Methods](#methods)
   - [get_protocol_version](#get_protocol_version)
   - [get_timesync](#get_timesync)
   - [register_player](#register_player)
   - [unregister_player](#unregister_player)
   - [assign_player_to_device](#assign_player_to_device)
   - [unassign_player_from_device](#unassign_player_from_device)
   - [update_player_state](#update_player_state)
   - [update_player_status](#update_player_status)
   - [update_player_timeline](#update_player_timeline)
   - [update_player_metadata](#update_player_metadata)
   - [get_player_assigned_device](#get_player_assigned_device)
   - [get_detected_devices](#get_detected_devices)
   - [get_device_info](#get_device_info)
9. [Driver Notifications](#driver-notifications)
   - [device_changed](#device_changed)
10. [Security and Ownership](#security-and-ownership)

---

## Overview

The FSCT driver (`fsctd`) exposes its API over a local IPC channel. Clients connect to a platform-specific
endpoint (Unix domain socket or Windows named pipe) and communicate using **JSON-RPC 2.0** messages framed as
**newline-delimited JSON (NDJSON)**.

The protocol is designed for user-service processes (e.g., media player integrations) that need to report
playback state and receive device change notifications.

---

## Transport

The IPC endpoint address depends on the host platform:

| Platform | Transport         | Default path                     |
|----------|-------------------|----------------------------------|
| Linux    | Unix domain socket | `/run/fsct/fsct.sock`            |
| macOS    | Unix domain socket | `/var/run/fsct/fsct.sock`        |
| Windows  | Named pipe        | `\\.\pipe\fsct_driver`           |

On Windows, the client should open the named pipe in read/write mode. If the pipe is busy (error
`ERROR_PIPE_BUSY`), retry for up to ~5 seconds with short delays.

On Unix, connect to the socket path using a standard Unix domain stream socket.

---

## Framing

All messages are exchanged as **NDJSON**: each message is a single-line JSON object followed by a newline
character (`\n`, `0x0A`). Carriage returns (`\r`) must not be used.

**Constraints:**
- Maximum message size: **1 MiB** (1,048,576 bytes), including the terminating newline.
- Messages exceeding this limit must be rejected.
- Both directions (client → driver, driver → client) use the same framing.

---

## Message Format (JSON-RPC 2.0)

The protocol follows [JSON-RPC 2.0](https://www.jsonrpc.org/specification). Three message types are used:

### Request (client → driver)

```json
{
  "jsonrpc": "2.0",
  "id": <integer>,
  "method": "<method_name>",
  "params": { ... }
}
```

- `id`: a client-chosen positive integer used to match responses. Must be unique within pending requests.
- `params`: a JSON object (never an array). May be `{}` for methods with no parameters.

### Response (driver → client)

Success:
```json
{
  "jsonrpc": "2.0",
  "id": <integer>,
  "result": <value>
}
```

Error:
```json
{
  "jsonrpc": "2.0",
  "id": <integer>,
  "error": {
    "code": <integer>,
    "message": "<human-readable description>"
  }
}
```

- `id` always mirrors the request `id`.
- On success, `error` is absent; on error, `result` is absent.

### Notification (driver → client)

Sent asynchronously by the driver without a corresponding request:
```json
{
  "jsonrpc": "2.0",
  "method": "<notification_name>",
  "params": { ... }
}
```

Notifications have **no `id` field** and require no response from the client.

---

## Connection Lifecycle

1. **Connect** to the endpoint.
2. **Handshake**: call [`get_protocol_version`](#get_protocol_version) immediately after connecting.
   - Compare the returned `major` version to the expected value. If major versions differ, close the connection —
     the protocol is incompatible.
   - Minor version differences are backwards-compatible (the driver may support more features).
3. **Time-sync**: call [`get_timesync`](#get_timesync) to establish the monotonic-frame offset used for
   [`TimelineInfo`](#timelineinfo-object) timestamps. Re-run this whenever you reconnect (a driver restart
   resets its monotonic epoch).
4. **Operate**: send method calls; handle responses and notifications concurrently.
5. **Disconnect**: the client may close the connection at any time. The driver will automatically unregister
   all players that were registered on that connection.

**Current protocol version:** `1.0`

---

## Data Types

### `PlayerId` (integer)

A non-zero unsigned 32-bit integer assigned by the driver when a player is registered. Scoped to the current
connection — not valid across reconnections.

```
valid range: 1 .. 4294967295
```

### `DeviceId` (UUID string)

A UUID that uniquely identifies a physical device. Computed deterministically from the device's USB VID, PID,
and serial number. Represented as a lowercase hyphenated string:

```
"550e8400-e29b-41d4-a716-446655440000"
```

### `FsctStatus` (string)

Playback status. One of the following string values:

| Value        | Meaning                                                |
|--------------|--------------------------------------------------------|
| `"stopped"`  | Playback is not active.                                |
| `"playing"`  | Playback is in progress.                               |
| `"paused"`   | Playback is paused and can be resumed.                 |
| `"seeking"`  | Seeking (fast-forward / rewind) in progress.           |
| `"buffering"`| Temporarily halted due to data loading.                |
| `"error"`    | An error occurred; playback cannot proceed.            |
| `"unknown"`  | Playback state could not be determined.                |

### `FsctTextMetadata` (string)

Identifies a text metadata slot. One of:

| Value             | Meaning                            |
|-------------------|------------------------------------|
| `"current_title"` | Title of the currently playing track |
| `"current_author"`| Artist/author of the current track |
| `"current_album"` | Album of the current track         |
| `"current_genre"` | Genre of the current track         |

### `TimelineInfo` (object)

Represents the playback timeline at a specific point in time. The `position_ms` was valid at `update_mono_ns`
and advances at `rate` milliseconds per real millisecond.

```json
{
  "position_ms": 45000,
  "update_mono_ns": 90000000000,
  "duration_ms": 240000,
  "rate": 1.0
}
```

| Field           | Type   | Description                                                              |
|-----------------|--------|--------------------------------------------------------------------------|
| `position_ms`   | u64    | Playback position in milliseconds at the time of `update_mono_ns`.       |
| `update_mono_ns`| i64    | Monotonic timestamp (nanoseconds) when `position_ms` was measured, expressed in the **driver's** monotonic frame (see [`get_timesync`](#get_timesync)). The client converts its own monotonic stamp into the driver's frame before sending. |
| `duration_ms`   | u64    | Total track duration in milliseconds.                                    |
| `rate`          | f64    | Playback rate relative to real time. `1.0` = normal speed, `0.0` = paused. |

> **Why monotonic, not wall-clock?** Playback position is a relative track offset, so the anchor only needs a
> jump-free clock — never the calendar time. Anchoring to the monotonic clock means a wall-clock step (e.g. an
> NTP sync on a device with no RTC) does not corrupt the extrapolated position. The two processes' monotonic
> clocks share the same source but differ by a per-boot constant, which is reconciled once at connect via
> [`get_timesync`](#get_timesync).

### `TrackMetadata` (object)

Text metadata for a track. All fields are optional (nullable).

```json
{
  "title": "Bohemian Rhapsody",
  "artist": "Queen",
  "album": "A Night at the Opera",
  "genre": "Rock"
}
```

| Field    | Type           | Description        |
|----------|----------------|--------------------|
| `title`  | string \| null | Track title        |
| `artist` | string \| null | Artist / author    |
| `album`  | string \| null | Album name         |
| `genre`  | string \| null | Genre              |

### `PlayerState` (object)

Full player state snapshot.

```json
{
  "status": "playing",
  "timeline": {
    "position_ms": 45000,
    "update_mono_ns": 90000000000,
    "duration_ms": 240000,
    "rate": 1.0
  },
  "texts": {
    "title": "Bohemian Rhapsody",
    "artist": "Queen",
    "album": null,
    "genre": null
  }
}
```

| Field      | Type                      | Description                                  |
|------------|---------------------------|----------------------------------------------|
| `status`   | `FsctStatus`              | Current playback status.                     |
| `timeline` | `TimelineInfo` \| `null`  | Timeline information; `null` when unavailable. |
| `texts`    | `TrackMetadata`           | Current track metadata.                      |

### `DeviceInfo` (object)

Information about a detected FSCT-compatible USB device.

```json
{
  "id": "550e8400-e29b-41d4-a716-446655440000",
  "name": "FSCT Audio DAC",
  "manufacturer": "HEM Sp. z o.o.",
  "vendor_id": 5824,
  "product_id": 1155,
  "serial_number": "HEM-00001"
}
```

| Field           | Type           | Description                                               |
|-----------------|----------------|-----------------------------------------------------------|
| `id`            | UUID string    | Unique device identifier (see [`DeviceId`](#deviceid-uuid-string)). |
| `name`          | string \| null | USB product string.                                       |
| `manufacturer`  | string \| null | USB manufacturer string.                                  |
| `vendor_id`     | integer        | USB Vendor ID (VID).                                      |
| `product_id`    | integer        | USB Product ID (PID).                                     |
| `serial_number` | string \| null | USB serial number string.                                 |

### `ProtocolVersion` (object)

```json
{ "major": 1, "minor": 0 }
```

---

## Error Codes

| Code    | Name              | Meaning                                                             |
|---------|-------------------|---------------------------------------------------------------------|
| -32700  | Parse error       | The line could not be parsed as valid JSON.                         |
| -32600  | Invalid request   | The JSON was valid but is not a well-formed JSON-RPC 2.0 request.  |
| -32601  | Method not found  | The requested method does not exist.                                |
| -32602  | Invalid params    | The parameters are missing or have the wrong type/value.           |
| -32000  | Application error | A driver-level error occurred (e.g., device not found, player ownership violation). The `message` field contains details. |

---

## Methods

All methods follow the same request/response pattern. `params` is always a JSON object.

---

### `get_protocol_version`

Returns the driver's protocol version. **Must be called first** after connecting; close the connection if
`major` does not match your expected value.

**Params:** `{}`

**Returns:** [`ProtocolVersion`](#protocolversion-object)

**Example:**

```json
→ {"jsonrpc":"2.0","id":1,"method":"get_protocol_version","params":{}}
← {"jsonrpc":"2.0","id":1,"result":{"major":1,"minor":0}}
```

---

### `get_timesync`

Returns a back-to-back sample of the driver's wall-clock and monotonic clocks, used to bridge the client's
monotonic frame to the driver's. **Should be called right after [`get_protocol_version`](#get_protocol_version)**
(and again on every reconnect) to compute the offset applied to [`TimelineInfo`](#timelineinfo-object)
`update_mono_ns` values.

**Params:** `{}`

**Returns:** an object with:

| Field     | Type | Description                                                                |
|-----------|------|----------------------------------------------------------------------------|
| `wall_ns` | u64  | Wall-clock time (nanoseconds since the Unix epoch), sampled with `mono_ns`. |
| `mono_ns` | u64  | Monotonic time (nanoseconds since the driver process's epoch), sampled with `wall_ns`. |

**Computing the offset.** Sample your own `(wall_c, mono_c)` immediately after the response arrives, then:

```
offset = (mono_d - mono_c) + (wall_c - wall_d)      // driver_frame = client_frame + offset
```

IPC latency cancels out (the `wall_c - wall_d` term corrects for which wall instant each side sampled), so the
offset is stable for the life of the connection regardless of later wall-clock steps. Only a wall-clock step
*during* the handshake itself can corrupt it, so perform the exchange **twice** and require the two offsets to
agree within a small tolerance (a few milliseconds); retry otherwise. Add this offset to a timeline's
client-frame `update_mono_ns` before sending. A driver restart yields a new monotonic epoch, so the handshake
must be re-run on reconnect.

**Example:**

```json
→ {"jsonrpc":"2.0","id":2,"method":"get_timesync","params":{}}
← {"jsonrpc":"2.0","id":2,"result":{"wall_ns":1746700000000000000,"mono_ns":90000000000}}
```

---

### `register_player`

Registers a new media player with the driver. Returns a `PlayerId` that must be used in all subsequent
player-related calls.

A player represents a single media player application or session. Multiple players can be registered on
one connection.

**Params:**

| Field     | Type   | Required | Description                                                           |
|-----------|--------|----------|-----------------------------------------------------------------------|
| `self_id` | string | Yes      | A human-readable identifier for this player (e.g., application name or D-Bus name). Used for logging; not required to be unique. |

**Returns:** integer (`PlayerId`)

**Example:**

```json
→ {"jsonrpc":"2.0","id":2,"method":"register_player","params":{"self_id":"my-music-app"}}
← {"jsonrpc":"2.0","id":2,"result":1}
```

---

### `unregister_player`

Unregisters a previously registered player. All device assignments for this player are removed. This is
optional — players are automatically unregistered when the connection closes.

**Params:**

| Field       | Type    | Required | Description                    |
|-------------|---------|----------|--------------------------------|
| `player_id` | integer | Yes      | The `PlayerId` to unregister.  |

**Returns:** `null`

**Example:**

```json
→ {"jsonrpc":"2.0","id":3,"method":"unregister_player","params":{"player_id":1}}
← {"jsonrpc":"2.0","id":3,"result":null}
```

---

### `assign_player_to_device`

Assigns a player to a specific FSCT device. The device will start showing the player's metadata. A player
can be assigned to at most one device at a time; a new assignment replaces the previous one.

**Params:**

| Field       | Type        | Required | Description                              |
|-------------|-------------|----------|------------------------------------------|
| `player_id` | integer     | Yes      | The `PlayerId` to assign.                |
| `device_id` | UUID string | Yes      | The `DeviceId` of the target device.     |

**Returns:** `null`

**Example:**

```json
→ {"jsonrpc":"2.0","id":4,"method":"assign_player_to_device","params":{"player_id":1,"device_id":"550e8400-e29b-41d4-a716-446655440000"}}
← {"jsonrpc":"2.0","id":4,"result":null}
```

---

### `unassign_player_from_device`

Removes the assignment between a player and a device.

**Params:**

| Field       | Type        | Required | Description                                  |
|-------------|-------------|----------|----------------------------------------------|
| `player_id` | integer     | Yes      | The `PlayerId` to unassign.                  |
| `device_id` | UUID string | Yes      | The `DeviceId` to remove the assignment from. |

**Returns:** `null`

**Example:**

```json
→ {"jsonrpc":"2.0","id":5,"method":"unassign_player_from_device","params":{"player_id":1,"device_id":"550e8400-e29b-41d4-a716-446655440000"}}
← {"jsonrpc":"2.0","id":5,"result":null}
```

---

### `update_player_state`

Updates the complete player state in a single call. This is a convenience method equivalent to calling
`update_player_status`, `update_player_timeline`, and `update_player_metadata` for all text slots.

**Params:**

| Field       | Type          | Required | Description                              |
|-------------|---------------|----------|------------------------------------------|
| `player_id` | integer       | Yes      | The `PlayerId` to update.                |
| `state`     | `PlayerState` | Yes      | The new complete player state.           |

**Returns:** `null`

**Example:**

```json
→ {
    "jsonrpc": "2.0",
    "id": 6,
    "method": "update_player_state",
    "params": {
      "player_id": 1,
      "state": {
        "status": "playing",
        "timeline": {
          "position_ms": 45000,
          "update_mono_ns": 90000000000,
          "duration_ms": 240000,
          "rate": 1.0
        },
        "texts": {
          "title": "Bohemian Rhapsody",
          "artist": "Queen",
          "album": "A Night at the Opera",
          "genre": null
        }
      }
    }
  }
← {"jsonrpc":"2.0","id":6,"result":null}
```

---

### `update_player_status`

Updates only the playback status of a player.

**Params:**

| Field       | Type         | Required | Description                   |
|-------------|--------------|----------|-------------------------------|
| `player_id` | integer      | Yes      | The `PlayerId` to update.     |
| `status`    | `FsctStatus` | Yes      | The new playback status.      |

**Returns:** `null`

**Example:**

```json
→ {"jsonrpc":"2.0","id":7,"method":"update_player_status","params":{"player_id":1,"status":"paused"}}
← {"jsonrpc":"2.0","id":7,"result":null}
```

---

### `update_player_timeline`

Updates only the timeline (position/duration/rate) for a player. Pass `null` to clear the timeline
(e.g., when the track ends or duration is unknown).

**Params:**

| Field       | Type                     | Required | Description                        |
|-------------|--------------------------|----------|------------------------------------|
| `player_id` | integer                  | Yes      | The `PlayerId` to update.          |
| `timeline`  | `TimelineInfo` \| `null` | Yes      | The new timeline, or `null`.       |

**Returns:** `null`

**Example — set timeline:**

```json
→ {
    "jsonrpc": "2.0",
    "id": 8,
    "method": "update_player_timeline",
    "params": {
      "player_id": 1,
      "timeline": {
        "position_ms": 90000,
        "update_mono_ns": 135000000000,
        "duration_ms": 240000,
        "rate": 1.0
      }
    }
  }
← {"jsonrpc":"2.0","id":8,"result":null}
```

**Example — clear timeline:**

```json
→ {"jsonrpc":"2.0","id":9,"method":"update_player_timeline","params":{"player_id":1,"timeline":null}}
← {"jsonrpc":"2.0","id":9,"result":null}
```

---

### `update_player_metadata`

Updates a single text metadata slot for a player. Pass `null` for `text` to clear the slot.

**Params:**

| Field         | Type                  | Required | Description                                           |
|---------------|-----------------------|----------|-------------------------------------------------------|
| `player_id`   | integer               | Yes      | The `PlayerId` to update.                             |
| `metadata_id` | `FsctTextMetadata`    | Yes      | Which metadata slot to update.                        |
| `text`        | string \| `null`      | Yes      | The new text value, or `null` to clear the slot.      |

**Returns:** `null`

**Example:**

```json
→ {"jsonrpc":"2.0","id":10,"method":"update_player_metadata","params":{"player_id":1,"metadata_id":"current_title","text":"Stairway to Heaven"}}
← {"jsonrpc":"2.0","id":10,"result":null}
```

**Example — clear slot:**

```json
→ {"jsonrpc":"2.0","id":11,"method":"update_player_metadata","params":{"player_id":1,"metadata_id":"current_genre","text":null}}
← {"jsonrpc":"2.0","id":11,"result":null}
```

---

### `get_player_assigned_device`

Returns the `DeviceId` currently assigned to a player, or `null` if no device is assigned.

**Params:**

| Field       | Type    | Required | Description               |
|-------------|---------|----------|---------------------------|
| `player_id` | integer | Yes      | The `PlayerId` to query.  |

**Returns:** UUID string \| `null`

**Example:**

```json
→ {"jsonrpc":"2.0","id":12,"method":"get_player_assigned_device","params":{"player_id":1}}
← {"jsonrpc":"2.0","id":12,"result":"550e8400-e29b-41d4-a716-446655440000"}
```

---

### `get_detected_devices`

Returns the list of all currently detected FSCT-compatible USB devices.

**Params:** `{}`

**Returns:** array of UUID strings

**Example:**

```json
→ {"jsonrpc":"2.0","id":13,"method":"get_detected_devices","params":{}}
← {"jsonrpc":"2.0","id":13,"result":["550e8400-e29b-41d4-a716-446655440000","6ba7b810-9dad-11d1-80b4-00c04fd430c8"]}
```

---

### `get_device_info`

Returns detailed information about a specific device.

**Params:**

| Field       | Type        | Required | Description                           |
|-------------|-------------|----------|---------------------------------------|
| `device_id` | UUID string | Yes      | The `DeviceId` of the device to query. |

**Returns:** [`DeviceInfo`](#deviceinfo-object)

**Example:**

```json
→ {"jsonrpc":"2.0","id":14,"method":"get_device_info","params":{"device_id":"550e8400-e29b-41d4-a716-446655440000"}}
← {
    "jsonrpc": "2.0",
    "id": 14,
    "result": {
      "id": "550e8400-e29b-41d4-a716-446655440000",
      "name": "FSCT Audio DAC",
      "manufacturer": "HEM Sp. z o.o.",
      "vendor_id": 3336,
      "product_id": 1155,
      "serial_number": "HEM123456"
    }
  }
```

---

## Driver Notifications

Notifications are sent asynchronously by the driver. They have no `id` field and require no response.
Clients should process them concurrently with waiting for RPC responses.

---

### `device_changed`

Sent whenever a FSCT-compatible USB device is connected or disconnected.

**Params:**

| Field       | Type        | Description                                              |
|-------------|-------------|----------------------------------------------------------|
| `event`     | string      | `"added"` when a device is connected, `"removed"` when disconnected. |
| `device_id` | UUID string | The `DeviceId` of the affected device.                   |

**Example — device connected:**

```json
← {"jsonrpc":"2.0","method":"device_changed","params":{"event":"added","device_id":"550e8400-e29b-41d4-a716-446655440000"}}
```

**Example — device disconnected:**

```json
← {"jsonrpc":"2.0","method":"device_changed","params":{"event":"removed","device_id":"550e8400-e29b-41d4-a716-446655440000"}}
```

---

## Security and Ownership

- **Player ownership is scoped to the connection.** A player registered by connection A cannot be
  manipulated (updated, assigned, unregistered) by connection B. Attempting to do so returns an
  application error (`-32000`).
- **Automatic cleanup on disconnect.** When a client disconnects (cleanly or abruptly), the driver
  automatically unregisters all players that were registered on that connection, removing their device
  assignments.
- **The IPC socket/pipe is local-only.** No network exposure. Access control is governed by OS-level
  file/pipe permissions on the socket path or named pipe.
