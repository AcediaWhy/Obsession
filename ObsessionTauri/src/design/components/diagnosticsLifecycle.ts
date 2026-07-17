interface MountedFlag {
  current: boolean;
}

export function beginMountedCycle(mounted: MountedFlag): () => void {
  mounted.current = true;
  return () => {
    mounted.current = false;
  };
}
