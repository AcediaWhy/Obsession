
import { useBrainStore } from "../store/brainStore";
import { useDpiStore } from "../store/dpiStore";
import { useLegacyReliabilityStore } from "../store/legacyReliabilityStore";
import { useProxyStore } from "../store/proxyStore";
import {
  deriveObsessionVisualPhase,
  type ObsessionVisualPhase,
} from "./obsessionVisualState";

export function useObsessionVisualPhase(): ObsessionVisualPhase {
  const dpiActive = useDpiStore((state) => state.active);
  const dpiTransitioning = useDpiStore((state) => state.transitioning);
  const dpiTesting = useDpiStore((state) => state.testing);
  const dpiError = useDpiStore((state) => state.error);
  const proxyRunning = useProxyStore((state) => state.running);
  const proxyTransitioning = useProxyStore((state) => state.transitioning);
  const proxyError = useProxyStore((state) => state.error);
  const brainPhase = useBrainStore((state) => state.status?.phase ?? null);
  const reliabilityPhase = useLegacyReliabilityStore((state) => state.status.phase);
  const recoveryPhase = useLegacyReliabilityStore(
    (state) => state.status.activeAttempt?.phase ?? null,
  );
  const haltedCategories = useLegacyReliabilityStore(
    (state) => state.status.haltedCategories,
  );

  return deriveObsessionVisualPhase({
    dpi: {
      active: dpiActive,
      transitioning: dpiTransitioning,
      testing: dpiTesting,
      error: dpiError,
    },
    proxy: {
      running: proxyRunning,
      transitioning: proxyTransitioning,
      error: proxyError,
    },
    brain: { phase: brainPhase },
    reliability: {
      phase: reliabilityPhase,
      activeAttemptPhase: recoveryPhase,
      haltedCategories,
    },
  });
}
