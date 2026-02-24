/**
 * ESLint rule: no-raw-typography
 *
 * Bans raw Tailwind font-size classes (text-xs, text-sm, text-base, text-lg,
 * text-xl, text-2xl, etc.) in string and template literals. Use the semantic
 * type scale instead (text-micro, text-small, text-body, text-h2, text-h1,
 * text-display).
 */

const RAW_SIZES = /\btext-(?:xs|sm|base|lg|xl|[2-9]xl)\b/g;

const SUGGESTIONS = {
  'text-xs': 'text-micro',
  'text-sm': 'text-small',
  'text-base': 'text-body',
  'text-lg': 'text-body or text-h2',
  'text-xl': 'text-h2',
  'text-2xl': 'text-h2',
  'text-3xl': 'text-h1',
  'text-4xl': 'text-h1 or text-display',
  'text-5xl': 'text-display',
  'text-6xl': 'text-display',
  'text-7xl': 'text-display',
  'text-8xl': 'text-display',
  'text-9xl': 'text-display',
};

/** @type {import('eslint').Rule.RuleModule} */
export default {
  meta: {
    type: 'problem',
    docs: {
      description: 'Disallow raw Tailwind font-size classes in string/template literals',
    },
    messages: {
      rawTypography:
        'Raw Tailwind size class "{{match}}" found. Use a semantic type scale class instead ({{suggestion}}).',
    },
    schema: [],
  },
  create(context) {
    function check(node) {
      const value = node.type === 'TemplateLiteral' ? node.quasis.map((q) => q.value.raw).join('*') : node.value;

      if (typeof value !== 'string') return;

      RAW_SIZES.lastIndex = 0;
      const m = RAW_SIZES.exec(value);
      if (m) {
        context.report({
          node,
          messageId: 'rawTypography',
          data: { match: m[0], suggestion: SUGGESTIONS[m[0]] || 'text-body' },
        });
      }
    }

    return {
      Literal: check,
      TemplateLiteral: check,
    };
  },
};
