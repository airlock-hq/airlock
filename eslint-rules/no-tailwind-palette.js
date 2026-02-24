/**
 * ESLint rule: no-tailwind-palette
 *
 * Bans Tailwind palette classes like `text-red-500`, `bg-zinc-800`, etc.
 * in string and template literals. Use semantic color classes instead
 * (e.g., `text-destructive`, `bg-muted`).
 */

const PALETTE_REGEX =
  /\b(?:text|bg|border|ring|outline|shadow|accent|decoration|divide|from|via|to)-(?:red|orange|amber|yellow|lime|green|emerald|teal|cyan|sky|blue|indigo|violet|purple|fuchsia|pink|rose|slate|gray|zinc|neutral|stone)-\d{2,3}\b/g;

/** @type {import('eslint').Rule.RuleModule} */
export default {
  meta: {
    type: 'problem',
    docs: {
      description: 'Disallow Tailwind palette color classes in string/template literals',
    },
    messages: {
      paletteClass:
        'Tailwind palette class "{{match}}" found. Use a semantic color class instead (e.g., text-destructive, bg-muted).',
    },
    schema: [],
  },
  create(context) {
    function check(node) {
      const value = node.type === 'TemplateLiteral' ? node.quasis.map((q) => q.value.raw).join('*') : node.value;

      if (typeof value !== 'string') return;

      PALETTE_REGEX.lastIndex = 0;
      const m = PALETTE_REGEX.exec(value);
      if (m) {
        context.report({
          node,
          messageId: 'paletteClass',
          data: { match: m[0] },
        });
      }
    }

    return {
      Literal: check,
      TemplateLiteral: check,
    };
  },
};
