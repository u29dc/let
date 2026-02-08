import { fail } from '../../src/envelope.js';

const start = performance.now();
fail('test.fail', 'TEST_ERROR', 'Something went wrong', 'Try again', start);
