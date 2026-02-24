import js from '@eslint/js';
import globals from 'globals';
import reactHooks from 'eslint-plugin-react-hooks';
import reactRefresh from 'eslint-plugin-react-refresh';
import tseslint from 'typescript-eslint';
import { defineConfig, globalIgnores } from 'eslint/config';
import eslintConfigPrettier from 'eslint-config-prettier/flat';

import noRawColors from './eslint-rules/no-raw-colors.js';
import noTailwindPalette from './eslint-rules/no-tailwind-palette.js';
import noDarkModeClasses from './eslint-rules/no-dark-mode-classes.js';
import noRawTypography from './eslint-rules/no-raw-typography.js';

export default defineConfig([
  globalIgnores([
    '**/dist/',
    '**/node_modules/',
    'target/',
    'packages/design-system/src/tailwind/preset.js',
    'eslint-rules/',
  ]),

  // Base TS/TSX config for all packages
  {
    files: ['**/*.{ts,tsx}'],
    extends: [js.configs.recommended, tseslint.configs.recommended, reactHooks.configs.flat.recommended],
    languageOptions: {
      ecmaVersion: 2020,
      globals: globals.browser,
    },
  },

  // react-refresh only for the app
  {
    files: ['crates/airlock-app/src/**/*.{ts,tsx}'],
    extends: [reactRefresh.configs.vite],
  },

  // shadcn/ui components export both components and variants — this is expected
  {
    files: ['crates/airlock-app/src/components/ui/**/*.{ts,tsx}'],
    rules: {
      'react-refresh/only-export-components': 'off',
    },
  },

  // Design-token custom rules for all JS/TS files
  {
    files: ['**/*.{ts,tsx,js,jsx}'],
    plugins: {
      'design-tokens': {
        rules: {
          'no-raw-colors': noRawColors,
          'no-tailwind-palette': noTailwindPalette,
          'no-dark-mode-classes': noDarkModeClasses,
          'no-raw-typography': noRawTypography,
        },
      },
    },
    rules: {
      'design-tokens/no-raw-colors': 'error',
      'design-tokens/no-tailwind-palette': 'error',
      'design-tokens/no-dark-mode-classes': 'error',
      'design-tokens/no-raw-typography': 'error',
    },
  },

  // Design-system components: re-exports allowed, raw typography allowed
  // (components define their own primitive sizing)
  {
    files: ['packages/design-system/src/**/*.{ts,tsx}'],
    rules: {
      'react-refresh/only-export-components': 'off',
      'design-tokens/no-raw-typography': 'off',
    },
  },

  // Prettier must be last
  eslintConfigPrettier,
]);
