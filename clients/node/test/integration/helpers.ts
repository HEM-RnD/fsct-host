import * as child_process from 'node:child_process';
import * as crypto from 'node:crypto';
import * as fs from 'node:fs';
import * as os from 'node:os';
import * as path from 'node:path';
import { fileURLToPath } from 'node:url';
import { FsctIpcClient } from '../../src/client.js';

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

export interface TestServer {
  socketPath: string;
  kill: () => void;
}

export async function startTestServer(): Promise<TestServer> {
  if (!explicitBin && !fs.existsSync(serverBin)) {
    buildTestServer();
  }

  const socketPath = testSocketPath();
  const proc = child_process.spawn(serverBin, [socketPath], { stdio: 'pipe' });

  let exited = false;
  proc.once('exit', () => {
    exited = true;
  });

  const deadline = Date.now() + 5000;
  let ready = false;

  while (!exited && Date.now() < deadline) {
    try {
      const probe = await FsctIpcClient.connectToEndpoint(socketPath);
      probe.disconnect();
      ready = true;
      break;
    } catch {
      await new Promise<void>((r) => setTimeout(r, 50));
    }
  }

  if (!ready) {
    proc.kill();
    throw new Error(`ipc_test_server did not become ready within 5 seconds on ${socketPath}`);
  }

  return {
    socketPath,
    kill: () => proc.kill(),
  };
}
