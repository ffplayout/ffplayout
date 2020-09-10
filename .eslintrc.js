module.exports = {
  root: true,
  env: {
    browser: true,
    node: true
  },
  parserOptions: {
    parser: 'babel-eslint'
  },
  extends: [
    '@nuxtjs',
    'plugin:nuxt/recommended'
  ],
  // add your custom rules here
  rules: {
    'vue/html-indent': ['error', 4],
    'vue/html-closing-bracket-newline': 'off',
    'indent': [2, 4],
    'no-tabs': 'off',
    "no-console": 0,
    "camelcase": ["error", {properties: "never"}]
  }
}
