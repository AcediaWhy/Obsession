import { AnimatePresence, motion, useIsPresent } from "framer-motion";
import { useThemeStore, type Theme } from "../../store/themeStore";
import { useMotionOff, useRenderHidden } from "../render";
import { dur, ease } from "../tokens";
import { AuroraCore } from "./AuroraCore";
import { GoldenMeadowCore } from "./GoldenMeadowCore";
import { RainBenchCore } from "./RainBenchCore";
import { AlchemistCore } from "./AlchemistCore";
import { FallenCore } from "./FallenCore";
import { CatnapCore } from "./CatnapCore";
import { MidnightCore } from "./MidnightCore";
import { YaniCatCore } from "./YaniCatCore";
import { ObsessionChoirCore } from "./ObsessionChoirCore";

type Props = {
  active: boolean;
  busy?: boolean;
  onClick: () => void;
  size?: number;
  // Реактивная телеметрия: ядра отражают проверку, переход и ошибку защиты.
  scanning?: boolean;
  alarm?: boolean;
  paused?: boolean;
};

// Ядро конкретной темы. Тема — пропом (как в HeroField): при кроссфейде
// уходящая ветка должна держать СТАРУЮ тему, а чтение стора внутри мгновенно
// переключило бы её на новую.
function ThemedCore({ theme, ...props }: Props & { theme: Theme }) {
  const isPresent = useIsPresent();
  const coreProps = { ...props, paused: props.paused || !isPresent };
  if (theme === "goldenmeadow") return <GoldenMeadowCore {...coreProps} />;
  if (theme === "obsession") return <ObsessionChoirCore {...coreProps} />;
  if (theme === "ophanim") return <AlchemistCore {...coreProps} />;
  if (theme === "fallendown") return <FallenCore {...coreProps} />;
  if (theme === "catnap") return <CatnapCore {...coreProps} />;
  if (theme === "midnight") return <MidnightCore {...coreProps} />;
  // «Yani Neko»: самостоятельная пиксельная кошка отражает состояние защиты.
  if (theme === "yanineko") return <YaniCatCore {...coreProps} />;
  // «Rain» (id japan): свернувшийся кот под старым пиксельным зонтом.
  if (theme === "japan") return <RainBenchCore {...coreProps} />;
  // Aurora — исходное ядро (световые шторы).
  return <AuroraCore {...coreProps} />;
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
          // Уходящее ядро не должно перехватывать клики по новому.
          exit={{ opacity: 0, pointerEvents: "none" }}
          // Синхронизируем переход ядра с переходом фоновой сцены.
          transition={{ duration: motionOff ? 0 : dur.slow, ease: ease.xfade }}
        >
          <ThemedCore theme={theme} {...props} />
        </motion.div>
      </AnimatePresence>
    </div>
  );
}
