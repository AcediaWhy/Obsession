import { useState, type ReactNode } from 'react';
import '../styles/alchemistComposition.css';

import { AlchemistRoom } from '../design/components/AlchemistRoom';

/** Original room artwork plus lightweight, independently controllable effects. */
export function AlchemistComposition({ children, paused, onTogglePause, reference, status }: { children: ReactNode; paused: boolean; onTogglePause: () => void; reference: boolean; status: string }) {
  const [showCat, setShowCat] = useState(true);
  const [ambient, setAmbient] = useState(true);
  return <section className="alchemist-composition" aria-label="Живая композиция алхимической мастерской" data-ambient={ambient && !reference} data-paused={paused}>
    <div className="composition-heading">
      <div><p className="alchemist-eyebrow">ЭТЮД 02 / ВСЯ КОМПОЗИЦИЯ</p><h2>Полночь в лавке алхимика</h2></div>
      <div className="composition-actions">
        <button type="button" aria-pressed={!showCat} onClick={() => setShowCat(value => !value)}>{showCat ? 'Посмотреть фон отдельно' : 'Вернуть котика'}</button>
        <button type="button" aria-pressed={ambient} onClick={() => setAmbient(value => !value)}>Атмосфера {ambient ? 'вкл.' : 'выкл.'}</button>
        <button type="button" aria-pressed={paused} onClick={onTogglePause}>{paused ? 'Оживить сцену' : 'Пауза сцены'}</button>
      </div>
    </div>
    <AlchemistRoom paused={paused} ambient={ambient && !reference}>{showCat ? children : null}</AlchemistRoom>
    <div className="composition-interface" aria-label="Пример материалов интерфейса темы">
      <div className="composition-caption"><span className="composition-seal" aria-hidden="true">✦</span><div><span className="alchemist-eyebrow">ПОЛНОЧНАЯ МАСТЕРСКАЯ</span><strong>Последняя капля</strong><p>Тёплая медь, тёмное дерево и немного лунного света.</p></div></div>
      <div className="composition-palette" aria-label="Палитра темы"><i title="Слива" /><i title="Лаванда" /><i title="Медь" /><i title="Шалфей" /><i title="Молочный" /></div>
      <span className="composition-status"><i /> {status}</span>
    </div>
    <p className="composition-note">Цветок покачивается отдельными исходными стеблями, у книги шевелится закладка. Мотылёк, зелья и светлячки — кодом; котик — готовое анимированное ядро. Настройки персонажа ниже действуют и здесь.</p>
  </section>;
}
