import { describe, it, expect, beforeEach, afterEach } from 'vitest';
import * as net from 'node:net';
import { Mux } from '../../src/mux.js';
import { FsctError, MAX_LINE_BYTES } from '../../src/protocol.js';
import type { DeviceChangeEvent } from '../../src/types.js';

// Creates an in-process connected socket pair via a loopback TCP server.
async function createSocketPair(): Promise<{ client: net.Socket; server: net.Socket }> {
  return new Promise((resolve, reject) => {
    let clientSocket: net.Socket;
    const srv = net.createServer((serverSocket) => {
      srv.close();
      resolve({ client: clientSocket, server: serverSocket });
    });
    srv.listen(0, '127.0.0.1', () => {
      const addr = srv.address() as net.AddressInfo;
      clientSocket = net.createConnection({ host: '127.0.0.1', port: addr.port });
      clientSocket.once('error', reject);
    });
    srv.once('error', reject);
  });
}

/** Write a JSON-RPC line to the server side of the pair. */
function writeResponse(socket: net.Socket, data: object): void {
  socket.write(JSON.stringify(data) + '\n');
}

describe('Mux', () => {
  let clientSocket: net.Socket;
  let serverSocket: net.Socket;
  let mux: Mux;

  beforeEach(async () => {
    ({ client: clientSocket, server: serverSocket } = await createSocketPair());
    mux = new Mux(clientSocket);
  });

  afterEach(() => {
    clientSocket.destroy();
    serverSocket.destroy();
  });

  it('resolves a single call when a matching response arrives', async () => {
    const promise = mux.call<number>('ping', {});
    // Give the mux a moment to write the request, then respond.
    await new Promise<void>((r) => setImmediate(r));
    writeResponse(serverSocket, { jsonrpc: '2.0', id: 1, result: 42 });
    expect(await promise).toBe(42);
  });

  it('resolves concurrent calls matched by ID in any order', async () => {
    const p1 = mux.call<string>('m1', {});
    const p2 = mux.call<string>('m2', {});
    const p3 = mux.call<string>('m3', {});

    await new Promise<void>((r) => setImmediate(r));

    // Answer out of order: 3, 1, 2
    writeResponse(serverSocket, { jsonrpc: '2.0', id: 3, result: 'three' });
    writeResponse(serverSocket, { jsonrpc: '2.0', id: 1, result: 'one' });
    writeResponse(serverSocket, { jsonrpc: '2.0', id: 2, result: 'two' });

    expect(await Promise.all([p1, p2, p3])).toEqual(['one', 'two', 'three']);
  });

  it('rejects with FsctError when the server returns an error response', async () => {
    const promise = mux.call<unknown>('bad', {});
    await new Promise<void>((r) => setImmediate(r));
    writeResponse(serverSocket, { jsonrpc: '2.0', id: 1, error: { code: -32000, message: 'app error' } });

    await expect(promise).rejects.toSatisfy((e) => e instanceof FsctError && e.code === -32000 && e.message === 'app error');
  });

  it('emits deviceChanged for a device_changed notification', async () => {
    const received: DeviceChangeEvent[] = [];
    mux.on('deviceChanged', (e: DeviceChangeEvent) => received.push(e));

    writeResponse(serverSocket, {
      jsonrpc: '2.0',
      method: 'device_changed',
      params: { event: 'added', device_id: 'aabbccdd-0000-0000-0000-000000000001' },
    });

    await new Promise<void>((r) => setTimeout(r, 20));
    expect(received).toHaveLength(1);
    expect(received[0]).toEqual({ event: 'added', deviceId: 'aabbccdd-0000-0000-0000-000000000001' });
  });

  it('does not crash on an unknown notification method', async () => {
    mux.on('error', () => {});
    writeResponse(serverSocket, { jsonrpc: '2.0', method: 'unknown_notif', params: {} });
    await new Promise<void>((r) => setTimeout(r, 20));
    // No error thrown; mux still works.
    const promise = mux.call<number>('ping', {});
    await new Promise<void>((r) => setImmediate(r));
    writeResponse(serverSocket, { jsonrpc: '2.0', id: 1, result: 1 });
    expect(await promise).toBe(1);
  });

  it('rejects all pending calls when the socket closes', async () => {
    mux.on('error', () => {});
    const p1 = mux.call<unknown>('slow1', {});
    const p2 = mux.call<unknown>('slow2', {});
    await new Promise<void>((r) => setImmediate(r));

    serverSocket.destroy();

    await expect(p1).rejects.toThrow();
    await expect(p2).rejects.toThrow();
  });

  it('does not crash on a malformed (non-JSON) line', async () => {
    mux.on('error', () => {});
    serverSocket.write('not-json\n');
    await new Promise<void>((r) => setTimeout(r, 20));

    // Mux should still work for subsequent valid responses.
    const promise = mux.call<number>('ok', {});
    await new Promise<void>((r) => setImmediate(r));
    writeResponse(serverSocket, { jsonrpc: '2.0', id: 1, result: 99 });
    expect(await promise).toBe(99);
  });

  it('rejects pending calls when an incoming line exceeds the byte limit', async () => {
    const promise = mux.call<unknown>('slow', {});
    await new Promise<void>((r) => setImmediate(r));

    const oversizedByBytes = '€'.repeat(Math.floor(MAX_LINE_BYTES / 3) + 1);
    serverSocket.write(oversizedByBytes + '\n');

    await expect(promise).rejects.toThrow('IPC message exceeds maximum line size');
  });

  it('rejects pending calls on a parse-error response without an ID', async () => {
    const promise = mux.call<unknown>('bad_json', {});
    await new Promise<void>((r) => setImmediate(r));
    writeResponse(serverSocket, { jsonrpc: '2.0', id: null, error: { code: -32700, message: 'parse error' } });

    await expect(promise).rejects.toSatisfy((e) => e instanceof FsctError && e.code === -32700);
  });

  it('ignores a response with an unknown ID', async () => {
    writeResponse(serverSocket, { jsonrpc: '2.0', id: 999, result: 'orphan' });
    await new Promise<void>((r) => setTimeout(r, 20));
    // No crash; mux still functional.
    const promise = mux.call<number>('ping', {});
    await new Promise<void>((r) => setImmediate(r));
    writeResponse(serverSocket, { jsonrpc: '2.0', id: 1, result: 7 });
    expect(await promise).toBe(7);
  });

  it('rejects immediately when destroyed', async () => {
    mux.destroy();
    await expect(mux.call<unknown>('any', {})).rejects.toThrow('IPC connection closed');
  });
});
