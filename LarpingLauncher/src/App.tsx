import { useState, useEffect } from 'react';
import { Shield, Power, Square } from 'lucide-react';
import classNames from 'classnames';

declare global {
  interface Window {
    ipcRenderer?: {
      on(channel: string, listener: (event: unknown, ...args: any[]) => void): () => void;
      off(channel: string, ...args: any[]): void;
      send(channel: string, ...args: any[]): void;
      invoke(channel: string, ...args: any[]): Promise<any>;
    };
  }
}

function App() {
  const [isActive, setIsActive] = useState(false);
  const [selectedCategories, setSelectedCategories] = useState<string[]>(['discord']);
  const [logs, setLogs] = useState<string[]>([]);
  const categoriesList = ['discord', 'youtube_twitch', 'gaming', 'universal'];

  const toggleCategory = (cat: string) => {
    if (isActive) return;
    setSelectedCategories(prev => {
      if (prev.includes(cat)) {
        // Prevent deselecting if it's the last one
        if (prev.length === 1) return prev;
        return prev.filter(c => c !== cat);
      } else {
        return [...prev, cat];
      }
    });
  };

  const handleToggle = () => {
    const newState = !isActive;
    setIsActive(newState);
    
    if (newState) {
      addLog(`Инициализация обхода для: ${selectedCategories.join(', ')}...`);
      window.ipcRenderer?.invoke('start-zapret', selectedCategories).then(() => {
        addLog(`Успешно.`);
      }).catch((err: Error) => {
        addLog(`Ошибка запуска: ${err.message}`);
        setIsActive(false);
      });
    } else {
      window.ipcRenderer?.invoke('stop-zapret').then(() => {
        addLog('Обход остановлен.');
      });
    }
  };

  const addLog = (msg: string) => {
    setLogs(prev => [`[${new Date().toLocaleTimeString('ru-RU')}] ${msg}`, ...prev].slice(0, 100));
  };

  useEffect(() => {
    const unsubscribe = window.ipcRenderer?.on('zapret-log', (_event: unknown, msg: string) => {
      addLog(msg);
    });
    return () => {
      if (unsubscribe) unsubscribe();
    }
  }, []);

  return (
    <div className="flex h-screen text-white font-sans overflow-hidden">
      <div className="absolute top-0 left-0 right-0 h-8" style={{ WebkitAppRegion: 'drag' } as any} />

      <div className="w-64 bg-[#1E293B] border-r border-[#334155] flex flex-col pt-10 pb-4">
        <div className="px-6 mb-8 flex items-center gap-3">
          <div className="w-10 h-10 rounded-xl bg-gradient-to-br from-blue-500 to-violet-600 flex items-center justify-center shadow-lg shadow-blue-500/20">
            <Shield className="w-6 h-6 text-white" />
          </div>
          <div>
            <h1 className="font-bold text-lg leading-tight tracking-tight">Larping</h1>
            <h2 className="text-sm text-blue-400 font-medium leading-none">Launcher</h2>
          </div>
        </div>

        <div className="px-4 space-y-1 flex-1">
          <div className="text-xs font-semibold text-slate-500 uppercase tracking-wider mb-3 px-2">Категории</div>
          <div className="text-[10px] text-slate-500 mb-2 px-2">Можно выбрать несколько</div>
          {categoriesList.map(cat => {
            const isSelected = selectedCategories.includes(cat);
            return (
              <button
                key={cat}
                onClick={() => toggleCategory(cat)}
                disabled={isActive}
                className={classNames(
                  "w-full flex items-center gap-3 px-3 py-2.5 rounded-lg text-sm font-medium transition-all duration-200",
                  isSelected 
                    ? "bg-blue-500/20 text-blue-400 border border-blue-500/30" 
                    : "text-slate-400 hover:bg-[#334155]/50 hover:text-slate-200 border border-transparent",
                  isActive && "opacity-50 cursor-not-allowed"
                )}
              >
                <div className={classNames(
                  "w-2.5 h-2.5 rounded-sm transition-colors",
                  isSelected ? "bg-blue-500" : "bg-[#334155]"
                )} />
                {cat === 'youtube_twitch' ? 'YouTube / Twitch' : cat.charAt(0).toUpperCase() + cat.slice(1)}
              </button>
            );
          })}
        </div>

        <div className="px-6 mt-auto">
          <div className="text-xs text-slate-500 font-medium tracking-wide">made by VlarpSu</div>
        </div>
      </div>

      <div className="flex-1 flex flex-col pt-12 pb-6 px-8 relative">
        <div className="flex justify-between items-start mb-10">
          <div>
            <h2 className="text-3xl font-bold tracking-tight mb-2">Dashboard</h2>
            <p className="text-slate-400 text-sm">Управление обходом блокировок (DPI bypass).</p>
          </div>

          <div className="flex items-center gap-4 bg-[#1E293B] px-4 py-2 rounded-full border border-[#334155]">
            <div className={classNames(
              "w-2.5 h-2.5 rounded-full",
              isActive ? "bg-emerald-400 animate-pulse shadow-[0_0_8px_rgba(52,211,153,0.8)]" : "bg-rose-400"
            )} />
            <span className="text-sm font-medium">
              {isActive ? 'Защита активна' : 'Остановлен'}
            </span>
          </div>
        </div>

        <div className="flex-1 flex flex-col items-center justify-center -mt-10">
          <button
            onClick={handleToggle}
            className={classNames(
              "relative group w-48 h-48 rounded-full flex flex-col items-center justify-center transition-all duration-500 shadow-2xl",
              isActive 
                ? "bg-[#064E3B]/80 border-2 border-emerald-500/50 shadow-emerald-500/20" 
                : "bg-blue-600 border-2 border-blue-400 shadow-blue-500/30 hover:bg-blue-500 hover:scale-105"
            )}
          >
            {isActive ? (
              <>
                <Square className="w-12 h-12 mb-2 text-emerald-400 drop-shadow-md" fill="currentColor" />
                <span className="font-bold text-emerald-400 tracking-wider">STOP</span>
              </>
            ) : (
              <>
                <Power className="w-12 h-12 mb-2 text-white drop-shadow-md" />
                <span className="font-bold text-white tracking-wider">START</span>
              </>
            )}
            
            <div className={classNames(
              "absolute inset-0 rounded-full blur-2xl transition-opacity duration-500 -z-10",
              isActive ? "bg-emerald-500/30 opacity-100" : "bg-blue-500/40 opacity-0 group-hover:opacity-100"
            )} />
          </button>
        </div>

        <div className="h-48 bg-[#0B1120] rounded-xl border border-[#334155] p-4 font-mono text-xs overflow-y-auto mt-auto flex flex-col-reverse shadow-inner">
          {logs.map((log, i) => (
            <div key={i} className={classNames("mb-1 font-medium", log.includes('ERR') ? "text-rose-400" : "text-slate-400")}>{log}</div>
          ))}
          {logs.length === 0 && <div className="text-slate-600 italic m-auto">Ожидание запуска...</div>}
        </div>
      </div>
    </div>
  );
}

export default App;
