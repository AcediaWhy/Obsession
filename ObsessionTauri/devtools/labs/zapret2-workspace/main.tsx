// Development-only visual fixture. No runtime requests or installed settings.
import ReactDOM from "react-dom/client";
import { useEffect, useRef, useState } from "react";
import { DpiScreen } from "../../../src/screens/Dpi";
import { useDpiStore } from "../../../src/store/dpiStore";
import { useSettingsStore } from "../../../src/store/settingsStore";
import { useZapret2StrategyStore } from "../../../src/store/zapret2StrategyStore";
import { THEMES, useThemeStore, type Theme } from "../../../src/store/themeStore";
import { ParallaxProvider } from "../../../src/design/parallax";
import { HeroField } from "../../../src/design/components/HeroField";
import { setWindowShown } from "../../../src/design/render";
import type { Zapret2CatalogProfile, Zapret2PickerResult, Zapret2PickMode } from "../../../src/lib/tauri";
import "../../../src/styles/fonts.css";
import "../../../src/styles/globals.css";

const query = new URLSearchParams(location.search);
const initialTheme = THEMES.find((entry) => entry.id === query.get("theme"))?.id ?? "yanineko";
useThemeStore.setState({ theme: initialTheme });
const categories = ["discord", "youtube_twitch", ...(query.has("gaming") ? ["gaming"] : [])];
const catalog: Zapret2CatalogProfile[] = categories.flatMap((category) => ["tls", "quic", ...(category === "discord" ? ["voice"] : [])]
  .flatMap((transport) => [0, 1].map((index) => ({ category, transport, selected: index === 0,
    profileId: category + "-" + transport + "-" + index,
    name: index === 0 && category === "discord" && transport === "tls" ? "Hostfake + MD5" : "Исходный профиль " + (index + 1) + " · " + (transport === "tls" ? "multidisorder" : "fake"),
    experimental: false, dataPlane: transport === "voice", compositionEligible:category==="discord"&&transport==="tls",level: 4 }))));
const composed: Zapret2CatalogProfile = { category:"discord",transport:"tls",profileId:"dc1_discord-tls-0--discord-tls-1",
  name:"Сборка Discord · API/Gateway + CDN", selected:false, experimental:false,dataPlane:false,level:0,
  composition:{apiProfileId:"discord-tls-0",cdnProfileId:"discord-tls-1"} };
catalog.push(composed);

function result(category: string, transport = "tls"): Zapret2PickerResult {
  const profile = catalog.find((entry) => entry.category === category && entry.transport === transport)!;
  return { category, transport, profileId: profile.profileId, status: "passed", checked: 3, total: 5,
    report: { status: "passed", passed: true, checks: [{ url: "https://example.invalid/fixture", passed: true, attempts: 1,
      bytes: 524288, elapsed_ms: 320, http_status: 200, error: null }] },
    tournament: { runId: "fixture-run", requiredConfirmations: 2, maxAttempts: 5, budgetMs: 180000,
      maxBodyBytes: 2097152, bytesRead: 1572864, elapsedMs: 1800, stopReason: "completed", rounds: [],
      candidates: [{ profileId: profile.profileId, label: profile.name, family: "fixture", fingerprint: "fixture-only",
        rounds: 2, completeRounds: 2, requiredConfirmations: 2, status: "confirmed", bytes: 1048576,
        transferBytesPerSec: 1638400, functions: [{ function: "fixture", label: "Тестовая сетевая проба", required: true,
          passed: 2, failed: 0, inconclusive: 0, cancelled: 0, notTested: 0, proof: "fixture" }] }] } };
}
let mode: Zapret2PickMode = "balanced";
let selectedService = query.get("service") ?? "discord";
let selectedTransport = query.get("transport") ?? "tls";
useSettingsStore.setState({ protectedDpiAvailable: true, saving: false, error: "",
  settings: { zapret2_pick_mode: mode, reduce_motion: query.has("still") } as never,
  patchConfirmed: async (patch) => { mode = patch.zapret2_pick_mode ?? mode;
    useSettingsStore.setState({ settings: { zapret2_pick_mode: mode, reduce_motion: query.has("still") } as never }); return true; } });
