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

/**
 * Process-global monotonic epoch, captured once at module load. Monotonic timestamps are
 * expressed as milliseconds since this epoch (a process-local, jump-free reference), mirroring the
 * Rust core's `EPOCH`. Milliseconds keep every value well within 2^53, so plain `number`
 * arithmetic is precise. The epoch differs per process, so a client's frame is bridged to the
 * driver's via the connect-time time-sync handshake (see {@link monoOffsetMs}).
 */
const NODE_EPOCH = process.hrtime.bigint();

/** Current monotonic time as milliseconds since {@link NODE_EPOCH}. */
function monoNowMs(): number {
  return Number((process.hrtime.bigint() - NODE_EPOCH) / 1_000_000n);
}

/** Maximum tolerated disagreement (ms) between two handshake offset samples before retrying. */
const OFFSET_CONSISTENCY_TOLERANCE_MS = 5;
/** How many times to retry the two-sample handshake before giving up. */
const OFFSET_HANDSHAKE_ATTEMPTS = 5;

/** A back-to-back wall + monotonic sample in this process's frame. */
function sampleClientTime(): { wallMs: number; monoMs: number } {
  const wallMs = Date.now();
  const monoMs = monoNowMs();
  return { wallMs, monoMs };
}

/**
 * NTP/PTP-style offset (ms) converting a *client-frame* monotonic stamp into the *driver-frame*:
 * `driver = client + offset`. IPC latency cancels (the `wallC - wallD` term corrects for which
 * wall instant each side sampled); only a wall-clock step between the two samples corrupts it,
 * which the two-sample consistency check detects.
 */
function monoOffsetMs(wallD: number, monoD: number, wallC: number, monoC: number): number {
  return monoD - monoC + (wallC - wallD);
}

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
      const onConnect = () => {
        socket.off('error', onError);
        resolve(socket);
      };
      const onError = (err: Error) => {
        socket.off('connect', onConnect);
        socket.destroy();
        reject(err);
      };
      socket.once('connect', onConnect);
      socket.once('error', onError);
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
  update_mono_ms: number;
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

// Converts the timeline's client-frame monotonic anchor into the driver's frame using the
// handshake offset, so the driver compares it directly against its own clock. An absent
// `updateMonoMs` is anchored at "now" (age 0).
function timelineToWire(t: TimelineInfo, offsetMs: number): TimelineWire {
  const clientMs = t.updateMonoMs !== undefined ? Math.trunc(t.updateMonoMs) : monoNowMs();
  return {
    position_ms: t.positionMs,
    update_mono_ms: clientMs + offsetMs,
    duration_ms: t.durationMs,
    rate: t.rate,
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

function playerStateToWire(s: PlayerState, offsetMs: number): PlayerStateWire {
  return {
    status: s.status,
    timeline: s.timeline ? timelineToWire(s.timeline, offsetMs) : null,
    texts: s.texts,
  };
}

/**
 * IPC client for the FSCT driver daemon.
 *
 * Emits:
 * - 'deviceChanged' (DeviceChangeEvent) — device added/removed notifications
 * - 'error' (Error) — connection errors; attach a listener to avoid Node.js
 *   treating emitted errors as uncaught exceptions
 * - 'close' () — connection closed
 */
export class FsctIpcClient extends EventEmitter {
  private constructor(
    private readonly mux: Mux,
    public readonly negotiatedVersion: ProtocolVersion,
    /** Offset (ms) converting this client's monotonic frame to the driver's: `driver = client + offset`. */
    private readonly monoOffsetMs: number,
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
    let monoOffsetMs: number;
    try {
      negotiatedVersion = await FsctIpcClient.performHandshake(mux);
      monoOffsetMs = await FsctIpcClient.establishMonoOffset(mux);
    } catch (err) {
      socket.destroy();
      throw err;
    }

    return new FsctIpcClient(mux, negotiatedVersion, monoOffsetMs);
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

  /**
   * Establish the monotonic-frame offset to the driver via a mini NTP/PTP handshake. Each
   * round-trip fetches the driver's `(wall, mono)` sample and pairs it with a local one taken
   * right after the response arrives; two round-trips are compared, and if they agree (no wall
   * step in between) the averaged offset is returned, otherwise we retry. Runs once per
   * connection, so a driver restart (new monotonic epoch) is re-bridged on reconnect.
   */
  private static async establishMonoOffset(mux: Mux): Promise<number> {
    let last: [number, number] | null = null;
    for (let i = 0; i < OFFSET_HANDSHAKE_ATTEMPTS; i++) {
      const k1 = await FsctIpcClient.measureOffsetOnce(mux);
      const k2 = await FsctIpcClient.measureOffsetOnce(mux);
      if (Math.abs(k1 - k2) <= OFFSET_CONSISTENCY_TOLERANCE_MS) {
        return Math.trunc((k1 + k2) / 2);
      }
      last = [k1, k2];
    }
    throw new Error(
      `time-sync handshake did not stabilize after ${OFFSET_HANDSHAKE_ATTEMPTS} attempts ` +
        `(last samples: ${last}); the wall clock may be stepping repeatedly`,
    );
  }

  private static async measureOffsetOnce(mux: Mux): Promise<number> {
    const driver = await mux.call<{ wall_ms: number; mono_ms: number }>('get_timesync', {});
    const client = sampleClientTime();
    return monoOffsetMs(
      Math.trunc(driver.wall_ms),
      Math.trunc(driver.mono_ms),
      client.wallMs,
      client.monoMs,
    );
  }

  /** Returns the protocol version negotiated during connection (no RPC call). */
  getProtocolVersion(): ProtocolVersion {
    return this.negotiatedVersion;
  }

  /**
   * Current monotonic time in milliseconds, in this client's frame. Pass the returned value as a
   * {@link TimelineInfo.updateMonoMs} to stamp when a playback position was sampled.
   */
  monoNowMs(): number {
    return monoNowMs();
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
      state: playerStateToWire(state, this.monoOffsetMs),
    });
  }

  async updatePlayerStatus(playerId: PlayerId, status: FsctStatus): Promise<void> {
    await this.mux.call<null>('update_player_status', { player_id: playerId, status });
  }

  async updatePlayerTimeline(playerId: PlayerId, timeline: TimelineInfo | null): Promise<void> {
    await this.mux.call<null>('update_player_timeline', {
      player_id: playerId,
      timeline: timeline ? timelineToWire(timeline, this.monoOffsetMs) : null,
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
    return result;
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
