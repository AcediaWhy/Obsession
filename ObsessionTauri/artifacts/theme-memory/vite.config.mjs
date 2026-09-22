import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

// Static diagnostic build: no HMR reload on WebView suspension/resume.
export default defineConfig({
  plugins: [react()],
  define: { __APP_VERSION__: JSON.stringify('tray-probe') },
  build: {
    outDir: 'src-tauri/target/theme-memory-static',
    rollupOptions: { input: ['artifacts/theme-memory/tray.html', 'artifacts/theme-memory/index.html'] },
  },
});
