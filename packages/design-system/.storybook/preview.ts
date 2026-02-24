import type { Preview } from 'storybook/internal/types';
import '../src/styles/storybook.css';

const preview: Preview = {
  parameters: {
    controls: {
      matchers: {
        color: /(background|color)$/i,
        date: /Date$/i,
      },
    },
    backgrounds: {
      default: 'app',
      values: [{ name: 'app', value: 'hsl(var(--background))' }],
    },
  },
};

export default preview;
