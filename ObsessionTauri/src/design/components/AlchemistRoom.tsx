import { type ReactNode } from 'react';
import '../../styles/alchemistComposition.css';

const base = `${import.meta.env.BASE_URL}lab-assets/alchemist-cat/`;

/** Shared approved artwork and effects. No lab controls or embedded character. */
export function AlchemistRoom({ children, paused = false, ambient = true }: { children?: ReactNode; paused?: boolean; ambient?: boolean }) {
  return <div className="alchemist-room alchemist-composition" data-ambient={ambient} data-paused={paused}>
    <div className="composition-scene">
      <img className="composition-room" src={`${base}scene-v1/workshop.webp`} width="1536" height="1024" alt="Ночная алхимическая мастерская: полки со склянками слева, лунное окно справа, деревянный стол и свеча" draggable={false} />
      <div className="composition-ambient" aria-hidden="true">
        <i className="room-candle-light" /><i className="room-flame-heart" />
        <div className="room-moon-dust"><i /><i /><i /><i /><i /><i /></div>
        <i className="room-bottle room-bottle-sage" /><i className="room-bottle room-bottle-violet" />
        <svg className="room-life" viewBox="0 0 1536 1024" focusable="false" shapeRendering="crispEdges">
          <g className="room-flower-layers">
            <image href={`${base}scene-v2-flora/backing.webp`} x="1380" y="480" width="156" height="210" />
            <image className="room-flower-stem room-flower-right" href={`${base}scene-v2-flora/right.webp`} x="1380" y="480" width="156" height="210" />
            <image className="room-flower-stem room-flower-tall" href={`${base}scene-v2-flora/tall.webp`} x="1380" y="480" width="156" height="210" />
            <image className="room-flower-stem room-flower-left" href={`${base}scene-v2-flora/left.webp`} x="1380" y="480" width="156" height="210" />
          </g>
          <g className="room-bookmark">
            <path fill="#4d303c" d="M1500 741H1515V754H1503V749H1500Z" />
            <path fill="#b78281" d="M1503 743H1512V752H1506V747H1503Z" />
            <g className="room-bookmark-ribbon">
              <path fill="#4d303c" d="M1503 751H1515V775H1503Z" />
              <path fill="#ab727a" d="M1505 751H1512V775H1505Z" />
              <path fill="#d1a39b" d="M1505 751H1507V775H1505Z" />
              <g className="room-bookmark-tip">
                <path fill="#4d303c" d="M1503 774H1515V796H1511V791H1507V796H1503Z" />
                <path fill="#ab727a" d="M1505 774H1512V790L1509 787L1505 791Z" />
                <path fill="#c7a477" d="M1505 782H1512V785H1505Z" />
              </g>
            </g>
          </g>
          <defs>
            <clipPath id="workshop-sage-liquid"><path d="M152 196H174V260H152Z" /></clipPath>
            <clipPath id="workshop-violet-liquid"><path d="M213 226H236V234H244V253H236V260H211V251H205V235H213Z" /></clipPath>
            <clipPath id="workshop-herb-jar"><path d="M209 427H250V440H264V482H254V493H198V480H190V447H209Z" /></clipPath>
          </defs>
          <g className="room-smoke" transform="translate(98 695)">
            <path className="room-smoke-puff" d="M-3 0H3V-6H7V-12H1V-6H-3Z" />
            <path className="room-smoke-puff room-smoke-puff-b" d="M-3 0H3V-6H7V-12H1V-6H-3Z" />
            <path className="room-smoke-puff room-smoke-puff-c" d="M-3 0H3V-6H7V-12H1V-6H-3Z" />
          </g>
          <g clipPath="url(#workshop-sage-liquid)" className="room-liquid-bubbles">
            <path className="room-liquid-rise" d="M156 258H160V262H156Z" />
            <path className="room-liquid-rise room-liquid-rise-b" d="M165 258H170V263H165Z" />
          </g>
          <g clipPath="url(#workshop-violet-liquid)" className="room-liquid-bubbles room-liquid-purple">
            <path className="room-liquid-rise room-liquid-rise-c" d="M217 257H222V262H217Z" />
            <path className="room-liquid-rise room-liquid-rise-d" d="M230 257H234V261H230Z" />
          </g>
          <g className="room-herb-jar" clipPath="url(#workshop-herb-jar)">
            <g transform="translate(213 470)"><path className="room-jar-leaf" d="M-6 0V-5H0V-9H5V0H0V5H-6Z" /></g>
            <g transform="translate(240 465)"><path className="room-jar-leaf room-jar-leaf-b" d="M-5-6H1V-2H6V4H0V8H-5Z" /></g>
            <g transform="translate(226 447)"><path className="room-jar-leaf room-jar-leaf-c" d="M-4-5H4V2H0V6H-4Z" /></g>
            <g transform="translate(211 482)"><path className="room-jar-spark" d="M-2-2H2V2H-2Z" /></g>
            <g transform="translate(246 478)"><path className="room-jar-spark room-jar-spark-b" d="M-2-2H2V2H-2Z" /></g>
          </g>
          <g className="room-moth-route">
            <g className="room-moth-hover">
              <g className="room-moth-wings">
                <path fill="#554238" d="M-2-4H-6V-8H-12V-10H-18V0H-15V4H-10V8H-3V3H3V8H10V4H15V0H18V-10H12V-8H6V-4H2Z" />
                <path fill="#d8c39a" d="M-3-2H-7V-6H-12V-8H-16V-1H-13V2H-8V6H-4V1H4V6H8V2H13V-1H16V-8H12V-6H7V-2H3Z" />
                <path fill="#f3e2ba" d="M-14-6H-10V-3H-6V0H-10V-2H-14ZM14-6H10V-3H6V0H10V-2H14Z" />
                <path fill="#a58167" d="M-12-2H-8V2H-12ZM8-2H12V2H8Z" />
              </g>
              <path fill="#42332e" d="M-2-6H2V7H-2ZM-5-10H-3V-6H-5ZM3-10H5V-6H3Z" />
              <path fill="#e0bb7e" d="M-1-4H1V4H-1Z" />
            </g>
          </g>
          <g transform="translate(1250 520)"><g className="room-firefly"><path className="room-firefly-halo" d="M-7-3H-3V-7H3V-3H7V3H3V7H-3V3H-7Z" /><path d="M-2-2H2V2H-2Z" /></g></g>
          <g transform="translate(1400 405)"><g className="room-firefly room-firefly-b"><path className="room-firefly-halo" d="M-7-3H-3V-7H3V-3H7V3H3V7H-3V3H-7Z" /><path d="M-2-2H2V2H-2Z" /></g></g>
          <g transform="translate(1140 590)"><g className="room-firefly room-firefly-c"><path className="room-firefly-halo" d="M-7-3H-3V-7H3V-3H7V3H3V7H-3V3H-7Z" /><path d="M-2-2H2V2H-2Z" /></g></g>
        </svg>
      </div>
      <div className="composition-occupant" hidden={!children}>
        <span className="composition-contact-shadow" aria-hidden="true" />
        <div className="composition-cat">{children}</div>
      </div>
    </div>
  </div>;
}
