import { defineConfig } from 'vite';
import tailwindcss from '@tailwindcss/vite';
import react from '@vitejs/plugin-react';
import path from 'path';

export default defineConfig({
  plugins: [tailwindcss(), react()],
  resolve: {
    alias: [
      { find: '@', replacement: path.resolve(__dirname, './src') },
      {
        find: /^@airlock-hq\/design-system\/react$/,
        replacement: path.resolve(__dirname, '../../packages/design-system/src/react/index.ts'),
      },
      {
        find: /^@airlock-hq\/design-system$/,
        replacement: path.resolve(__dirname, '../../packages/design-system/src/index.ts'),
      },
    ],
    dedupe: [
      'react',
      'react-dom',
      'react/jsx-runtime',
      '@radix-ui/react-dialog',
      '@radix-ui/react-label',
      '@radix-ui/react-scroll-area',
      '@radix-ui/react-select',
      '@radix-ui/react-separator',
      '@radix-ui/react-slot',
      '@radix-ui/react-tabs',
      '@radix-ui/react-tooltip',
    ],
  },
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true,
  },
  build: {
    target: ['es2022', 'chrome100', 'safari16.4'],
    outDir: 'dist',
    chunkSizeWarningLimit: 2000,
  },
});
