/**
 * ESLint rule: no-dark-mode-classes
 *
 * Bans `dark:*` Tailwind classes in string and template literals.
 * Dark mode should be handled via CSS custom properties / semantic tokens,
 * not inline `dark:` prefixes.
 */

const DARK_REGEX = /\bdark:[^\s'"`]+/g;

/** @type {import('eslint').Rule.RuleModule} */
export default {
  meta: {
    type: 'problem',
    docs: {
      description: 'Disallow dark: Tailwind classes in string/template literals',
    },
    messages: {
      darkClass: 'Dark mode class "{{match}}" found. Use semantic design tokens that adapt to dark mode automatically.',
    },
    schema: [],
  },
  create(context) {
    function check(node) {
      const value = node.type === 'TemplateLiteral' ? node.quasis.map((q) => q.value.raw).join('*') : node.value;

      if (typeof value !== 'string') return;

      DARK_REGEX.lastIndex = 0;
      const m = DARK_REGEX.exec(value);
      if (m) {
        context.report({
          node,
          messageId: 'darkClass',
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
