import { useEffect, useRef, useState, type CSSProperties } from 'react';
import { type EyeFrame } from '../../labs/alchemistLayers';
import { drawExactAlchemist, exactPoseAt, exactFiles, neutralExactPose, type ExactImages, type ExactMotionMode } from '../../labs/alchemistExact';
import { drawPotionFrame, potionFrameAt } from '../../labs/alchemistPotion';
import '../../styles/alchemistSprite.css';

const catSource = `${import.meta.env.BASE_URL}lab-assets/alchemist-cat/exact-v4/original.webp`;
export type AlchemistMood = 'rest' | 'brew' | 'ready';
type Mood = AlchemistMood;

export function AlchemistSprite({ size, mood, label, paused, eyeMode, replayKey, motionMode, reference, previewTime }: { size: number; mood: Mood; label: string; paused: boolean; eyeMode: 'auto' | EyeFrame; replayKey: number; motionMode: ExactMotionMode; reference: boolean; previewTime: number | null }) {
  const host = useRef<HTMLDivElement>(null);
  const live = useRef({ mood, paused, eyeMode, motionMode, reference, previewTime });
  live.current = { mood, paused, eyeMode, motionMode, reference, previewTime };
  const refresh = useRef<() => void>(() => {});
  const replay = useRef<() => void>(() => {});
  const [loaded, setLoaded] = useState(false);
  const [failed, setFailed] = useState(false);
  useEffect(() => {
    const element = host.current;
    if (!element) return;
    let disposed = false;
    let images: ExactImages | undefined;
    let elapsed = 0;
    let previous: number | null = null;
    let raf = 0;
    let lastPose = '';
    let measuredWidth = element.getBoundingClientRect().width;
    const canvas = element.querySelector('canvas')!;
    const potionCanvas = element.querySelector<HTMLCanvasElement>('.alchemist-potion-frame')!;
    let lastPotion = '';
    const reduced = matchMedia('(prefers-reduced-motion: reduce)');
    const redraw = () => {
      if (!images || disposed) return;
      const pixels = Math.max(1, Math.round(measuredWidth * Math.min(devicePixelRatio || 1, 3)));
      const time = reduced.matches ? 0 : (live.current.previewTime ?? elapsed);
      const potion = potionFrameAt(time, live.current.mood, !reduced.matches && !live.current.reference);
      const potionKey = `${pixels}:${Math.floor(time / 60)}:${live.current.mood}:${live.current.reference}:${reduced.matches}`;
      if (potionKey !== lastPotion) { drawPotionFrame(potionCanvas, potion, pixels); lastPotion = potionKey; }
      const pose = live.current.reference ? { ...neutralExactPose } : exactPoseAt(reduced.matches ? 0 : (live.current.previewTime ?? elapsed), live.current.mood, live.current.motionMode);
      if (!live.current.reference && live.current.eyeMode !== 'auto') pose.blink = live.current.eyeMode === 'open' ? 0 : live.current.eyeMode === 'half' ? .6 : 1;
      const key = `${pixels}:${JSON.stringify(pose)}`;
      if (lastPose === key) return;
      lastPose = key;
      drawExactAlchemist(canvas, images, pose, pixels);
      element.dataset.twitch = `${pose.tail},${pose.leftEar},${pose.rightEar}`;
      canvas.dataset.eye = String(pose.blink);
      element.dataset.action = pose.action;
      element.dataset.flaskY = '0';
    };
    const tick = (now: number) => {
      raf = 0;
      if (disposed || live.current.paused || document.hidden || reduced.matches) { previous = null; return; }
      if (previous !== null) elapsed += Math.min(now - previous, 100);
      previous = now;
      redraw();
      raf = requestAnimationFrame(tick);
    };
    const resume = () => {
      previous = null;
      redraw();
      if (live.current.paused || document.hidden || reduced.matches) { cancelAnimationFrame(raf); raf = 0; }
      else if (!raf && images) raf = requestAnimationFrame(tick);
    };
    refresh.current = resume;
    replay.current = () => { elapsed = 0; resume(); };
    const resized = () => { measuredWidth = element.getBoundingClientRect().width; lastPose = ''; redraw(); };
    const observer = new ResizeObserver(resized);
    observer.observe(element);
    const events = new AbortController();
    window.addEventListener('resize', resized, { signal: events.signal });
    document.addEventListener('visibilitychange', resume, { signal: events.signal });
    reduced.addEventListener('change', resume, { signal: events.signal });
    Promise.all(Object.entries(exactFiles).map(async ([key, filename]) => {
      const image = new Image();
      image.src = `${import.meta.env.BASE_URL}lab-assets/alchemist-cat/exact-v4/${filename}.webp`;
      await image.decode();
      return [key, image] as const;
    })).then(entries => {
      if (disposed) return;
      images = Object.fromEntries(entries) as ExactImages;
      resume();
      setLoaded(true);
    }).catch(() => { if (!disposed) setFailed(true); });
    return () => { disposed = true; cancelAnimationFrame(raf); observer.disconnect(); events.abort(); refresh.current = () => {}; replay.current = () => {}; };
  }, []);
  useEffect(() => refresh.current(), [mood, paused, eyeMode, motionMode, reference, previewTime]);
  useEffect(() => { if (replayKey > 0) replay.current(); }, [replayKey]);
  return <div className="alchemist-sprite" data-mood={mood} data-reference={reference} style={{ '--sprite-size': `${size}px` } as CSSProperties}>
    <div className="alchemist-aura" aria-hidden="true" />
    <div ref={host} className="alchemist-character" data-loaded={loaded} role="img" aria-label={failed ? `${label} — слои не загрузились, показан исходник` : label}>
      <img className="alchemist-fallback" src={catSource} width="1254" height="1254" alt="" aria-hidden="true" draggable={false} />
      <canvas className="alchemist-frame" aria-hidden="true" />
      <div className="alchemist-flask-effects" aria-hidden="true">
        <canvas className="alchemist-potion-frame" />
      </div>
    </div>
  </div>;
}
