import { useState } from "react";

import type { ObsessionVisualPhase } from "../../obsessionVisualState";
import { Chip, Row, SectionLabel, Switch } from "../atoms";
import { GlassPanel } from "../GlassPanel";
import { SunkenStarSpriteLab } from "../SunkenStarSpriteLab";
import { Icon } from "./pixelIcons";
import { MATERIALS, type Level, type Material, type PlaneIndex, rawRamp } from "../pixelart/pixelCore";
import { labToast } from "./PixelToaster";

// Лабораторная половина: сверху лаба выглядит как боевой экран, здесь показывает
// свои кишки — рампы, планы глубины, шаг сетки, стенд спрайта. Изоляция планов и
// тумблер дизеринга не декорация: только ими правила «дальнее светлее ближнего» и
// «дизеринг вместо градиента» вообще можно проверить глазами.
//
// Оформление — общий стеклянный язык приложения, как и весь остальной хром.

const LEVELS: readonly Level[] = [0, 1, 2, 3];
const LEVEL_NAMES = ["shade", "body", "light", "spec"] as const;
const PHASES: readonly ObsessionVisualPhase[] = ["idle", "engaging", "scanning", "focused", "fault"];
const PLANE_LABELS: Record<PlaneIndex, string> = {
  0: "0 · небо",
  1: "1 · дальний город",
  2: "2 · средний город",
  3: "3 · крыша",
};

export type InspectorState = {
  isolate: PlaneIndex | null;
  dither: boolean;
  scale: number | null;
  artWidth: number;
  artHeight: number;
  artScale: number;
  paused: boolean;
  phase: ObsessionVisualPhase;
};

export type InspectorHandlers = {
  onIsolate: (value: PlaneIndex | null) => void;
  onDither: (value: boolean) => void;
  onScale: (value: number | null) => void;
  onPhase: (value: ObsessionVisualPhase) => void;
  onPaused: (value: boolean) => void;
};

async function copyColor(material: Material, level: Level, value: string) {
  try {
    await navigator.clipboard.writeText(value);
    labToast.success(`${material}.${LEVEL_NAMES[level]} → ${value}`);
  } catch {
    labToast.warn(`буфер обмена недоступен · ${value}`);
  }
}

function PaletteCard() {
  const [hovered, setHovered] = useState<string | null>(null);
  return (
    <GlassPanel className="sunken-lab-inspect">
      <SectionLabel>Палитра · {MATERIALS.length} рампы × 4 ступени</SectionLabel>
      <div className="sunken-lab-ramps">
        {MATERIALS.map((material) => (
          <div className="sunken-lab-ramp" key={material}>
            <span>{material}</span>
            <div>
              {LEVELS.map((level) => {
                const value = rawRamp(material, level);
                const id = `${material}-${level}`;
                return (
                  <button
                    aria-label={`${material} ${LEVEL_NAMES[level]} ${value}`}
                    className="sunken-lab-swatch no-drag"
                    key={id}
                    onClick={() => void copyColor(material, level, value)}
                    onPointerEnter={() => setHovered(id)}
                    onPointerLeave={() => setHovered((current) => (current === id ? null : current))}
                    style={{ background: value }}
                    type="button"
                  >
                    {hovered === id ? <em>{value}</em> : null}
                  </button>
                );
              })}
            </div>
          </div>
        ))}
      </div>
      <p className="sunken-lab-note">
        Ступени идут shade → body → light → spec. Дальнее в кадре СВЕТЛЕЕ и бледнее
        ближнего — только так глубина читается в шестнадцати цветах. Весь тёплый
        свет один и он лунный: холодный, сверху-справа. Тёплого в кадре две крупицы —
        щель люка и молоко; остальное тепло живёт в окнах города.
      </p>
    </GlassPanel>
  );
}

function SpriteBench({ phase, paused }: { phase: ObsessionVisualPhase; paused: boolean }) {
  return (
    <GlassPanel className="sunken-lab-inspect">
      <SectionLabel>Стенд спрайта · 64×64</SectionLabel>
      <div className="sunken-lab-bench">
        {PHASES.map((item) => (
          <div data-current={item === phase || undefined} key={item}>
            <SunkenStarSpriteLab detail="hero" paused={paused} phase={item} size={128} />
            <span>{item}</span>
          </div>
        ))}
      </div>
    </GlassPanel>
  );
}

function RenderCard({ state, handlers }: { state: InspectorState; handlers: InspectorHandlers }) {
  const { isolate, dither, scale, artWidth, artHeight, artScale, paused, phase } = state;
  return (
    <GlassPanel className="sunken-lab-inspect">
      <SectionLabel>Рендер</SectionLabel>

      <Row label="Фаза">
        <div className="sunken-lab-chips">
          {PHASES.map((item) => (
            <Chip
              active={phase === item}
              ariaChecked={phase === item}
              key={item}
              label={item}
              onClick={() => handlers.onPhase(item)}
              role="radio"
            />
          ))}
        </div>
      </Row>

      <Row hint="Каждый план сдвинут по контрасту: дальний к дымке, ближний к чернилам" label="Планы глубины">
        <div className="sunken-lab-chips">
          <Chip active={isolate === null} label="все" onClick={() => handlers.onIsolate(null)} />
          {([0, 1, 2, 3] as const).map((index) => (
            <Chip
              active={isolate === index}
              key={index}
              label={PLANE_LABELS[index]}
              onClick={() => handlers.onIsolate(isolate === index ? null : index)}
            />
          ))}
        </div>
      </Row>

      <Row hint="Выключенный — жёсткие стыки ступеней" label="Дизеринг">
        <Switch ariaLabel="Дизеринг" checked={dither} onChange={handlers.onDither} />
      </Row>

      <Row hint={`${artWidth}×${artHeight} арт-пикселей`} label="Шаг сетки">
        <div className="sunken-lab-chips">
          <Chip active={scale === null} label="авто" onClick={() => handlers.onScale(null)} />
          {[3, 4, 5, 6].map((value) => (
            <Chip
              active={scale === value}
              key={value}
              label={`×${value}`}
              onClick={() => handlers.onScale(value)}
            />
          ))}
        </div>
      </Row>

      <Row label="Движение">
        <Switch ariaLabel="Движение" checked={!paused} onChange={(value) => handlers.onPaused(!value)} />
      </Row>

      <div className="sunken-lab-readout">
        <Icon.Ruler size={16} />
        <b>ROOFTOP LAB</b>
        <span>
          {artWidth}×{artHeight}
        </span>
        <span>×{artScale}</span>
      </div>
    </GlassPanel>
  );
}

export function InspectorStrip({
  state,
  handlers,
}: {
  state: InspectorState;
  handlers: InspectorHandlers;
}) {
  return (
    <section aria-label="Инспектор лаборатории" className="sunken-lab-inspector" data-axolotl-inspector>
      <PaletteCard />
      <SpriteBench paused={state.paused} phase={state.phase} />
      <RenderCard handlers={handlers} state={state} />
    </section>
  );
}

