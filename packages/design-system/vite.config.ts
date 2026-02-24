import { resolve } from 'node:path';
import { defineConfig } from 'vite';
import dts from 'vite-plugin-dts';
import preserveDirectives from 'rollup-plugin-preserve-directives';

export default defineConfig({
  plugins: [
    dts({
      tsconfigPath: './tsconfig.build.json',
      exclude: ['**/*.stories.tsx', '**/*.stories.ts'],
    }),
  ],
  build: {
    lib: {
      entry: {
        index: resolve(__dirname, 'src/index.ts'),
        'react/index': resolve(__dirname, 'src/react/index.ts'),
      },
      formats: ['es'],
    },
    rollupOptions: {
      external: [
        'react',
        'react-dom',
        'react/jsx-runtime',
        /^@radix-ui\//,
        'class-variance-authority',
        'clsx',
        'tailwind-merge',
        'lucide-react',
        'react-resizable-panels',
        /^@fontsource/,
      ],
      output: {
        preserveModules: true,
        preserveModulesRoot: 'src',
      },
      plugins: [preserveDirectives()],
    },
    copyPublicDir: false,
  },
});
