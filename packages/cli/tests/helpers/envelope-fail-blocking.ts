import { fail } from '../../src/envelope.js';

const start = performance.now();
fail('test.fail', 'NO_CONFIG', 'Config not found', 'Copy template', start);
