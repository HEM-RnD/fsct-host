import { describe, it, expect, beforeEach, afterEach } from 'vitest';
import { FsctIpcClient, FsctError } from '../../src/client.js';
import type { DeviceChangeEvent, PlayerState } from '../../src/types.js';
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

  it('update player status resolves and server receives correct value', async () => {
    const playerId = await client.registerPlayer('status-test');

    const [, event1] = await Promise.all([
      client.updatePlayerStatus(playerId, 'playing'),
      server.waitForReceived('update_player_status'),
    ]);
    expect(event1.playerId).toBe(playerId);
    expect(event1['status']).toBe('playing');

    const [, event2] = await Promise.all([
      client.updatePlayerStatus(playerId, 'paused'),
      server.waitForReceived('update_player_status'),
    ]);
    expect(event2['status']).toBe('paused');

    const [, event3] = await Promise.all([
      client.updatePlayerStatus(playerId, 'stopped'),
      server.waitForReceived('update_player_status'),
    ]);
    expect(event3['status']).toBe('stopped');
  });

  it('update player timeline and server receives snake_case fields', async () => {
    const playerId = await client.registerPlayer('timeline-test');

    const [, event1] = await Promise.all([
      client.updatePlayerTimeline(playerId, {
        positionMs: 30_000,
        updateUnixMs: Date.now(),
        durationMs: 240_000,
        rate: 1.0,
      }),
      server.waitForReceived('update_player_timeline'),
    ]);
    expect(event1.playerId).toBe(playerId);
    expect(event1['timeline']).toMatchObject({
      position_ms: 30_000,
      duration_ms: 240_000,
      rate: 1.0,
    });

    const [, event2] = await Promise.all([
      client.updatePlayerTimeline(playerId, null),
      server.waitForReceived('update_player_timeline'),
    ]);
    expect(event2['timeline']).toBeNull();
  });

  it('update player metadata and server receives correct slot and value', async () => {
    const playerId = await client.registerPlayer('metadata-test');

    async function checkMeta(metadataId: string, text: string | null) {
      const [, event] = await Promise.all([
        client.updatePlayerMetadata(playerId, metadataId as Parameters<typeof client.updatePlayerMetadata>[1], text),
        server.waitForReceived('update_player_metadata'),
      ]);
      expect(event.playerId).toBe(playerId);
      expect(event['metadataId']).toBe(metadataId);
      expect(event['text']).toBe(text);
    }

    await checkMeta('current_title', 'My Song');
    await checkMeta('current_author', 'My Artist');
    await checkMeta('current_album', 'My Album');
    await checkMeta('current_genre', 'Jazz');
    await checkMeta('current_title', null);
  });

  it('update full player state and server receives complete structure', async () => {
    const playerId = await client.registerPlayer('state-test');
    const state: PlayerState = {
      status: 'playing',
      timeline: { positionMs: 1000, updateUnixMs: Date.now(), durationMs: 5000, rate: 1.0 },
      texts: { title: 'Track', artist: 'Artist', album: null, genre: null },
    };

    const [, event] = await Promise.all([
      client.updatePlayerState(playerId, state),
      server.waitForReceived('update_player_state'),
    ]);
    expect(event.playerId).toBe(playerId);
    const received = event['state'] as Record<string, unknown>;
    expect(received['status']).toBe('playing');
    expect(received['timeline']).toMatchObject({
      position_ms: 1000,
      duration_ms: 5000,
      rate: 1.0,
    });
    expect(received['texts']).toEqual({
      title: 'Track',
      artist: 'Artist',
      album: null,
      genre: null,
    });
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

  it('emits deviceChanged notification triggered via stdin command', async () => {
    const eventPromise = new Promise<DeviceChangeEvent>((resolve) => {
      client.once('deviceChanged', (e: DeviceChangeEvent) => resolve(e));
    });

    server.emitDeviceChanged('added', DEVICE_ID_1);

    const event = await eventPromise;
    expect(event).toEqual({ event: 'added', deviceId: DEVICE_ID_1 });
  });

  it('emits deviceChanged removed notification', async () => {
    const eventPromise = new Promise<DeviceChangeEvent>((resolve) => {
      client.once('deviceChanged', (e: DeviceChangeEvent) => resolve(e));
    });

    server.emitDeviceChanged('removed', DEVICE_ID_2);

    const event = await eventPromise;
    expect(event).toEqual({ event: 'removed', deviceId: DEVICE_ID_2 });
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
