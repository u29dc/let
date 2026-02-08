import { ok } from '../../src/envelope.js';

const start = performance.now();
ok('test.ok', { items: [1, 2, 3] }, start);
