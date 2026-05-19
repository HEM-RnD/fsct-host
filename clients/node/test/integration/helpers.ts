import * as child_process from 'node:child_process';
import * as crypto from 'node:crypto';
import * as fs from 'node:fs';
import * as os from 'node:os';
import * as path from 'node:path';
import * as readline from 'node:readline';
import { fileURLToPath } from 'node:url';

const repoRoot = fileURLToPath(new URL('../../../../', import.meta.url));
const defaultBin = path.join(
  repoRoot,
  'target/debug/ipc_test_server' + (process.platform === 'win32' ? '.exe' : ''),
);

const explicitBin = process.env['FSCT_TEST_SERVER_BIN'];
export const serverBin = explicitBin ?? defaultBin;

export function testSocketPath(): string {
  const id = crypto.randomUUID().replace(/-/g, '');
  return process.platform === 'win32'
    ? `\\\\.\\pipe\\fsct_test_${id}`
    : path.join(os.tmpdir(), `fsct_test_${id}.sock`);
}

function buildTestServer(): void {
  console.log('ipc_test_server not found, running: cargo build --bin ipc_test_server');
  const result = child_process.spawnSync('cargo', ['build', '--bin', 'ipc_test_server'], {
    cwd: repoRoot,
    stdio: 'inherit',
  });
  if (result.error) {
    throw new Error(`Failed to run cargo: ${result.error.message}`);
  }
  if (result.status !== 0) {
    throw new Error('cargo build --bin ipc_test_server failed');
  }
}

/** A JSON event received from the server's stdout, emitted when the mock driver handles an update call. */
export interface ReceivedEvent {
  type: 'received';
  method: string;
  playerId: number;
  [key: string]: unknown;
}

/**
 * FIFO queue for ReceivedEvents from the server's stdout.
 * Supports matching by method name, either from the existing buffer or
 * by registering a waiter that is resolved when the event arrives later.
 */
class EventBuffer {
  private buffer: ReceivedEvent[] = [];
  private waiters: Array<{
    method: string;
    resolve: (e: ReceivedEvent) => void;
    reject: (e: Error) => void;
    timer: ReturnType<typeof setTimeout>;
  }> = [];
  private closedWith: Error | null = null;

  push(event: ReceivedEvent): void {
    const idx = this.waiters.findIndex((w) => w.method === event.method);
    if (idx >= 0) {
      const [waiter] = this.waiters.splice(idx, 1);
      clearTimeout(waiter.timer);
      waiter.resolve(event);
    } else {
      this.buffer.push(event);
    }
  }

  wait(method: string, timeoutMs: number): Promise<ReceivedEvent> {
    if (this.closedWith) {
      return Promise.reject(this.closedWith);
    }
    const idx = this.buffer.findIndex((e) => e.method === method);
    if (idx >= 0) {
      const [event] = this.buffer.splice(idx, 1);
      return Promise.resolve(event);
    }
    return new Promise<ReceivedEvent>((resolve, reject) => {
      const timer = setTimeout(() => {
        const i = this.waiters.findIndex((w) => w.resolve === resolve);
        if (i >= 0) this.waiters.splice(i, 1);
        reject(new Error(`Timeout (${timeoutMs}ms) waiting for server event: method=${method}`));
      }, timeoutMs);
      this.waiters.push({ method, resolve, reject, timer });
    });
  }

  close(reason: Error): void {
    this.closedWith = reason;
    for (const waiter of this.waiters) {
      clearTimeout(waiter.timer);
      waiter.reject(reason);
    }
    this.waiters = [];
  }
}

export interface TestServer {
  socketPath: string;
  kill: () => void;
  /**
   * Resolves with the next server-side event for the given IPC method.
   * Checks the buffer first; if empty, waits up to `timeoutMs` (default 2000ms).
   */
  waitForReceived(method: string, timeoutMs?: number): Promise<ReceivedEvent>;
  /** Sends an emit_device_changed command to the server via stdin. */
  emitDeviceChanged(event: 'added' | 'removed', deviceId: string): void;
}

export async function startTestServer(): Promise<TestServer> {
  if (!explicitBin && !fs.existsSync(serverBin)) {
    buildTestServer();
  }

  const socketPath = testSocketPath();
  // stdin: pipe (for commands), stdout: pipe (for events), stderr: inherit (visible in test output)
  const proc = child_process.spawn(serverBin, [socketPath], {
    stdio: ['pipe', 'pipe', 'inherit'],
  });

  const events = new EventBuffer();
  let serverReady = false;
  let readyResolve!: () => void;
  let readyReject!: (e: Error) => void;
  const readyPromise = new Promise<void>((resolve, reject) => {
    readyResolve = resolve;
    readyReject = reject;
  });

  const rl = readline.createInterface({ input: proc.stdout!, crlfDelay: Infinity });
  rl.on('line', (line) => {
    let parsed: Record<string, unknown>;
    try {
      parsed = JSON.parse(line) as Record<string, unknown>;
    } catch {
      return;
    }
    if (parsed['type'] === 'ready' && !serverReady) {
      serverReady = true;
      readyResolve();
    } else if (parsed['type'] === 'received') {
      events.push(parsed as unknown as ReceivedEvent);
    }
  });

  proc.once('exit', () => {
    if (!serverReady) {
      readyReject(new Error('server exited before signalling ready'));
    }
    events.close(new Error('server exited'));
    rl.close();
  });

  await Promise.race([
    readyPromise,
    new Promise<void>((_, reject) =>
      setTimeout(() => reject(new Error('timeout (5s) waiting for server ready signal')), 5000),
    ),
  ]);

  return {
    socketPath,
    kill: () => {
      events.close(new Error('server killed'));
      proc.kill();
    },
    waitForReceived: (method, timeoutMs = 2000) => events.wait(method, timeoutMs),
    emitDeviceChanged: (event, deviceId) => {
      const cmd = JSON.stringify({ type: 'emit_device_changed', event, deviceId });
      proc.stdin!.write(cmd + '\n');
    },
  };
}
