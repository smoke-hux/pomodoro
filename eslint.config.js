import js from "@eslint/js";
import jsxA11y from "eslint-plugin-jsx-a11y";
import reactHooks from "eslint-plugin-react-hooks";
import globals from "globals";
import tseslint from "typescript-eslint";

export default tseslint.config(
  {
    ignores: [
      ".agents/**",
      ".codex/**",
      ".playwright-mcp/**",
      "test-artifacts/**",
      "node_modules/**",
      "dist/**",
      "src-tauri/target/**",
      "src-tauri/gen/**",
    ],
  },
  {
    linterOptions: { reportUnusedDisableDirectives: "error" },
  },
  js.configs.recommended,
  ...tseslint.configs.recommended,
  {
    files: ["src/**/*.{ts,tsx}", "public/**/*.js"],
    languageOptions: { globals: globals.browser },
  },
  {
    files: ["*.{js,ts,mjs}", "scripts/**/*.mjs"],
    languageOptions: { globals: globals.node },
  },
  {
    files: ["src/**/*.{ts,tsx}"],
    plugins: { "react-hooks": reactHooks },
    rules: {
      // These correctness rules apply whether or not React Compiler is used.
      "react-hooks/rules-of-hooks": "error",
      "react-hooks/exhaustive-deps": "error",
    },
  },
  {
    ...jsxA11y.flatConfigs.recommended,
    files: ["src/**/*.tsx"],
    rules: {
      ...jsxA11y.flatConfigs.recommended.rules,
      "jsx-a11y/label-has-associated-control": ["error", { depth: 3 }],
    },
  },
);
