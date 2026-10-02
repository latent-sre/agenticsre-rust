import { spawn } from 'node:child_process';
import path from 'node:path';

const python = process.env.WORKBENCH_BROWSER_PYTHON;
if (!python || !process.env.WORKBENCH_BROWSER_EXECUTABLE) {
  throw new Error('Run browser checks through tools/test-ui-sandbox.sh or the required Cally boundary.');
}
const child = spawn(python, ['-B', path.resolve('tests/browser.py'), ...process.argv.slice(2)], { stdio: 'inherit' });
child.on('error', () => { process.stderr.write('Could not start the browser test driver.\n'); process.exitCode = 1; });
child.on('exit', code => { process.exitCode = code ?? 1; });
