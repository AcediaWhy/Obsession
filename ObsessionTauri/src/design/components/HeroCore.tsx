import { AnimatePresence, motion } from "framer-motion";
import { useThemeStore, type Theme } from "../../store/themeStore";
import { useMotionOff, useRenderHidden } from "../render";
import { dur, ease } from "../tokens";
import { AuroraCore } from "./AuroraCore";
import { RainCore } from "./RainCore";
import { OphanimCore } from "./OphanimCore";
import { FallenCore } from "./FallenCore";
import { CatnapCore } from "./CatnapCore";
import { MidnightCore } from "./MidnightCore";

type Props = {
  active: boolean;
  busy?: boolean;
  onClick: () => void;
  size?: number;
  // Реактивная телеметрия щита — читает только Ophanim (Великое Око). Остальные
  // ядра игнорируют лишние пропсы; экраны, не знающие про тему, могут их не слать.
  scanning?: boolean;
  alarm?: boolean;
};

// Ядро конкретной темы. Тема — пропом (как в HeroField): при кроссфейде
// уходящая ветка должна держать СТАРУЮ тему, а чтение стора внутри мгновенно
// переключило бы её на новую.
function ThemedCore({ theme, ...props }: Props & { theme: Theme }) {
  if (theme === "ophanim") return <OphanimCore {...props} />;
  if (theme === "fallendown") return <FallenCore {...props} />;
  if (theme === "catnap") return <CatnapCore {...props} />;
  if (theme === "midnight") return <MidnightCore {...props} />;
  // «Rain» (id japan): поверхность воды с расходящейся рябью от капель.
  if (theme === "japan") return <RainCore {...props} />;
  // Aurora — исходное ядро (световые шторы).
  return <AuroraCore {...props} />;
}

// Диспетчер hero-ядра по выбранной теме + кроссфейд при её смене. Все варианты
// имеют одинаковый интерфейс — экраны просто рендерят <HeroCore/> и не знают
// про тему. Контейнер фиксированного размера: во время кроссфейда оба ядра
// лежат стопкой absolute, и макет не дёргается.
export function HeroCore(props: Props) {
  const theme = useThemeStore((s) => s.theme);
  const motionOff = useMotionOff();
  const hidden = useRenderHidden();
  const size = props.size ?? 240; // зеркалит дефолт size всех ядер

  // В трее (suspended) размонтируем canvas-ядро целиком: освобождаем backing
  // store и rAF-цикл. Держим пустой контейнер того же размера — макет не
  // дёргается при возврате, сцена собирается заново на показе.
  if (hidden) return <div style={{ width: size, height: size }} />;

  return (
    <div className="relative" style={{ width: size, height: size }}>
      <AnimatePresence mode="sync" initial={false}>
        <motion.div
          key={theme}
          className="absolute inset-0 flex items-center justify-center"
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          // pointerEvents гасим сразу: уходящее ядро — кнопка, она не должна
          // перехватывать клики поверх входящего.
          exit={{ opacity: 0, pointerEvents: "none" }}
          // dur.slow — как у кроссфейда фона (App): ядро и поле меняют тему
          // ОДНИМ движением, а не вразнобой (раньше 0.3 против 0.42 у фона).
          transition={{ duration: motionOff ? 0 : dur.slow, ease: ease.xfade }}
        >
          <ThemedCore theme={theme} {...props} />
        </motion.div>
      </AnimatePresence>
    </div>
  );
}
