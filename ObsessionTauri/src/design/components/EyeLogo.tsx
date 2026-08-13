import { useEffect, useRef } from "react";

import eyeWebm from "../../assets/eye.webm";
import eyePoster from "../../assets/eye-poster.png";
import { releaseMediaSource, restoreMediaSource } from "../mediaSource";
import { onRenderActiveChange, renderActive, useRenderHidden } from "../render";
import { useSettingsStore } from "../../store/settingsStore";

// Живой глаз-логотип: реалистичный моргающий глаз (vp9-webm с альфой).
// Идентичность Obsession — «вечно наблюдающий глаз».
//
// Фон плитки — ФИКСИРОВАННЫЙ тёмный (не токен темы): у глаза полупрозрачные края,
// и без глухой подложки сквозь них просвечивал бы фон активной темы, подкрашивая
// глаз в голубой/фиолетовый. С тёмной подложкой глаз всегда одинаково мрачный.
//
// Пауза кадров, когда окно скрыто/в трее (renderActive). При reduce_motion —
// статичный постер вместо видео.
export function EyeLogo({ size = 40 }: { size?: number }) {
  const reduceMotion = useSettingsStore((s) => s.settings?.reduce_motion);
  const hidden = useRenderHidden();
  const ref = useRef<HTMLVideoElement>(null);
  // Постер вместо <video> при reduce_motion ИЛИ когда окно скрыто в трей. Второе
  // важно для RAM: EyeLogo смонтирован на КАЖДОМ экране (NavRail), и без этого в
  // трее постоянно жил бы alpha-видеодекодер. Размонтирование <video> дергает
  // teardown ниже и отпускает декодер.
  const off = reduceMotion || hidden;

  useEffect(() => {
    if (off) return;
    const v = ref.current;
    if (!v) return;
    restoreMediaSource(v, eyeWebm);
    const apply = (on: boolean) => {
      if (on) void v.play().catch(() => {});
      else v.pause();
    };
    apply(renderActive());
    const unsub = onRenderActiveChange(apply);
    return () => {
      unsub();
      // Отпускаем видеодекодер при размонтировании/тогле off (тот же teardown,
      // что в VideoField) — иначе detached <video> течёт.
      releaseMediaSource(v);
    };
  }, [off]);

  return (
    <div
      className="shrink-0 overflow-hidden rounded-xl border border-white/10 bg-[#050609]"
      style={{ width: size, height: size }}
    >
      {off ? (
        <img src={eyePoster} alt="Obsession" className="h-full w-full object-cover" />
      ) : (
        <video
          ref={ref}
          src={eyeWebm}
          poster={eyePoster}
          muted
          loop
          playsInline
          autoPlay
          aria-label="Obsession"
          className="h-full w-full object-cover"
        />
      )}
    </div>
  );
}
