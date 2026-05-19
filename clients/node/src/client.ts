// Copyright 2025 HEM Sp. z o.o.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.
//
// This file is part of an implementation of Ferrum Streaming Control Technology™,
// which is subject to additional terms found in the LICENSE-FSCT.md file.

import { EventEmitter } from 'node:events';
import * as net from 'node:net';
import { EXPECTED_PROTOCOL_MAJOR } from './protocol.js';
import { Mux } from './mux.js';
import type {
  DeviceChangeEvent,
  DeviceId,
  DeviceInfo,
  FsctStatus,
  FsctTextMetadata,
  PlayerId,
  PlayerState,
  ProtocolVersion,
  TimelineInfo,
} from './types.js';

export { FsctError } from './protocol.js';

const PIPE_RETRY_INTERVAL_MS = 50;
const PIPE_RETRY_TIMEOUT_MS = 5000;

function defaultEndpointPath(): string {
  switch (process.platform) {
    case 'linux':
      return '/run/fsct/fsct.sock';
    case 'darwin':
      return '/var/run/fsct/fsct.sock';
    case 'win32':
      return '\\\\.\\pipe\\fsct_driver';
    default:
      throw new Error(`Unsupported platform: ${process.platform}`);
  }
}

async function openSocket(path: string): Promise<net.Socket> {
  const attempt = (): Promise<net.Socket> =>
    new Promise((resolve, reject) => {
      const socket = net.createConnection({ path });
      socket.once('connect', () => resolve(socket));
      socket.once('error', reject);
    });

  if (process.platform !== 'win32') {
    return attempt();
  }

  // Windows named pipes can return EBUSY when all instances are in use.
  // Retry with 50ms intervals up to 5 seconds, matching the Rust client behaviour.
  const deadline = Date.now() + PIPE_RETRY_TIMEOUT_MS;
  while (true) {
    try {
      return await attempt();
    } catch (err) {
      const e = err as NodeJS.ErrnoException;
      if ((e.code === 'EBUSY' || e.code === 'ENOENT') && Date.now() < deadline) {
        await new Promise<void>((r) => setTimeout(r, PIPE_RETRY_INTERVAL_MS));
        continue;
      }
      throw err;
    }
  }
}

// Wire-format helpers for camelCase ↔ snake_case conversions.

interface TimelineWire {
  position_ms: number;
  update_unix_ms: number;
  duration_ms: number;
  rate: number;
}

interface DeviceInfoWire {
  id: string;
  name: string | null;
  manufacturer: string | null;
  vendor_id: number;
  product_id: number;
  serial_number: string | null;
}

interface PlayerStateWire {
  status: FsctStatus;
  timeline: TimelineWire | null;
  texts: { title: string | null; artist: string | null; album: string | null; genre: string | null };
}

function timelineToWire(t: TimelineInfo): TimelineWire {
  return {
    position_ms: t.positionMs,
    update_unix_ms: t.updateUnixMs,
    duration_ms: t.durationMs,
    rate: t.rate,
  };
}

function timelineFromWire(w: TimelineWire): TimelineInfo {
  return {
    positionMs: w.position_ms,
    updateUnixMs: w.update_unix_ms,
    durationMs: w.duration_ms,
    rate: w.rate,
  };
}

function deviceInfoFromWire(w: DeviceInfoWire): DeviceInfo {
  return {
    id: w.id,
    name: w.name,
    manufacturer: w.manufacturer,
    vendorId: w.vendor_id,
    productId: w.product_id,
    serialNumber: w.serial_number,
  };
}

function playerStateToWire(s: PlayerState): PlayerStateWire {
  return {
    status: s.status,
    timeline: s.timeline ? timelineToWire(s.timeline) : null,
    texts: s.texts,
  };
}

/**
 * IPC client for the FSCT driver daemon.
 *
 * Emits:
 * - 'deviceChanged' (DeviceChangeEvent) — device added/removed notifications
 * - 'error' (Error) — connection errors
 * - 'close' () — connection closed
 */
export class FsctIpcClient extends EventEmitter {
  private constructor(
    private readonly mux: Mux,
    public readonly negotiatedVersion: ProtocolVersion,
  ) {
    super();
    mux.on('deviceChanged', (event: DeviceChangeEvent) => this.emit('deviceChanged', event));
    mux.on('error', (err: Error) => this.emit('error', err));
    mux.on('close', () => this.emit('close'));
  }

