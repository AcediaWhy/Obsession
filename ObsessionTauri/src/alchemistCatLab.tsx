import { useEffect, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { type EyeFrame } from './labs/alchemistLayers';
import { type ExactMotionMode } from './labs/alchemistExact';
import { AlchemistSprite } from './design/components/AlchemistSprite';
import './styles/fonts.css';
import './styles/alchemistCatLab.css';
import { AlchemistComposition } from './labs/AlchemistComposition';

const catSource = `${import.meta.env.BASE_URL}lab-assets/alchemist-cat/exact-v4/original.webp`;
const moods = [
  { id: 'rest', name: 'Размышляет', text: 'Тихое свечение. Рецепт пока держит в секрете.' },
  { id: 'brew', name: 'Колдует', text: 'Зелье пузырится, по стеклу пробегает блик. Иногда из горлышка вырывается маленький пуф.' },
  { id: 'ready', name: 'Зелье готово', text: 'Ровный шалфейный свет. Кажется, на этот раз получилось.' },
] as const;
type Mood = typeof moods[number]['id'];


function AlchemistCatLab() {
  const [mood, setMood] = useState<Mood>('brew');
  const [paused, setPaused] = useState(() => matchMedia('(prefers-reduced-motion: reduce)').matches);
  const [hidden, setHidden] = useState(document.hidden);
  const [light, setLight] = useState(false);
  const [eyeMode, setEyeMode] = useState<'auto' | EyeFrame>('auto');
  const [replayKey, setReplayKey] = useState(0);
  const [motionMode, setMotionMode] = useState<ExactMotionMode>('auto');
  const [reference, setReference] = useState(false);
  const [previewTime, setPreviewTime] = useState<number | null>(null);
  const selected = moods.find(item => item.id === mood)!;

  useEffect(() => {
    const events = new AbortController();
    const reduced = matchMedia('(prefers-reduced-motion: reduce)');
    document.addEventListener('visibilitychange', () => setHidden(document.hidden), { signal: events.signal });
    reduced.addEventListener('change', event => setPaused(event.matches), { signal: events.signal });
    return () => events.abort();
  }, []);

  return <main className="alchemist-lab" data-paused={paused || hidden} data-eyes={eyeMode}>
    <div className="alchemist-shell">
      <header className="alchemist-header"><a href="/">obsession<span> / мастерская тем</span></a><span>ЭТЮД 01 · АЛХИМИК</span></header>
      <section className="alchemist-intro">
        <div><p className="alchemist-eyebrow">МАЛЕНЬКАЯ ДОМАШНЯЯ МАГИЯ</p><h1>Последняя <em>капля.</em></h1></div>
        <p>Большая шляпа. Серьёзный взгляд.<br />И совершенно секретный рецепт.</p>
      </section>
      <AlchemistComposition paused={paused || hidden} reference={reference} status={selected.name} onTogglePause={() => { if (paused) setPreviewTime(null); setPaused(value => !value); }}>
        <AlchemistSprite size={440} mood={mood} paused={paused || hidden} eyeMode={eyeMode} replayKey={replayKey} motionMode={motionMode} reference={reference} previewTime={previewTime} label="Котик-алхимик на столе в ночной мастерской" />
      </AlchemistComposition>
      <section className="alchemist-workbench" aria-label="Примерка котика-алхимика">
        <div className="alchemist-stage" data-light={light}>
          <div className="alchemist-stage-top"><span><i /> {selected.name}</span><span>ТВОИ СЛОИ · МАЛЕНЬКИЙ РИТУАЛ</span></div>
          <AlchemistSprite size={440} mood={mood} paused={paused || hidden} eyeMode={eyeMode} replayKey={replayKey} motionMode={motionMode} reference={reference} previewTime={previewTime} label="Белый котик в фиолетовой шляпе держит зелёное зелье двумя лапками" />
          <div className="alchemist-stage-bottom"><span>ИСХОДНЫЕ ПИКСЕЛИ · МАЛЫЕ ДВИЖЕНИЯ</span><div className="alchemist-playback"><button onClick={() => { setReference(false); setPreviewTime(null); setMood('brew'); setEyeMode('auto'); setPaused(false); setReplayKey(value => value + 1); }}>Показать действие</button><button aria-pressed={paused} onClick={() => { if (paused) setPreviewTime(null); setPaused(value => !value); }}>{paused ? 'Продолжить' : 'Пауза'}</button></div></div>
        </div>
        <aside className="alchemist-controls">
          <section><p className="alchemist-eyebrow">01 / ХАРАКТЕР</p><h2>Занят важным.</h2><div className="alchemist-moods">{moods.map((item, index) => <button key={item.id} aria-pressed={mood === item.id} onClick={() => setMood(item.id)}><span>0{index + 1}</span>{item.name}<i /></button>)}</div><p className="alchemist-description" aria-live="polite">{selected.text}</p></section>
          <section className="alchemist-palette"><p className="alchemist-eyebrow">02 / НАСТРОЕНИЕ ТЕМЫ</p><div className="alchemist-swatches"><span style={{ background: '#211a29' }} title="Баклажан · #211a29" /><span style={{ background: '#9c7bac' }} title="Лаванда · #9c7bac" /><span style={{ background: '#b9d89b' }} title="Шалфей · #b9d89b" /><span style={{ background: '#c18b65' }} title="Медь · #c18b65" /></div><p>Баклажан, лаванда, шалфей<br />и чуть-чуть тёплой меди.</p><label><input type="checkbox" checked={light} onChange={event => setLight(event.target.checked)} /> Проверить на светлом фоне</label><label className="alchemist-eye-select">Кадр глаз<select value={eyeMode} onChange={event => setEyeMode(event.target.value as typeof eyeMode)}><option value="auto">Автоморгание</option><option value="open">Открыты</option><option value="half">Прикрыты</option><option value="closed">Закрыты</option></select></label></section>
          <section className="alchemist-exact-controls"><label><input type="checkbox" checked={reference} onChange={event => setReference(event.target.checked)} /> Эталон — без движений и эффектов</label><label>Движения<select value={motionMode} onChange={event => { setMotionMode(event.target.value as ExactMotionMode); setPreviewTime(null); setReference(false); setReplayKey(value => value + 1); }}><option value="auto">Вместе, с паузами</option><option value="still">Неподвижно</option><option value="ears">Только ушки</option><option value="tail">Только хвост</option></select></label><label>Покадровая проверка<input aria-label="Время кадра" type="range" min="0" max="23000" step="25" value={previewTime ?? 0} onChange={event => { setPreviewTime(Number(event.target.value)); setReference(false); setPaused(true); }} /><output>{previewTime === null ? 'Автоматически' : `${(previewTime / 1000).toFixed(2)} с`}</output></label></section>
        </aside>
      </section>
      <section className="alchemist-scale" aria-label="Проверка размеров">
        <div className="alchemist-scale-copy"><p className="alchemist-eyebrow">03 / РЕАЛЬНЫЙ МАСШТАБ</p><h2>Магия<br />в мелочах.</h2><p>Та же картинка и те же эффекты.<br />Без увеличения карточки ради эскиза.</p><small>Это примерка, не подключённая тема.</small></div>
        <div className="alchemist-theme-tile"><span className="alchemist-eyebrow">КАРТОЧКА · 104 PX</span><AlchemistSprite size={104} mood={mood} paused={paused || hidden} eyeMode={eyeMode} replayKey={replayKey} motionMode={motionMode} reference={reference} previewTime={previewTime} label="Котик-алхимик в карточке 104 на 104 пикселя" /><strong>Алхимик</strong><span className="alchemist-tile-note">Последняя капля</span></div>
        <div className="alchemist-core-preview"><span className="alchemist-eyebrow">ГЛАВНОЕ ЯДРО · 240 PX</span><AlchemistSprite size={240} mood={mood} paused={paused || hidden} eyeMode={eyeMode} replayKey={replayKey} motionMode={motionMode} reference={reference} previewTime={previewTime} label="Котик-алхимик в размере ядра 240 на 240 пикселей" /><span className="alchemist-ready"><i /> {selected.name}</span></div>
      </section>
      <footer><span>Новый PNG · исходная прозрачность сохранена · WebP lossless.</span><a href={catSource} target="_blank" rel="noreferrer">Открыть исходный котик ↗</a></footer>
    </div>
  </main>;
}

createRoot(document.getElementById('root')!).render(<AlchemistCatLab />);
