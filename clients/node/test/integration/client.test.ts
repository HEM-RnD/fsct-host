import { describe, it, expect, beforeEach, afterEach } from 'vitest';
import { FsctIpcClient, FsctError } from '../../src/client.js';
import type { DeviceChangeEvent } from '../../src/types.js';
import { startTestServer, serverBin } from './helpers.js';
import type { TestServer } from './helpers.js';

const DEVICE_ID_1 = '11111111-0000-0000-0000-000000000001';
const DEVICE_ID_2 = '22222222-0000-0000-0000-000000000002';

describe.skipIf(!serverBin)('FsctIpcClient integration', () => {
  let server: TestServer;
  let client: FsctIpcClient;

  beforeEach(async () => {
    server = await startTestServer();
    client = await FsctIpcClient.connectToEndpoint(server.socketPath);
  });

  afterEach(() => {
    client.disconnect();
    server.kill();
  });

  it('handshake returns protocol version major 1', () => {
    const version = client.getProtocolVersion();
    expect(version.major).toBe(1);
    expect(typeof version.minor).toBe('number');
  });

  it('register and unregister player', async () => {
    const playerId = await client.registerPlayer('test-player');
    expect(typeof playerId).toBe('number');
    expect(playerId).toBeGreaterThan(0);
    await expect(client.unregisterPlayer(playerId)).resolves.toBeUndefined();
  });

  it('assign and unassign player to device', async () => {
    const playerId = await client.registerPlayer('assign-test');
    await expect(client.assignPlayerToDevice(playerId, DEVICE_ID_1)).resolves.toBeUndefined();
    expect(await client.getPlayerAssignedDevice(playerId)).toBe(DEVICE_ID_1);
    await expect(client.unassignPlayerFromDevice(playerId, DEVICE_ID_1)).resolves.toBeUndefined();
    expect(await client.getPlayerAssignedDevice(playerId)).toBeNull();
  });

  it('update player status resolves for all statuses', async () => {
    const playerId = await client.registerPlayer('status-test');
    await expect(client.updatePlayerStatus(playerId, 'playing')).resolves.toBeUndefined();
    await expect(client.updatePlayerStatus(playerId, 'paused')).resolves.toBeUndefined();
    await expect(client.updatePlayerStatus(playerId, 'stopped')).resolves.toBeUndefined();
  });

  it('update player timeline with value and null', async () => {
    const playerId = await client.registerPlayer('timeline-test');
    await expect(
      client.updatePlayerTimeline(playerId, {
        positionMs: 30_000,
        updateUnixMs: Date.now(),
        durationMs: 240_000,
        rate: 1.0,
      }),
    ).resolves.toBeUndefined();
    await expect(client.updatePlayerTimeline(playerId, null)).resolves.toBeUndefined();
  });

  it('update player metadata for all text slots', async () => {
    const playerId = await client.registerPlayer('metadata-test');
    await expect(client.updatePlayerMetadata(playerId, 'current_title', 'My Song')).resolves.toBeUndefined();
    await expect(client.updatePlayerMetadata(playerId, 'current_author', 'My Artist')).resolves.toBeUndefined();
    await expect(client.updatePlayerMetadata(playerId, 'current_album', 'My Album')).resolves.toBeUndefined();
    await expect(client.updatePlayerMetadata(playerId, 'current_genre', 'Jazz')).resolves.toBeUndefined();
    await expect(client.updatePlayerMetadata(playerId, 'current_title', null)).resolves.toBeUndefined();
  });

  it('update full player state', async () => {
    const playerId = await client.registerPlayer('state-test');
    await expect(
      client.updatePlayerState(playerId, {
        status: 'playing',
        timeline: { positionMs: 1000, updateUnixMs: Date.now(), durationMs: 5000, rate: 1.0 },
        texts: { title: 'Track', artist: 'Artist', album: null, genre: null },
      }),
    ).resolves.toBeUndefined();
  });

  it('get player assigned device returns null before assignment', async () => {
    const playerId = await client.registerPlayer('unassigned');
    expect(await client.getPlayerAssignedDevice(playerId)).toBeNull();
  });

  it('get detected devices returns two fixed UUIDs', async () => {
    const devices = await client.getDetectedDevices();
    expect(devices).toHaveLength(2);
    expect(devices).toContain(DEVICE_ID_1);
    expect(devices).toContain(DEVICE_ID_2);
  });

  it('get device info returns correct camelCase fields', async () => {
    const info1 = await client.getDeviceInfo(DEVICE_ID_1);
    expect(info1.id).toBe(DEVICE_ID_1);
    expect(info1.name).toBe('FSCT Test Device 1');
    expect(info1.manufacturer).toBe('HEM Sp. z o.o.');
    expect(info1.vendorId).toBe(0x1234);
    expect(info1.productId).toBe(0x0001);
    expect(info1.serialNumber).toBe('SN-TEST-001');

    const info2 = await client.getDeviceInfo(DEVICE_ID_2);
    expect(info2.id).toBe(DEVICE_ID_2);
    expect(info2.productId).toBe(0x0002);
    expect(info2.serialNumber).toBeNull();
  });

  it('emits deviceChanged notification from server', async () => {
    // The mock emits Added(DEVICE_ID_1) 200ms after the first subscribe_device_changes call.
    // startTestServer's probe connection triggered that first call, so the timer is already running.
    const received: DeviceChangeEvent[] = [];
    client.on('deviceChanged', (e: DeviceChangeEvent) => received.push(e));

    await new Promise<void>((r) => setTimeout(r, 500));

    expect(received).toHaveLength(1);
    expect(received[0]).toEqual({ event: 'added', deviceId: DEVICE_ID_1 });
  });

  it('rejects with FsctError(-32000) for unknown device', async () => {
    const unknownId = '00000000-0000-0000-0000-000000000000';
    await expect(client.getDeviceInfo(unknownId)).rejects.toSatisfy(
      (e) => e instanceof FsctError && e.code === -32000,
    );
  });

  it('handles five concurrent requests all resolving', async () => {
    const playerId = await client.registerPlayer('concurrent');
    const results = await Promise.all(
      Array.from({ length: 5 }, () => client.updatePlayerStatus(playerId, 'playing')),
    );
    expect(results).toHaveLength(5);
  });

  it('reconnect after disconnect succeeds', async () => {
    client.disconnect();
    client = await FsctIpcClient.connectToEndpoint(server.socketPath);
    expect(client.getProtocolVersion().major).toBe(1);
  });
});