  /** Connect to the platform-default IPC endpoint and perform handshake. */
  static async connect(): Promise<FsctIpcClient> {
    return FsctIpcClient.connectToEndpoint(defaultEndpointPath());
  }

  /** Connect to a specific socket path or named pipe and perform handshake. */
  static async connectToEndpoint(path: string): Promise<FsctIpcClient> {
    const socket = await openSocket(path);
    const mux = new Mux(socket);

    let negotiatedVersion: ProtocolVersion;
    try {
      negotiatedVersion = await FsctIpcClient.performHandshake(mux);
    } catch (err) {
      socket.destroy();
      throw err;
    }

    return new FsctIpcClient(mux, negotiatedVersion);
  }

  private static async performHandshake(mux: Mux): Promise<ProtocolVersion> {
    const resp = await mux.call<{ major: number; minor: number }>('get_protocol_version', {});
    if (resp.major !== EXPECTED_PROTOCOL_MAJOR) {
      throw new Error(
        `incompatible protocol version: remote ${resp.major}.${resp.minor}, expected major ${EXPECTED_PROTOCOL_MAJOR}`,
      );
    }
    return { major: resp.major, minor: resp.minor };
  }

  /** Returns the protocol version negotiated during connection (no RPC call). */
  getProtocolVersion(): ProtocolVersion {
    return this.negotiatedVersion;
  }

  /** Destroy the underlying socket. The server automatically unregisters all players. */
  disconnect(): void {
    this.mux.destroy();
  }

  // ---------------------------------------------------------------------------
  // Player management
  // ---------------------------------------------------------------------------

  async registerPlayer(selfId: string): Promise<PlayerId> {
    const result = await this.mux.call<number>('register_player', { self_id: selfId });
    if (typeof result !== 'number' || result <= 0 || !Number.isInteger(result)) {
      throw new Error(`register_player: unexpected response: ${JSON.stringify(result)}`);
    }
    return result;
  }

  async unregisterPlayer(playerId: PlayerId): Promise<void> {
    await this.mux.call<null>('unregister_player', { player_id: playerId });
  }

  async assignPlayerToDevice(playerId: PlayerId, deviceId: DeviceId): Promise<void> {
    await this.mux.call<null>('assign_player_to_device', { player_id: playerId, device_id: deviceId });
  }

  async unassignPlayerFromDevice(playerId: PlayerId, deviceId: DeviceId): Promise<void> {
    await this.mux.call<null>('unassign_player_from_device', { player_id: playerId, device_id: deviceId });
  }

  // ---------------------------------------------------------------------------
  // State updates
  // ---------------------------------------------------------------------------

  async updatePlayerState(playerId: PlayerId, state: PlayerState): Promise<void> {
    await this.mux.call<null>('update_player_state', {
      player_id: playerId,
      state: playerStateToWire(state),
    });
  }

  async updatePlayerStatus(playerId: PlayerId, status: FsctStatus): Promise<void> {
    await this.mux.call<null>('update_player_status', { player_id: playerId, status });
  }

  async updatePlayerTimeline(playerId: PlayerId, timeline: TimelineInfo | null): Promise<void> {
    await this.mux.call<null>('update_player_timeline', {
      player_id: playerId,
      timeline: timeline ? timelineToWire(timeline) : null,
    });
  }

  async updatePlayerMetadata(playerId: PlayerId, metadataId: FsctTextMetadata, text: string | null): Promise<void> {
    await this.mux.call<null>('update_player_metadata', {
      player_id: playerId,
      metadata_id: metadataId,
      text,
    });
  }

  // ---------------------------------------------------------------------------
  // Device queries
  // ---------------------------------------------------------------------------

  async getPlayerAssignedDevice(playerId: PlayerId): Promise<DeviceId | null> {
    const result = await this.mux.call<string | null>('get_player_assigned_device', { player_id: playerId });
    return result ?? null;
  }

  async getDetectedDevices(): Promise<DeviceId[]> {
    const result = await this.mux.call<string[]>('get_detected_devices', {});
    if (!Array.isArray(result)) {
      throw new Error(`get_detected_devices: expected array, got ${JSON.stringify(result)}`);
    }
    return result;
  }

  async getDeviceInfo(deviceId: DeviceId): Promise<DeviceInfo> {
    const result = await this.mux.call<DeviceInfoWire>('get_device_info', { device_id: deviceId });
    return deviceInfoFromWire(result);
  }
}

