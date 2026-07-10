// Минимальный набор SVG-иконок (stroke-based), чтобы не тянуть иконочную либу.

type P = { size?: number; className?: string };

const base = (size: number, className: string) => ({
  width: size,
  height: size,
  viewBox: "0 0 24 24",
  fill: "none",
  stroke: "currentColor",
  strokeWidth: 1.8,
  strokeLinecap: "round" as const,
  strokeLinejoin: "round" as const,
  className,
});

export const Icon = {
  Bolt: ({ size = 18, className = "" }: P) => (
    <svg {...base(size, className)}>
      <path d="M13 2 3 14h7l-1 8 10-12h-7l1-8Z" />
    </svg>
  ),
  Robot: ({ size = 18, className = "" }: P) => (
    <svg {...base(size, className)}>
      <rect x="4" y="8" width="16" height="12" rx="3" />
      <path d="M12 8V4M9 14h.01M15 14h.01M2 13v3M22 13v3" />
    </svg>
  ),
  Send: ({ size = 18, className = "" }: P) => (
    <svg {...base(size, className)}>
      <path d="m22 2-7 20-4-9-9-4 20-7Z" />
    </svg>
  ),
  List: ({ size = 18, className = "" }: P) => (
    <svg {...base(size, className)}>
      <path d="M8 6h13M8 12h13M8 18h13M3 6h.01M3 12h.01M3 18h.01" />
    </svg>
  ),
  Layers: ({ size = 18, className = "" }: P) => (
    <svg {...base(size, className)}>
      <path d="m12 2 9 5-9 5-9-5 9-5ZM3 12l9 5 9-5M3 17l9 5 9-5" />
    </svg>
  ),
  Settings: ({ size = 18, className = "" }: P) => (
    <svg {...base(size, className)}>
      <circle cx="12" cy="12" r="3" />
      <path d="M19.4 15a1.7 1.7 0 0 0 .3 1.9l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-2.9 1.2 2 2 0 1 1-4 0 1.7 1.7 0 0 0-2.9-1.2l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1A1.7 1.7 0 0 0 4.6 15a1.7 1.7 0 0 0-1.6-1H3a2 2 0 1 1 0-4h.1A1.7 1.7 0 0 0 4.6 9a1.7 1.7 0 0 0-.3-1.9l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1A1.7 1.7 0 0 0 9 4.6a1.7 1.7 0 0 0 1-1.6V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.9-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.9 1.7 1.7 0 0 0 1.6 1H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1Z" />
    </svg>
  ),
  Copy: ({ size = 16, className = "" }: P) => (
    <svg {...base(size, className)}>
      <rect x="9" y="9" width="12" height="12" rx="2" />
      <path d="M5 15V5a2 2 0 0 1 2-2h10" />
    </svg>
  ),
  Check: ({ size = 16, className = "" }: P) => (
    <svg {...base(size, className)}>
      <path d="M20 6 9 17l-5-5" />
    </svg>
  ),
  Refresh: ({ size = 16, className = "" }: P) => (
    <svg {...base(size, className)}>
      <path d="M21 12a9 9 0 1 1-3-6.7L21 8M21 3v5h-5" />
    </svg>
  ),
  Plus: ({ size = 16, className = "" }: P) => (
    <svg {...base(size, className)}>
      <path d="M12 5v14M5 12h14" />
    </svg>
  ),
  Trash: ({ size = 16, className = "" }: P) => (
    <svg {...base(size, className)}>
      <path d="M3 6h18M8 6V4a1 1 0 0 1 1-1h6a1 1 0 0 1 1 1v2M19 6l-1 14a2 2 0 0 1-2 2H8a2 2 0 0 1-2-2L5 6" />
    </svg>
  ),
  X: ({ size = 16, className = "" }: P) => (
    <svg {...base(size, className)}>
      <path d="M18 6 6 18M6 6l12 12" />
    </svg>
  ),
  Info: ({ size = 16, className = "" }: P) => (
    <svg {...base(size, className)}>
      <circle cx="12" cy="12" r="9" />
      <path d="M12 16v-4M12 8h.01" />
    </svg>
  ),
  Alert: ({ size = 16, className = "" }: P) => (
    <svg {...base(size, className)}>
      <path d="M10.3 3.9 1.8 18a2 2 0 0 0 1.7 3h17a2 2 0 0 0 1.7-3L13.7 3.9a2 2 0 0 0-3.4 0Z" />
      <path d="M12 9v4M12 17h.01" />
    </svg>
  ),
  Shield: ({ size = 18, className = "" }: P) => (
    <svg {...base(size, className)}>
      <path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10Z" />
    </svg>
  ),
  Globe: ({ size = 18, className = "" }: P) => (
    <svg {...base(size, className)}>
      <circle cx="12" cy="12" r="9" />
      <path d="M3 12h18M12 3a15 15 0 0 1 0 18M12 3a15 15 0 0 0 0 18" />
    </svg>
  ),
  Eye: ({ size = 18, className = "" }: P) => (
    <svg {...base(size, className)}>
      <path d="M2 12s3.5-7 10-7 10 7 10 7-3.5 7-10 7-10-7-10-7Z" />
      <circle cx="12" cy="12" r="3" />
    </svg>
  ),
};
