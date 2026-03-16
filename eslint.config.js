import pluginVue from "eslint-plugin-vue";
import tseslint from "typescript-eslint";
import eslintConfigPrettier from "eslint-config-prettier";

export default tseslint.config(
  { ignores: ["dist/**", "node_modules/**", "src-tauri/**"] },
  // TS rules first (sets parser globally)
  ...tseslint.configs.recommended,
  // Vue rules after (overrides parser for .vue files with vue-eslint-parser)
  ...pluginVue.configs["flat/recommended"],
  // Disable formatting rules that conflict with Prettier
  eslintConfigPrettier,
  // Use the TS parser as the sub-parser for <script> blocks in .vue files
  {
    files: ["**/*.vue"],
    languageOptions: {
      parserOptions: {
        parser: tseslint.parser,
      },
    },
  },
  {
    rules: {
      "vue/multi-word-component-names": "off",
    },
  },
  // Relax strict TS rules in auto-generated declaration files
  {
    files: ["**/*.d.ts"],
    rules: {
      "@typescript-eslint/no-empty-object-type": "off",
      "@typescript-eslint/no-explicit-any": "off",
    },
  }
);
