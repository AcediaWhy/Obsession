import { motion } from "framer-motion";

// Дрейфующие radial-blur пятна — «живой» aurora-фон без GLSL.
const blobs = [
  { color: "#6366F1", size: 560, top: "-12%", left: "-8%", dur: 26 },
  { color: "#22D3EE", size: 460, top: "38%", left: "62%", dur: 32 },
  { color: "#8B5CF6", size: 420, top: "68%", left: "8%", dur: 30 },
];

export function AuroraBackground() {
  return (
    <div className="pointer-events-none absolute inset-0 overflow-hidden">
      {/* Базовый вертикальный градиент. */}
      <div className="absolute inset-0 bg-gradient-to-b from-base via-base to-[#070810]" />
      {blobs.map((b, i) => (
        <motion.div
          key={i}
          className="absolute rounded-full"
          style={{
            width: b.size,
            height: b.size,
            top: b.top,
            left: b.left,
            background: `radial-gradient(circle at center, ${b.color}, transparent 68%)`,
            filter: "blur(90px)",
            opacity: 0.32,
          }}
          animate={{
            x: [0, 40, -20, 0],
            y: [0, -30, 20, 0],
            scale: [1, 1.12, 0.96, 1],
          }}
          transition={{
            duration: b.dur,
            repeat: Infinity,
            ease: "easeInOut",
          }}
        />
      ))}
      {/* Лёгкая зернистость/виньетка сверху для глубины. */}
      <div className="absolute inset-0 bg-[radial-gradient(ellipse_at_top,transparent_40%,rgba(0,0,0,0.35))]" />
    </div>
  );
}
