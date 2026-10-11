// Executed by the native test host in its isolated WebView, never in the app UI.
(() => {
  const expected = 'jNQXAC9IVRw';
  const player = document.getElementById('movie_player');
  const video = document.querySelector('video');
  const identity = player?.getVideoData?.()?.video_id;
  const ad = !!player?.classList.contains('ad-showing');
  const error = document.querySelector('.ytp-error-content-wrap, #error-screen');
  const visibleError = !!error && error.getClientRects().length > 0
    && getComputedStyle(error).visibility !== 'hidden' && !!error.textContent?.trim();
  const usable = identity === expected && !ad && !!video;
  if (usable) {
    video.muted = true;
    video.playbackRate = 1;
    if (video.paused && !video.ended) video.play().catch(() => {});
  }
  const quality = video?.getVideoPlaybackQuality?.();
  return {
    identity: identity ?? '', ad, error: visibleError,
    present: usable, time: usable ? video.currentTime : 0,
    duration: usable && Number.isFinite(video.duration) ? video.duration : 0,
    ready: usable ? video.readyState : 0, paused: !usable || video.paused,
    ended: usable && video.ended, width: usable ? video.videoWidth : 0,
    height: usable ? video.videoHeight : 0,
    frames: usable ? Math.max(0, (quality?.totalVideoFrames ?? 0) - (quality?.droppedVideoFrames ?? 0)) : 0,
    dropped: usable ? (quality?.droppedVideoFrames ?? 0) : 0,
    source: usable ? video.currentSrc : '',
  };
})()