useDpiStore.setState({ selectedCategories: categories, config: { categories: ["discord", "youtube_twitch", "gaming"], configs: {}, lists: [] },
  active: true, transitioning: false, testing: false, startedAt: Date.now() - 720000,
  engines: [{ kind: "zapret2", selected: true, available: true, beta: true, version: "0.12.0", name: "Zapret2" },
    { kind: "legacy", selected: false, available: false, beta: false, version: "71", name: "Legacy" }],
  start: async () => { useDpiStore.setState({ active: true }); }, stop: async () => { useDpiStore.setState({ active: false }); },
  toggleCategory: (category) => useDpiStore.setState((s) => ({ selectedCategories: s.selectedCategories.includes(category)
    ? s.selectedCategories.filter((value) => value !== category) : [...s.selectedCategories, category] })) });
useZapret2StrategyStore.setState({ catalog, loading: false, error: "", history: [], historyError: "",
  load: async () => { useZapret2StrategyStore.setState({ catalog }); }, loadHistory: async () => {},
  pick: async (category, transport, _mode, composition) => show(composition ? "composition" : "picking", category, transport),
  cancel: async () => show("cancelled"),
  select: async (category, transport, profileId) => { catalog.forEach((entry) => { if (entry.category === category && entry.transport === transport) entry.selected = entry.profileId === profileId; });
    useZapret2StrategyStore.setState({ catalog: [...catalog], results: {} }); },
  applyResult: async () => show("idle"), testScenario: async (category, transport, profileId, scenario) => {
    useDpiStore.setState({ testing: true });
    useZapret2StrategyStore.setState({ busy: true, operation: "scenario", scenarioProgress: { requestId: "fixture", trialId: "fixture", runId: "fixture-run",
      category, transport, profileId, scenario, phase: "step", stepId: "fixture", stepIndex: 1, total: 2,
      label: "Проверь видео", instruction: "Открой любое видео и включи качество 1080p.", confirmAfterUnixMs: 0, expiresUnixMs: Date.now() + 180000 } });
  }, observeScenario: async () => show("passed") });

function show(state: string, category = selectedService, transport = selectedTransport) {
  selectedService = category; selectedTransport = transport;
  const picking = ["picking", "confirming", "composition", "restoring"].includes(state);
  useDpiStore.setState({ testing: picking });
  const results: Record<string, Zapret2PickerResult> = {};
  if (["passed", "composed", "partial", "cancelled", "tie", "exhausted"].includes(state)) categories.forEach((key) => {
    const value = result(key, key === category ? transport : "tls");
    if (state === "composed" && key === "discord") {
      value.profileId=composed.profileId;
      const winner=value.tournament!.candidates[0];
      value.tournament!.candidates=[{...winner,profileId:"discord-tls-0",label:"Hostfake + MD5",status:"partial",completeRounds:0},
        {...winner,profileId:"discord-tls-1",label:"Разбиение SNI",status:"partial",completeRounds:0},
        {...winner,profileId:composed.profileId,label:composed.name,composition:composed.composition}];
    } else if (!["passed","composed"].includes(state)) { value.status = state as Zapret2PickerResult["status"]; value.profileId = undefined; }
    results[key + ":" + value.transport] = value;
  });
  useZapret2StrategyStore.setState({ busy: picking, operation: picking ? "pick" : null, cancelling: state === "restoring",
    scenarioProgress: null, results, progress: picking ? { category, transport, current: 2, total: 5,
      profileId: "fixture", label: "Тестовый профиль", phase: state === "composition" ? "composition" : state === "confirming" ? "confirmation" : state === "restoring" ? "restoring" : "screening" } : null });
}
show(query.get("state") ?? "idle");

