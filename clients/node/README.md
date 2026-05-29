# Ferrum Streaming Control Technology™ Client for Node.js

This is the official Node.js IPC client for the Ferrum Streaming Control Technology™ Host driver (`fsct-driver`). It lets Node.js applications register a player, push playback state and metadata, and observe FSCT-compatible audio devices connected to the host machine.

Unlike a native binding, this package does **not** talk to USB devices directly. It connects over a local IPC transport (Unix domain socket on Linux/macOS, named pipe on Windows) to a running `fsct-driver` service, which owns the USB layer.

## Features

- Pure TypeScript/JavaScript — no native build step on install
- Promise-based API
- Works on Linux, macOS, and Windows (Node.js 18+)
- TypeScript type definitions included
- Event-based device add/remove notifications
- Automatic IPC protocol version negotiation

## Requirements

You need the `fsct-driver` service running on the same machine. Prebuilt installers for Windows (MSI), macOS (pkg) and Linux (Debian `.deb`) are published as release assets on the project's GitHub repository:

https://github.com/HEM-RnD/fsct-host/releases

After installing the driver, the IPC endpoint becomes available at the platform-default location:

| Platform | Default endpoint |
|----------|------------------|
| Linux    | `/run/fsct/fsct.sock` |
| macOS    | `/var/run/fsct/fsct.sock` |
| Windows  | `\\.\pipe\fsct_driver` |

## Installation

```bash
npm install @hemspzoo/fsct-client
```

## Quick Start

```ts
import { FsctIpcClient } from '@hemspzoo/fsct-client';

const client = await FsctIpcClient.connect();

client.on('deviceChanged', (e) => {
  console.log(`device ${e.event}: ${e.deviceId}`);
});

const playerId = await client.registerPlayer('com.example.my-player');

const devices = await client.getDetectedDevices();
if (devices.length > 0) {
  await client.assignPlayerToDevice(playerId, devices[0]);
}

await client.updatePlayerState(playerId, {
  status: 'playing',
  timeline: {
    positionMs: 0,
    // Omit updateMonoNs to anchor at "now" (age 0). If you sampled the position earlier, pass
    // client.monoNowNs() captured at that moment instead.
    durationMs: 240_000,
    rate: 1.0,
  },
  texts: {
    title: 'Song Title',
    artist: 'Artist Name',
    album: 'Album Name',
    genre: null,
  },
});

// ... later, on shutdown:
await client.unregisterPlayer(playerId);
client.disconnect();
```

To connect to a non-default endpoint (for example a test driver running under a different path), use `FsctIpcClient.connectToEndpoint(path)`.

## API Overview

- **Connection** — `FsctIpcClient.connect()`, `FsctIpcClient.connectToEndpoint(path)`, `disconnect()`, `getProtocolVersion()`
- **Player lifecycle** — `registerPlayer(selfId)`, `unregisterPlayer(id)`, `assignPlayerToDevice(playerId, deviceId)`, `unassignPlayerFromDevice(playerId, deviceId)`
- **State updates** — `updatePlayerState`, `updatePlayerStatus`, `updatePlayerTimeline`, `updatePlayerMetadata`
- **Device queries** — `getDetectedDevices()`, `getDeviceInfo(deviceId)`, `getPlayerAssignedDevice(playerId)`
- **Events** — `'deviceChanged'`, `'error'`, `'close'`

Full type definitions ship with the package (`DeviceId`, `PlayerId`, `FsctStatus`, `FsctTextMetadata`, `TimelineInfo`, `TrackMetadata`, `PlayerState`, `DeviceInfo`, `ProtocolVersion`, `DeviceChangeEvent`).

## Protocol Reference

The IPC protocol is JSON-RPC 2.0 framed as NDJSON. The authoritative specification — including version compatibility rules — is maintained in [`docs/ipc.md`](https://github.com/HEM-RnD/fsct-host/blob/main/docs/ipc.md) in the project repository.

## License

This package is licensed under the Apache License, Version 2.0. You may not use this file except in compliance with the License. You can obtain a copy of the License at:

http://www.apache.org/licenses/LICENSE-2.0

In addition, this package implements Ferrum Streaming Control Technology™ (FSCT), which is subject to the Ferrum Streaming Control Technology™ License, Version 1.0. All rights to FSCT are reserved by HEM Sp. z o.o.

## Attribution and Trademark

Ferrum Streaming Control Technology™ is a trademark of HEM Sp. z o.o. All rights reserved. Please refer to the LICENSE-FSCT.md file for full details on the usage and attribution requirements for FSCT.

## Disclaimer

This package is provided "as-is" without warranties or conditions of any kind, either express or implied. For more information, refer to the LICENSE file.
