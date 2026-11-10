import react from '@vitejs/plugin-react';
import { defineConfig } from 'vite';

export default defineConfig(() => ({
  plugins: [react()],
  // The generated clients drop the program's error messages when NODE_ENV
  // is production; the app shows them, so they stay.
  define: { 'process.env': JSON.stringify({ NODE_ENV: 'development' }) },
  // The app is served from wherever it is hosted, at any path.
  base: './',
  server: { fs: { allow: ['..'] } },
}));
