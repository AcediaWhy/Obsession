import { useMotionOff, useRenderHidden } from '../render';
import { AlchemistSprite, type AlchemistMood } from './AlchemistSprite';
import '../../styles/alchemistTheme.css';

type Props = {
  active: boolean;
  busy?: boolean;
  scanning?: boolean;
  alarm?: boolean;
  onClick?: () => void;
  size?: number;
  interactive?: boolean;
  paused?: boolean;
};

export function AlchemistCore({ active, busy = false, scanning = false, alarm = false, onClick, size = 240, interactive = true, paused = false }: Props) {
  const motionOff = useMotionOff();
  const hidden = useRenderHidden();
  const still = paused || motionOff || hidden;
  const phase = alarm ? 'fault' : busy ? 'engaging' : scanning ? 'scanning' : active ? 'focused' : 'idle';
  const mood: AlchemistMood = alarm ? 'rest' : busy || scanning ? 'brew' : active ? 'ready' : 'rest';
  const label = alarm ? 'Алхимик — проверьте состояние защиты' : busy || scanning ? 'Алхимик готовит зелье' : active ? 'Алхимик — зелье готово' : 'Алхимик отдыхает';
  const sprite = <AlchemistSprite size={size} mood={mood} label={label} paused={still} eyeMode="auto" replayKey={0} motionMode="auto" reference={false} previewTime={null} />;
  const shared = { className: 'alchemist-core', 'data-alchemist-core': true, 'data-phase': phase, 'data-motion': still ? 'still' : 'running', style: { width: size, height: size } };
  if (!interactive) return <div {...shared}>{hidden ? null : sprite}</div>;
  return <button {...shared} type="button" aria-label={active ? 'Отключить защиту' : 'Активировать защиту'} aria-pressed={active} aria-busy={busy} disabled={busy} onClick={onClick}>{hidden ? null : sprite}</button>;
}
