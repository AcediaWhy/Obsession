import { app, BrowserWindow, ipcMain, dialog } from 'electron';
import path from 'node:path';
import { spawn, execSync, ChildProcess } from 'node:child_process';
import fs from 'node:fs';
import { fileURLToPath } from 'node:url';

// В dev глушим security-предупреждения Electron (CSP и т.п.) — в пакете они не показываются
if (!app.isPackaged) {
  process.env.ELECTRON_DISABLE_SECURITY_WARNINGS = 'true';
}

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

// The built directory structure
process.env.DIST = path.join(__dirname, '../dist');
process.env.VITE_PUBLIC = app.isPackaged ? process.env.DIST : path.join(process.env.DIST, '../public');

let win: BrowserWindow | null;
const VITE_DEV_SERVER_URL = process.env['VITE_DEV_SERVER_URL'];

let zapretProcesses: ChildProcess[] = [];

// --- Admin privileges ---

function isAdmin(): boolean {
  try {
    execSync('net session', { windowsHide: true, stdio: 'ignore' });
    return true;
  } catch {
    return false;
  }
}

function relaunchAsAdmin(): void {
  if (app.isPackaged) {
    spawn('powershell', [
      '-Command',
      `Start-Process -FilePath '${app.getPath('exe')}' -Verb RunAs`
    ], { detached: true, stdio: 'ignore', windowsHide: true });
  } else {
    // Dev mode: preserve VITE_DEV_SERVER_URL via temp cmd file
    const electronExe = process.argv[0];
    const args = process.argv.slice(1).map(a => `"${a}"`).join(' ');
    const devServerUrl = process.env.VITE_DEV_SERVER_URL || '';

    const cmdPath = path.join(app.getPath('temp'), 'larping_relaunch.cmd');
    fs.writeFileSync(cmdPath, [
      '@echo off',
      `set "VITE_DEV_SERVER_URL=${devServerUrl}"`,
      `start "" "${electronExe}" ${args}`,
      'exit',
    ].join('\r\n'));

    spawn('powershell', [
      '-Command',
      `Start-Process -FilePath '${cmdPath}' -Verb RunAs -WindowStyle Hidden`
    ], { detached: true, stdio: 'ignore', windowsHide: true });
  }

  app.quit();
}

function createWindow() {
  win = new BrowserWindow({
    icon: path.join(process.env.VITE_PUBLIC, 'electron-vite.svg'),
    width: 900,
    height: 650,
    webPreferences: {
      preload: path.join(__dirname, 'preload.cjs'),
      sandbox: false,         // preload (CJS) требует sandbox:false для доступа к require/ipcRenderer
      contextIsolation: true, // contextBridge работает только с isolation
    },
    titleBarStyle: 'hidden',
    titleBarOverlay: {
      color: '#0F172A',
      symbolColor: '#ffffff',
      height: 30
    }
  });

  // Диагностика рендерера: пробрасываем в лог-панель только реальные ошибки (level 3).
  // Весь dev-шум (vite HMR, React DevTools, security warnings) игнорируем.
  win.webContents.on('console-message', (e) => {
    const d: any = e;
    if (d.level === 3) sendLog(`[renderer] ${d.message}`);
  });
  win.webContents.on('did-fail-load', (_e, code: number, desc: string, url: string) => {
    sendLog(`[did-fail-load] code=${code} desc=${desc} url=${url}`);
  });
  win.webContents.on('render-process-gone', (_e, details: any) => {
    sendLog(`[render-process-gone] ${JSON.stringify(details)}`);
  });

  if (VITE_DEV_SERVER_URL) {
    sendLog(`Загружаю dev-сервер: ${VITE_DEV_SERVER_URL}`);
    win.loadURL(VITE_DEV_SERVER_URL);
  } else {
    sendLog('Загружаю сборку: dist/index.html');
    win.loadFile(path.join(process.env.DIST, 'index.html'));
  }
  if (!app.isPackaged) win.webContents.openDevTools({ mode: 'detach' });
}

app.on('window-all-closed', () => {
  stopZapret().finally(() => {
    if (process.platform !== 'darwin') {
      app.quit();
      win = null;
    }
  });
});

app.whenReady().then(() => {
  if (!isAdmin()) {
    dialog.showMessageBoxSync({
      type: 'warning',
      title: 'Larping Launcher',
      message: 'Требуются права администратора для работы обхода DPI (WinDivert).\nПриложение будет перезапущено.',
      buttons: ['OK']
    });
    relaunchAsAdmin();
    return;
  }
  createWindow();
});

// IPC Handlers

