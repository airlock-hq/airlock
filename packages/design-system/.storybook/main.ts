import type { StorybookConfig } from 'storybook/internal/types';
import tailwindcss from '@tailwindcss/vite';
import path from 'node:path';

const config: StorybookConfig = {
  stories: ['../src/stories/**/*.stories.@(ts|tsx|mdx)'],
  addons: [],
  framework: {
    name: '@storybook/react-vite',
    options: {},
  },
  viteFinal: async (config) => {
    config.resolve = config.resolve || {};
    config.resolve.alias = {
      ...(config.resolve.alias || {}),
      '@': path.resolve(__dirname, '../../crates/airlock-app/src'),
    };
    config.plugins = config.plugins || [];
    config.plugins.push(tailwindcss());
    return config;
  },
};

export default config;
