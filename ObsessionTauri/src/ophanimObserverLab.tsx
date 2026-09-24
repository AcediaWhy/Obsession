import { useEffect, useRef, useState } from 'react';
import ReactDOM from 'react-dom/client';
import { OphanimCatSpriteLab } from './design/components/OphanimCatSpriteLab';
import type { ObsessionVisualPhase } from './design/obsessionVisualState';
import { createOphanimObserver, type ObserverOptions } from './labs/ophanimObserver';
import './styles/fonts.css';
import './styles/ophanimObserverLab.css';

const states: { id: ObsessionVisualPhase; label: string; description: string }[] = [
  { id: 'idle', label: 'Бдение', description: 'Кольца плывут вокруг котика. Глаза на ободах лениво моргают.' },
  { id: 'engaging', label: 'Пробуждение', description: 'Глаза раскрываются по очереди, кольца набирают ход.' },
  { id: 'scanning', label: 'Поиск', description: 'Глаза на широких обручах переводят взгляд и осматривают пространство.' },
  { id: 'focused', label: 'Защита', description: 'Кольца плавно замирают. Все глаза открыты — страж на посту.' },
  { id: 'fault', label: 'Тревога', description: 'Ободы расходятся, золото теплеет до меди, глаза настораживаются.' },
];
const initialOptions: ObserverOptions = { phase: 'idle', rings: true, eyes: true, strength: 1 };

