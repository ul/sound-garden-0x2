import assert from 'node:assert/strict';
import test from 'node:test';
import { stereoFrames } from '../sound_garden_web/web/editor/monitor-samples.js';

test('web monitor forwards consecutive audio frames across 30 Hz batches', () => {
  const sampleRate = 48_000;
  const framesPerBatch = 1_600;
  const captured = [];
  for (let batch = 0; batch < 3; batch++) {
    const samples = new Float32Array(2 * framesPerBatch);
    for (let i = 0; i < framesPerBatch; i++) {
      const frame = batch * framesPerBatch + i;
      samples[2 * i] = Math.sin(2 * Math.PI * 220 * frame / sampleRate);
      samples[2 * i + 1] = -samples[2 * i];
    }
    captured.push(...stereoFrames(samples));
  }
  assert.equal(captured.length, 3 * framesPerBatch);
  for (const frame of [0, 511, 512, 1_599, 1_600, 3_199, 3_200, 4_799]) {
    assert.ok(Math.abs(captured[frame][0] - Math.sin(2 * Math.PI * 220 * frame / sampleRate)) < 1e-6);
    assert.equal(captured[frame][1], -captured[frame][0]);
  }
});