function sendLog(msg: string) {
  if (win && !win.isDestroyed()) {
    win.webContents.send('zapret-log', msg);
  }
  console.log(msg);
}

async function stopZapret() {
  try {
    // Kill ALL winws.exe processes forcefully и ДОЖДАТЬСЯ завершения.
    // Раньше taskkill запускался без ожидания, и следующий spawn(winws.exe)
    // мог быть убит всё ещё работающим taskkill ("гонка").
    const kill = spawn('taskkill', ['/F', '/IM', 'winws.exe'], { windowsHide: true, stdio: 'ignore' });
    await new Promise<void>((resolve) => {
      let done = false;
      const finish = () => { if (!done) { done = true; resolve(); } };
      kill.once('close', finish);
      kill.once('error', finish);
      setTimeout(finish, 2000); // страховка: не висеть вечно
    });
    zapretProcesses = [];
    sendLog('Все процессы winws.exe остановлены.');

    // Flush DNS cache (like Smart Zapret does on exit)
    spawn('ipconfig', ['/flushdns'], { windowsHide: true, stdio: 'ignore' });
    sendLog('DNS кэш очищен.');
  } catch (e: any) {
    sendLog(`Ошибка остановки процессов: ${e.message}`);
  }
}

ipcMain.handle('start-zapret', async (event, categories: string[]) => {
  await stopZapret();
  // короткая пауза чтобы WinDivert полностью освободил порты перед новым захватом
  await new Promise(r => setTimeout(r, 300));

  try {
    const isPackaged = app.isPackaged;
    const basePath = isPackaged ? path.dirname(app.getPath('exe')) : path.join(__dirname, '..');
    const binPath = path.join(basePath, 'bin', 'winws.exe');
    
    if (!fs.existsSync(binPath)) {
      throw new Error(`Ядро winws.exe не найдено по пути ${binPath}`);
    }

    sendLog(`Выбраны категории: ${categories.join(', ')}`);

    for (const category of categories) {
      const categoryPath = path.join(basePath, 'configs', category);
      if (!fs.existsSync(categoryPath)) {
        sendLog(`Папка категории ${category} не найдена, пропускаем.`);
        continue;
      }

      // We prioritize '*_1.conf' as it's the default best config in Smart Zapret
      const files = fs.readdirSync(categoryPath).filter(f => f.endsWith('.conf'));
      if (files.length === 0) {
        sendLog(`Конфиги не найдены в ${category}`);
        continue;
      }

      // Try to find the default one (e.g. discord_1.conf), or just pick the first one
      let selectedConf = files.find(f => f.includes('_1.conf')) || files[0];
      const confPath = path.join(categoryPath, selectedConf);

      sendLog(`Запускаю: ${category} -> ${selectedConf}`);
      
      // Launch winws.exe using the @path syntax which natively reads the .conf file
      // Working directory must be basePath so relative paths like "lists\discord.txt" resolve correctly
      const proc = spawn(binPath, [`@${confPath}`], {
        cwd: basePath,
        windowsHide: true,
      });

      proc.stdout?.on('data', (data) => {
        const lines = data.toString().split(/\r?\n/).map(l => l.trim()).filter(Boolean);
        for (const line of lines) sendLog(`[${category}] ${line}`);
      });

      proc.stderr?.on('data', (data) => {
        const lines = data.toString().split(/\r?\n/).map(l => l.trim()).filter(Boolean);
        for (const line of lines) sendLog(`[${category} ERR] ${line}`);
      });

      proc.on('error', (err) => {
        sendLog(`[${category} FATAL] Не удалось запустить процесс: ${err.message}`);
      });

      const startedAt = Date.now();
      proc.on('close', (code) => {
        const livedMs = Date.now() - startedAt;
        if (livedMs < 2000) {
          sendLog(
            `[${category}] ВНИМАНИЕ: winws умер через ${livedMs}мс после запуска (код ${code}). ` +
            `Обход НЕ работает. Причины: антивирус блокирует winws/WinDivert, конфликт с другим процессом, либо некорректный конфиг.`
          );
        } else {
          sendLog(`Процесс ${category} завершился (код ${code})`);
        }
      });

      zapretProcesses.push(proc);
    }

    if (zapretProcesses.length === 0) {
      throw new Error('Ни один процесс не был запущен. Проверьте конфиги.');
    }

    sendLog('Запуск завершен. Работает процессов: ' + zapretProcesses.length);
    return { success: true };
  } catch (err: any) {
    sendLog(`Ошибка запуска: ${err.message}`);
    throw err;
  }
});

ipcMain.handle('stop-zapret', async () => {
  await stopZapret();
  return { success: true };
});
