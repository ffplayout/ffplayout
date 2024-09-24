import withNuxt from './frontend/.nuxt/eslint.config.mjs'

export default withNuxt(
    // your custom flat configs go here, for example:
    // {
    //   files: ['**/*.ts', '**/*.tsx'],
    //   rules: {
    //     'no-console': 'off' // allow console.log in TypeScript files
    //   }
    // },
    // {
    //   ...
    // }
    {
        rules: {
            '@typescript-eslint/no-explicit-any': 'off',
            'no-control-regex': 'off',
            'vue/html-self-closing': 'off',
            'vue/no-v-html': 'off',
        },
    }
)