function OphanimObserverLab() {
  const [options, setOptions] = useState(initialOptions);
  const [paused, setPaused] = useState(() => matchMedia('(prefers-reduced-motion: reduce)').matches);
  const [light, setLight] = useState(false);
  const [status, setStatus] = useState('Загружаю котика…');
  const stage = useRef<HTMLCanvasElement>(null);
  const small = useRef<HTMLCanvasElement>(null);
  const hero = useRef<HTMLCanvasElement>(null);
  const live = useRef({ options, paused });
  live.current = { options, paused };
  const invalidate = useRef<() => void>(() => {});
  const selected = states.find(s => s.id === options.phase)!;

  useEffect(() => {
    const cat = new Image();
    cat.src = `${import.meta.env.BASE_URL}devtools/labs/assets/ophanim-observer/cat.png`;
    const rigs: ReturnType<typeof createOphanimObserver>[] = [];
    let disposed = false;
    let raf = 0;
    let last: number | null = null;
    let previousDraw = 0;
    const events = new AbortController();
    const reduced = matchMedia('(prefers-reduced-motion: reduce)');
    function draw(dt: number) { for (const rig of rigs) rig.render(dt, live.current.options); }
    function frame(now: number) {
      raf = 0;
      if (disposed || document.hidden || live.current.paused) { last = null; return; }
      if (now - previousDraw >= 1000 / 30) {
        draw(last === null ? 0 : (now - last) / 1000);
        last = now; previousDraw = now;
      }
      raf = requestAnimationFrame(frame);
    }
    function refresh() {
      if (disposed || !rigs.length) return;
      draw(0);
      if (live.current.paused || document.hidden) { cancelAnimationFrame(raf); raf = 0; last = null; }
      else if (!raf) raf = requestAnimationFrame(frame);
    }
    invalidate.current = refresh;
    document.addEventListener('visibilitychange', refresh, { signal: events.signal });
    reduced.addEventListener('change', event => setPaused(event.matches), { signal: events.signal });
    cat.decode().then(() => {
      if (disposed) return;
      for (const element of [stage.current, small.current, hero.current]) {
        if (element) rigs.push(createOphanimObserver(element, cat));
      }
      setStatus('Котик на месте'); refresh();
    }).catch(() => {
      rigs.forEach(rig => rig.dispose()); rigs.length = 0;
      if (!disposed) setStatus('Не удалось запустить сцену. Проверь поддержку WebGL и обнови страницу.');
    });
    return () => { disposed = true; events.abort(); cancelAnimationFrame(raf); rigs.forEach(rig => rig.dispose()); invalidate.current = () => {}; };
  }, []);
  useEffect(() => invalidate.current(), [options, paused]);

  return <main className="observer-lab" data-light={light}>
    <div className="observer-shell">
      <header className="observer-header"><a href="/">obsession<span> / лаборатория</span></a><span className="observer-edition">OPHANIM · STUDY 02</span></header>
      <section className="observer-intro"><div><p className="observer-eyebrow">МАЛЕНЬКИЙ КОТ. БОЛЬШОЕ БДЕНИЕ.</p><h1>Небесный<br /><em>наблюдатель.</em></h1></div><p>Он ещё не совсем понял,<br />почему вокруг него вращается вселенная.</p></section>
      <section className="observer-workbench">
        <div className="observer-stage">
          <div className="observer-stage-label"><span><i /> {selected.label}</span><span>01 / ЖИВОЕ ЯДРО</span></div>
          <canvas ref={stage} width="1024" height="1024" role="img" aria-label={`Котик с вращающимися кольцами: ${selected.label}`} />
          <div className="observer-stage-foot"><span>ТВОЙ РИСУНОК · ЗОЛОТЫЕ КОЛЬЦА</span><button onClick={() => setPaused(!paused)} aria-pressed={paused}>{paused ? '▷ Продолжить' : 'Ⅱ Пауза'}</button></div>
        </div>
        <aside className="observer-aside">
          <section className="observer-panel"><h2>Характер движения</h2><div className="observer-states">{states.map((state, index) => <button key={state.id} aria-pressed={state.id === options.phase} onClick={() => setOptions({ ...options, phase: state.id })}><span>0{index + 1}</span>{state.label}<i /></button>)}</div><p className="observer-description" aria-live="polite">{selected.description}</p></section>
          <section className="observer-panel observer-tuning"><h2>Примерка</h2><label>Кольца<input type="checkbox" checked={options.rings} onChange={e => setOptions({ ...options, rings: e.target.checked })} /></label><label>Глаза на кольцах<input type="checkbox" checked={options.eyes} onChange={e => setOptions({ ...options, eyes: e.target.checked })} /></label><label>Светлый фон<input type="checkbox" checked={light} onChange={e => setLight(e.target.checked)} /></label><label className="observer-speed">Движение <output>{options.strength.toFixed(1)}×</output><input aria-label="Скорость движения" type="range" min="0.3" max="1.5" step="0.1" value={options.strength} onChange={e => setOptions({ ...options, strength: Number(e.target.value) })} /></label></section>
        </aside>
      </section>
      <section className="observer-scale"><div className="observer-scale-copy"><p className="observer-eyebrow">ПРОВЕРКА МАСШТАБА</p><h2>Как он живёт<br />в интерфейсе</h2><p>Одна и та же сцена.<br />От карточки темы до главного ядра.</p></div><div className="observer-tile"><span className="observer-caption">БЫЛО · 104 PX</span><OphanimCatSpriteLab size={104} phase={options.phase} paused={paused} /><strong>Ophanim</strong></div><div className="observer-tile observer-tile-new"><span className="observer-caption">ПРОБА · 104 PX</span><canvas ref={small} width="312" height="312" style={{ width: 104, height: 104 }} role="img" aria-label="Новый Ophanim в карточке темы" /><strong>Ophanim</strong></div><div className="observer-hero-preview"><span className="observer-caption">ЯДРО · 240 PX</span><canvas ref={hero} width="480" height="480" style={{ width: 240, height: 240 }} role="img" aria-label="Новый Ophanim в размере главного ядра" /></div></section>
      <footer><span>Эскиз в движении. Отдельная лаборатория Ophanim.</span><span role="status">{status}</span></footer>
    </div>
  </main>;
}

const root = ReactDOM.createRoot(document.getElementById('root')!);
root.render(<OphanimObserverLab />);
if (import.meta.hot) import.meta.hot.dispose(() => root.unmount());
