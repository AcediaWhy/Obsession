import mothIcon from "../../../src-tauri/icons/128x128.png";

// Keep the component API stable for NavRail. The static cat needs no decoder.
export function EyeLogo({ size = 40 }: { size?: number }) {
  return (
    <img
      src={mothIcon}
      alt="Obsession"
      draggable={false}
      className="shrink-0 rounded-lg object-contain"
      style={{ width: size, height: size }}
    />
  );
}