function Lab() {
  const [theme, setTheme] = useState<Theme>(initialTheme);
  const [state, setState] = useState(query.get("state") ?? "idle");
  const [playing, setPlaying] = useState(false);
  const [hidden, setHidden] = useState(false);
  const [still, setStill] = useState(query.has("still"));
  const [fadeReceipt,setFadeReceipt]=useState("");
  const [costReceipt,setCostReceipt]=useState("");
  const timers = useRef<ReturnType<typeof setTimeout>[]>([]);
  const stop = () => { timers.current.forEach(clearTimeout); timers.current = []; setPlaying(false); };
  useEffect(() => () => { timers.current.forEach(clearTimeout); setWindowShown(true); }, []);
  useEffect(() => {
    if (!hidden) return;
    const timer = setTimeout(() => { setHidden(false); setWindowShown(true); }, 2000);
    return () => clearTimeout(timer);
  }, [hidden]);
  useEffect(() => {
    if (!query.has("focus")) return;
    const observer = new MutationObserver(() => {
      const target = document.querySelector(".z2-picker-animation");
      if (target) { target.scrollIntoView({ block: "center" }); observer.disconnect(); }
    });
    observer.observe(document.getElementById("root")!, { childList: true, subtree: true });
    return () => observer.disconnect();
  }, []);
  const nextCandidate = () => {
    const progress = useZapret2StrategyStore.getState().progress;
    if (progress) useZapret2StrategyStore.setState({ progress: { ...progress, current: progress.current + 1,
      total: Math.max(progress.total, progress.current + 1), profileId: "fixture-" + (progress.current + 1), phase: "screening" } });
  };
  const cycle = () => {
    stop(); setPlaying(true); setState("picking"); show("picking");
    const steps: [number, () => void][] = [[2500, nextCandidate], [4900, () => { setState("confirming"); show("confirming"); }],
      [7400, () => { setState("restoring"); show("restoring"); }], [8600, () => { setState("passed"); show("passed"); setPlaying(false); }]];
    timers.current = steps.map(([delay, action]) => setTimeout(action, delay));
  };
  const measureFade = () => {
    stop(); setState("composition"); show("composition"); setFadeReceipt("Замер идёт…");
    timers.current.push(setTimeout(() => {
      setState("composed"); show("composed");
      const started=performance.now();
      const samples:{ms:number;opacity:number|null;height:number|null;exiting:boolean}[]=[];
      const sample=()=>{
        const element=document.querySelector<HTMLElement>(".z2-picker-animation");
        samples.push({ms:Math.round(performance.now()-started),opacity:element?Number(getComputedStyle(element).opacity):null,
          height:element?Number(element.getBoundingClientRect().height.toFixed(2)):null,exiting:element?.dataset.exiting==="true"});
        if(performance.now()-started<6100) timers.current.push(setTimeout(sample,100));
        else setFadeReceipt(JSON.stringify(samples));
      };
      sample();
    },600));
  };
  const measureCards = () => {
    stop(); setState("idle"); show("idle"); setCostReceipt("Замер идёт…");
    const snapshot=()=>[...document.querySelectorAll<HTMLCanvasElement>(".z2-card-dotfield canvas")]
      .map(canvas=>({renderer:canvas.dataset.renderer,frames:Number(canvas.dataset.frames),dots:Number(canvas.dataset.dots),
        width:canvas.width,height:canvas.height,settling:canvas.dataset.settling}));
    timers.current.push(setTimeout(()=>{
      const idleBefore=snapshot();
      timers.current.push(setTimeout(()=>{
        const idleAfter=snapshot();
        const card=document.querySelector<HTMLElement>('[aria-label="Конфигурации Discord"]')!;
        const started=performance.now(),paints:number[]=[];
        const sweep=()=>{
          const elapsed=performance.now()-started,rect=card.getBoundingClientRect();
          if(elapsed<1600) {
            card.dispatchEvent(new PointerEvent("pointermove",{clientX:rect.left+rect.width*(.5+.25*Math.sin(elapsed/600)),
              clientY:rect.top+rect.height*.62,bubbles:true}));
            const canvas=card.querySelector<HTMLCanvasElement>(".z2-card-dotfield canvas");
            if(canvas?.dataset.paintMs)paints.push(Number(canvas.dataset.paintMs));
            timers.current.push(setTimeout(sweep,16));
          } else {
            card.dispatchEvent(new PointerEvent("pointerleave"));
            timers.current.push(setTimeout(()=>{
              const afterLeave=snapshot();
              setCostReceipt(JSON.stringify({idleBefore,idleAfter,afterLeave,
                cpuPaintAverageMs:paints.reduce((sum,value)=>sum+value,0)/Math.max(1,paints.length),
                cpuPaintMaxMs:Math.max(0,...paints),samples:paints.length,scope:"card 2D CPU paint; not whole-app GPU utilization"}));
            },1300));
          }
        };
        sweep();
      },1200));
    },1100));
  };
  return <div data-theme={theme} data-reduce-motion={still} style={{ minHeight: "100vh", padding: "12px 24px", background: "#0a0b10",isolation:"isolate" }}>
    {query.has("backdrop")&&<div style={{position:"fixed",inset:0,zIndex:-1,pointerEvents:"none"}}><HeroField theme={theme} screen="dpi" /></div>}
    <div style={{ position:"relative",display: "flex", flexWrap: "wrap", alignItems: "center", gap: 12, color: "#93a1c2", fontSize: 11, marginBottom: 24 }}>
      <span>Макет · тестовые состояния</span><select aria-label="Состояние макета" value={state} onChange={(event) => { stop(); setState(event.target.value); show(event.target.value); }} style={{ background: "#161d32", padding: 6 }}>
        {["idle", "picking", "confirming", "composition", "restoring", "passed", "composed", "partial", "cancelled", "tie", "exhausted"].map((item) => <option key={item}>{item}</option>)}
      </select>
      <select aria-label="Тема макета" value={theme} onChange={(event) => { const value = event.target.value as Theme; setTheme(value); useThemeStore.setState({ theme:value }); }} style={{ background:"#161d32", padding:6 }}>
        {THEMES.map((entry) => <option key={entry.id} value={entry.id}>{entry.label}</option>)}
      </select>
      <button onClick={playing ? stop : cycle}>{playing ? "Остановить демо" : "Показать цикл"}</button>
      <button onClick={nextCandidate}>Следующий кандидат</button>
      <button onClick={measureFade}>Замерить исчезновение</button>
      <button onClick={measureCards}>Замерить карточки</button>
      <label><input type="checkbox" checked={still} onChange={(event) => { setStill(event.target.checked);
        useSettingsStore.setState((current) => ({ settings: { ...current.settings, reduce_motion: event.target.checked } as never })); }} /> Без движения</label>
      <button disabled={hidden} onClick={() => { setHidden(true); setWindowShown(false); }}>Скрыть окно на 2 с</button>
    </div>
    {fadeReceipt && <output aria-label="Замер исчезновения" style={{display:"block",fontSize:10,color:"#93a1c2",maxHeight:30,overflow:"auto"}}>{fadeReceipt}</output>}
    {costReceipt && <output aria-label="Замер карточек" style={{display:"block",fontSize:10,color:"#93a1c2",maxHeight:30,overflow:"auto"}}>{costReceipt}</output>}
    <ParallaxProvider><main style={{ height: "calc(100vh - 78px)", maxWidth: Number(query.get("width")) || 980, margin: "0 auto" }}><DpiScreen /></main></ParallaxProvider>
  </div>;
}
const root = ReactDOM.createRoot(document.getElementById("root")!);
root.render(<Lab />);
if (import.meta.hot) import.meta.hot.dispose(() => root.unmount());
