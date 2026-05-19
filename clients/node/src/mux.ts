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

// Internal module: NDJSON read loop, request/response multiplexer, notification dispatch.

import { EventEmitter } from 'node:events';
import { createInterface } from 'node:readline';
import type { Socket } from 'node:net';
import {
  FsctError,
  isRpcNotification,
  isRpcResponse,
  MAX_LINE_BYTES,
  type RpcRequest,
} from './protocol.js';
import type { DeviceChangeEvent } from './types.js';

interface PendingCall {
  resolve: (value: unknown) => void;
  reject: (reason: Error) => void;
}

/**
 * Multiplexes JSON-RPC 2.0 requests/responses over a single socket and dispatches
 * server-sent notifications as EventEmitter events.
 *
 * Emits:
 * - 'deviceChanged' (DeviceChangeEvent) — device_changed notifications from the server
 * - 'error' (Error) — forwarded socket errors
 * - 'close' () — socket closed
 */
export class Mux extends EventEmitter {
  private readonly pending = new Map<number, PendingCall>();
  private readonly rl: ReturnType<typeof createInterface>;
  private nextId = 1;
  #destroyed = false;

  constructor(private readonly socket: Socket) {
    super();
    socket.on('error', (err: Error) => this.onError(err));
    socket.on('close', () => this.onClose());
    this.rl = createInterface({ input: socket, crlfDelay: Infinity });
    // Node.js readline re-emits socket errors on the Interface; suppress to avoid
    // double-handling since the socket 'error' listener below already handles them.
    this.rl.on('error', () => {});
    this.rl.on('line', (line: string) => this.dispatchLine(line));
  }

  get destroyed(): boolean {
    return this.#destroyed;
  }

  /** Send an RPC call and return a Promise that resolves with the result value. */
  call<T>(method: string, params: Record<string, unknown>): Promise<T> {
    if (this.#destroyed) {
      return Promise.reject(new Error('IPC connection closed'));
    }

    return new Promise<T>((resolve, reject) => {
      const id = this.nextId++;
      this.pending.set(id, {
        resolve: resolve as (value: unknown) => void,
        reject,
      });

      const request: RpcRequest = { jsonrpc: '2.0', id, method, params };
      const line = JSON.stringify(request) + '\n';

      this.socket.write(line, (err) => {
        if (err) {
          const cb = this.pending.get(id);
          if (cb) {
            this.pending.delete(id);
            cb.reject(err);
          }
        }
      });
    });
  }

  private dispatchLine(line: string): void {
    if (line.length > MAX_LINE_BYTES) {
      this.failAll(new Error('IPC message exceeds maximum line size'));
      this.socket.destroy();
      return;
    }

    const trimmed = line.trim();
    if (!trimmed) return;

    let parsed: unknown;
    try {
      parsed = JSON.parse(trimmed);
    } catch {
      console.warn('[fsct-client] Received malformed JSON line, ignoring');
      return;
    }

    if (isRpcResponse(parsed)) {
      const id = typeof parsed.id === 'number' ? parsed.id : null;
      if (id === null) return;

      const cb = this.pending.get(id);
      if (!cb) return;
      this.pending.delete(id);

      if (parsed.error) {
        cb.reject(new FsctError(parsed.error.code, parsed.error.message));
      } else {
        cb.resolve(parsed.result ?? null);
      }
    } else if (isRpcNotification(parsed)) {
      if (parsed.method === 'device_changed') {
        this.dispatchDeviceChanged(parsed.params);
      } else {
        console.warn('[fsct-client] Unknown notification method:', parsed.method);
      }
    }
  }

  private dispatchDeviceChanged(params: unknown): void {
    if (typeof params !== 'object' || params === null) {
      console.warn('[fsct-client] Malformed device_changed notification: params not an object');
      return;
    }
    const p = params as Record<string, unknown>;
    const event = p['event'];
    const deviceId = p['device_id'];

    if ((event === 'added' || event === 'removed') && typeof deviceId === 'string') {
      const changeEvent: DeviceChangeEvent = { event, deviceId };
      this.emit('deviceChanged', changeEvent);
    } else {
      console.warn('[fsct-client] Malformed device_changed notification: invalid fields');
    }
  }

  private onError(err: Error): void {
    this.rl.close();
    this.socket.destroy();
    this.failAll(err);
    this.emit('error', err);
  }

  private onClose(): void {
    this.rl.close();
    this.failAll(new Error('IPC connection closed'));
    this.emit('close');
  }

  /** Destroy the underlying socket, rejecting all pending calls. */
  destroy(): void {
    this.rl.close();
    this.failAll(new Error('IPC connection closed'));
    this.socket.destroy();
  }

  private failAll(err: Error): void {
    if (this.#destroyed) return;
    this.#destroyed = true;
    for (const cb of this.pending.values()) {
      cb.reject(err);
    }
    this.pending.clear();
  }
}
