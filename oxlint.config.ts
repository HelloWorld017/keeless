import { defineConfig } from 'oxlint';

export default defineConfig({
  plugins: ['eslint', 'typescript', 'oxc', 'import', 'react', 'jsx-a11y'],

  env: {
    browser: true,
    node: true,
    builtin: true,
  },

  options: {
    typeAware: true,
    reportUnusedDisableDirectives: 'warn',
  },

  categories: {
    correctness: 'error',
    suspicious: 'warn',
  },

  rules: {
    'typescript/consistent-type-imports': 'error',
    'typescript/no-misused-promises': 'off',
    'typescript/no-empty-interface': 'off',
    'typescript/no-empty-object-type': ['error', { allowInterfaces: 'with-single-extends' }],
    'typescript/no-non-null-assertion': 'off',
    'typescript/no-unsafe-type-assertion': 'off',
    'no-empty-function': 'off',
    'no-unused-vars': ['warn', { argsIgnorePattern: '^_', ignoreRestSiblings: true }],
    'import/first': 'error',
    'import/newline-after-import': 'error',
    'import/no-duplicates': 'error',
    'import/no-named-as-default-member': 'off',
    'import/no-unassigned-import': 'off',
    'import/prefer-default-export': 'off',

    'react/no-unknown-property': 'off',
    'react/react-in-jsx-scope': 'off',

    'arrow-body-style': ['error', 'as-needed'],
    'eqeqeq': ['error', 'always'],
    'class-methods-use-this': 'off',
    'curly': ['error', 'all'],
    'no-console': ['error', { allow: ['warn', 'error', 'info'] }],
    'prefer-const': 'off',
    'prefer-promise-reject-errors': 'off',
  },

  overrides: [
    {
      files: ['**/*.d.ts'],
      rules: {
        'no-var': 'off',
      },
    },
  ],
});
