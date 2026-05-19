import { copyFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, resolve } from 'node:path';

const here = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(here, '..', '..', '..');
const packageRoot = resolve(here, '..');

for (const file of ['LICENSE', 'LICENSE-FSCT.md', 'NOTICE']) {
  copyFileSync(resolve(repoRoot, file), resolve(packageRoot, file));
}
