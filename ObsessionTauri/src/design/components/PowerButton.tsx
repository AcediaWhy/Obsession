import { motion } from "framer-motion";

type Props = {
  active: boolean;
  busy?: boolean;
  onClick: () => void;
  size?: number;
};

// Крупная неоновая кнопка вкл/выкл с pulse-glow — центральный элемент экрана.
export function PowerButton({ active, busy = false, onClick, size = 168 }: Props) {
  return (
    <div className="relative flex items-center justify-center" style={{ width: size, height: size }}>
      {/* Внешнее свечение. */}
      <motion.div
        className="absolute rounded-full"
        style={{
          width: size,
          height: size,
          background: active
            ? "radial-gradient(circle, rgba(52,211,153,0.45), transparent 70%)"
            : "radial-gradient(circle, rgba(99,102,241,0.35), transparent 70%)",
        }}
        animate={{ scale: active ? [1, 1.15, 1] : 1, opacity: active ? [0.6, 1, 0.6] : 0.5 }}
        transition={{ duration: 2.4, repeat: active ? Infinity : 0, ease: "easeInOut" }}
      />

      <motion.button
        onClick={onClick}
        disabled={busy}
        whileHover={{ scale: 1.04 }}
        whileTap={{ scale: 0.96 }}
        className="no-drag relative flex flex-col items-center justify-center rounded-full disabled:opacity-70"
        style={{
          width: size * 0.82,
          height: size * 0.82,
          background: active
            ? "linear-gradient(145deg, #34D399, #059669)"
            : "linear-gradient(145deg, #6366F1, #8B5CF6)",
          boxShadow: active
            ? "0 0 42px -4px rgba(52,211,153,0.7), inset 0 2px 12px rgba(255,255,255,0.25)"
            : "0 0 42px -6px rgba(99,102,241,0.7), inset 0 2px 12px rgba(255,255,255,0.2)",
        }}
      >
        <PowerIcon size={size * 0.3} spinning={busy} />
        <span className="mt-1 text-xs font-semibold tracking-widest text-white/90">
          {busy ? "..." : active ? "ON" : "OFF"}
        </span>
      </motion.button>
    </div>
  );
}

function PowerIcon({ size, spinning }: { size: number; spinning: boolean }) {
  return (
    <motion.svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="white"
      strokeWidth={2.4}
      strokeLinecap="round"
      animate={spinning ? { rotate: 360 } : { rotate: 0 }}
      transition={spinning ? { duration: 1, repeat: Infinity, ease: "linear" } : {}}
    >
      <path d="M12 3v9" />
      <path d="M6.4 6.4a8 8 0 1 0 11.2 0" />
    </motion.svg>
  );
}
