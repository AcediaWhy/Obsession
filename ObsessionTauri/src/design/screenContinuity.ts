import { createContext, useContext } from "react";
import type { Tab } from "./components/NavRail";

export type ServiceTab = "dpi" | "telegram";
export const isServiceTab = (tab: Tab): tab is ServiceTab => tab === "dpi" || tab === "telegram";
export const isServicePair = (from: Tab, to: Tab) => from !== to && isServiceTab(from) && isServiceTab(to);

// The screen owns its focus transition. Local entrances must not compete with
// that rhythm or animate a backdrop-containing route ancestor.
export const ScreenContinuity = createContext(false);
export const useScreenContinuity = () => useContext(ScreenContinuity);
