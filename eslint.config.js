import js from "@eslint/js";
import globals from "globals";
import tseslint from "typescript-eslint";

export default tseslint.config(
  { ignores: ["dist/", "src-tauri/"] },
  js.configs.recommended,
  ...tseslint.configs.recommended,
  {
    // Node 侧脚本（版本一致性校验等）与本配置文件使用 node 全局
    files: ["scripts/**/*.mjs", "eslint.config.js"],
    languageOptions: { globals: { ...globals.node } },
  },
  {
    languageOptions: {
      globals: { ...globals.browser },
    },
    rules: {
      // 下划线前缀视为有意忽略的参数/变量
      "@typescript-eslint/no-unused-vars": [
        "error",
        { argsIgnorePattern: "^_", varsIgnorePattern: "^_" },
      ],
    },
  }
);
