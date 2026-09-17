import { useEffect, useRef, useState } from "react";
import { createBlackCatRig, type BlackCatRig } from "../../../labs/blackCatRig.js";
import type { RenderLoop } from "../../render";
import "../../../styles/blackCatIdle.css";

/** The approved lab idle; other poses stay in BlackPondCatSprite as WebP. */
export function BlackCatIdle({ size }: { size: number }) {
  const hostRef = useRef<HTMLSpanElement>(null);
  const [ready, setReady] = useState(false);
  const source = `${import.meta.env.BASE_URL}lab-assets/black-cat-states/idle-256-still.webp`;
  const role = size >= 160 ? "hero" : "preview";

  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;
    let disposed = false;
    let rig: BlackCatRig | undefined;
    let loop: RenderLoop | undefined;
    let time = 0;
    setReady(false);
    const image = new Image();
    image.src = source;
    // The scheduler touches window at import time; load it only on the client.
    void Promise.all([image.decode(), import("../../render")]).then(([, { createRenderLoop }]) => {
      if (disposed) return;
      rig = createBlackCatRig(host, image);
      rig.render(0, { neutral: true });
      loop = createRenderLoop((dt) => {
        time += dt;
        rig?.render(time);
      }, { role, fps: 60 });
      loop.start();
      setReady(true);
    }).catch(() => {
      // A decoding/canvas failure must leave the original still visible.
      loop?.dispose();
      rig?.dispose();
    });
    return () => {
      disposed = true;
      loop?.dispose();
      rig?.dispose();
    };
  }, [source, role]);

  return (
    <span
      aria-hidden="true"
      className="black-pond-cat-sprite black-cat-idle"
      data-black-pond-cat-sprite="true"
      data-motion="running"
      data-phase="idle"
      data-idle-renderer="layered"
      data-rig-ready={ready}
      style={{ height: size, width: size }}
    >
      <img className="black-cat-idle-fallback" alt="" src={source} draggable={false} />
      <span className="black-cat-idle-rig" ref={hostRef} />
    </span>
  );
}
