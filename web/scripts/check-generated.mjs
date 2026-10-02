import { spawnSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import path from 'node:path';

const output = path.resolve('../target/ui/openapi-check.ts');
const check = spawnSync(process.execPath, ['node_modules/openapi-typescript/bin/cli.js', '../docs/ui/openapi.json', '-o', output], { stdio: 'inherit' });
if (check.error || check.status !== 0) throw new Error('OpenAPI type generation failed.');
if (readFileSync(output, 'utf8') !== readFileSync('src/generated/api.ts', 'utf8')) {
  throw new Error('Generated API types are stale. Run npm --prefix web run generate in the sandbox.');
}
process.stdout.write('Generated API types match the canonical OpenAPI contract.\n');
