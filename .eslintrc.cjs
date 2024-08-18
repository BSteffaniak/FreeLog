module.exports = {
    root: true,
    parser: '@typescript-eslint/parser',
    plugins: ['@typescript-eslint'],
    extends: [
        'eslint:recommended',
        'plugin:@typescript-eslint/recommended',
        'airbnb/base',
        'airbnb-typescript/base',
        'plugin:diff/diff',
        'prettier',
    ],
    parserOptions: {
        project: ['./tsconfig.packages.json'],
    },
    rules: {
        'func-names': 'off',
        'import/prefer-default-export': 'off',
        '@typescript-eslint/no-use-before-define': 'off',
        '@typescript-eslint/ban-ts-comment': 'off',
        'import/no-extraneous-dependencies': [
            'error',
            { devDependencies: true },
        ],
        '@typescript-eslint/naming-convention': [
            'error',
            {
                selector: 'default',
                format: ['camelCase'],
                leadingUnderscore: 'allow',
                trailingUnderscore: 'forbid',
            },
            {
                selector: 'variable',
                format: ['camelCase', 'PascalCase', 'UPPER_CASE'],
                leadingUnderscore: 'allow',
                trailingUnderscore: 'forbid',
            },
            {
                selector: 'typeLike',
                format: ['PascalCase'],
            },
        ],
        'no-unused-vars': 'off', // prefer @typescript-eslint/no-unused-vars
        '@typescript-eslint/no-unused-vars': [
            'error',
            {
                argsIgnorePattern: '^_',
                varsIgnorePattern: '^_',
                caughtErrorsIgnorePattern: '^_',
            },
        ],
    },
    overrides: [
        {
            files: ['**/*.ts'],
            rules: {
                '@typescript-eslint/naming-convention': [
                    'error',
                    {
                        selector: 'objectLiteralProperty',
                        format: null,
                        custom: { regex: '.+', match: true },
                    },
                ],
            },
        },
        {
            files: ['**/sst-env.d.ts', '**/sst.config.ts'],
            rules: {
                '@typescript-eslint/triple-slash-reference': 'off',
            },
        },
    ],
};
