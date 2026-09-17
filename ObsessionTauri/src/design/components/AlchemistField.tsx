import { useMotionOff, useRenderHidden } from '../render';
import { AlchemistRoom } from './AlchemistRoom';
import '../../styles/alchemistTheme.css';

export function AlchemistField({ paused = false }: { paused?: boolean }) {
  const motionOff = useMotionOff();
  const hidden = useRenderHidden();
  return <div className="alchemist-field" aria-hidden="true">
    {!hidden && <AlchemistRoom paused={paused || motionOff} />}
    <div className="alchemist-field-shade" />
  </div>;
}
