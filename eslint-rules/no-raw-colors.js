/**
 * ESLint rule: no-raw-colors
 *
 * Bans raw color values (#hex, rgb(), rgba(), hsl(), hsla(), oklch()) in
 * string and template literals. Use semantic design tokens instead.
 *
 * Allows `hsl(var(...))` since that references a CSS custom property.
 */

const patterns = [
  { name: 'hex color', regex: /#[0-9a-fA-F]{3,8}\b/g },
  { name: 'rgb()', regex: /\brgba?\(/g },
  { name: 'hsla()', regex: /\bhsla\(/g },
  {
    name: 'hsl() (use hsl(var(...)) or a design token)',
    regex: /\bhsl\((?!\s*var\()/g,
  },
  { name: 'oklch()', regex: /\boklch\(/g },
];

/** @type {import('eslint').Rule.RuleModule} */
export default {
  meta: {
    type: 'problem',
    docs: {
      description: 'Disallow raw color values in string/template literals',
    },
    messages: {
      rawColor: 'Raw {{kind}} found: "{{match}}". Use a semantic design token instead.',
    },
    schema: [],
  },
  create(context) {
    function check(node) {
      const value = node.type === 'TemplateLiteral' ? node.quasis.map((q) => q.value.raw).join('*') : node.value;

      if (typeof value !== 'string') return;

      for (const { name, regex } of patterns) {
        regex.lastIndex = 0;
        const m = regex.exec(value);
        if (m) {
          context.report({
            node,
            messageId: 'rawColor',
            data: { kind: name, match: m[0] },
          });
        }
      }
    }

    return {
      Literal: check,
      TemplateLiteral: check,
    };
  },
};
