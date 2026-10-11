import { readFileSync } from 'node:fs';
import { runInNewContext } from 'node:vm';
import assert from 'node:assert/strict';
import { test } from 'node:test';

const script = readFileSync(new URL('../src-tauri/src/protected_runtime/youtube_playback.js', import.meta.url), 'utf8');
function observe({ id = 'jNQXAC9IVRw', ad = false, errorVisible = false, errorHidden = false } = {}) {
  let plays = 0;
  const video = { currentTime: 2, duration: 19, readyState: 4, paused: true, ended: false,
    videoWidth: 480, videoHeight: 360, currentSrc: 'blob:private-session',
    getVideoPlaybackQuality: () => ({ totalVideoFrames: 60, droppedVideoFrames: 5 }),
    play: () => { plays++; return Promise.resolve(); } };
  const player = { getVideoData: () => ({ video_id: id }), classList: { contains: () => ad } };
  const error = { textContent: 'Player error', getClientRects: () => errorVisible ? [{}] : [] };
  const result = runInNewContext(script, { document: {
    getElementById: () => player, querySelector: (selector) => selector === 'video' ? video : error,
  }, getComputedStyle: () => ({ visibility: errorHidden ? 'hidden' : 'visible' }) });
  return { result, video, plays };
}
test('observes the intended video, suppresses audio and excludes dropped frames', () => {
  const { result, video, plays } = observe();
  assert.equal(result.present, true); assert.equal(result.frames, 55);
  assert.equal(result.error, false); assert.equal(video.muted, true);
  assert.equal(video.playbackRate, 1); assert.equal(plays, 1);
});
test('advertising or another video does not contribute time, pixels or frames', () => {
  for (const options of [{ ad: true }, { id: 'another-video' }]) {
    const { result, plays } = observe(options);
    assert.equal(result.present, false); assert.equal(result.time, 0);
    assert.equal(result.width, 0); assert.equal(result.frames, 0); assert.equal(plays, 0);
  }
});
test('hidden error placeholders are harmless while visible errors are reported', () => {
  assert.equal(observe({ errorVisible: true }).result.error, true);
  assert.equal(observe({ errorVisible: true, errorHidden: true }).result.error, false);
});
