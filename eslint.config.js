// Web 侧 lint(flat config)。规则集只取 typescript-eslint 与 solid 插件的 recommended,
// 不加风格类规则(格式交给 tsc + 人审)。`pnpm lint` 与 CI lint job 同步跑。
import tseslint from "typescript-eslint";
import solid from "eslint-plugin-solid/configs/typescript";

export default tseslint.config(
  {
    ignores: [
      "**/dist/**",
      "**/node_modules/**",
      "web/packages/ipc-types/src/generated.ts",
      "site/**",
      "e2e/test-results/**",
      "e2e/playwright-report/**",
    ],
  },
  ...tseslint.configs.recommended,
  {
    files: ["web/packages/*/src/**/*.{ts,tsx}"],
    ...solid,
  },
  {
    rules: {
      // 项目里 `_` 前缀表示刻意未用
      "@typescript-eslint/no-unused-vars": [
        "error",
        { argsIgnorePattern: "^_", varsIgnorePattern: "^_", caughtErrorsIgnorePattern: "^_" },
      ],
    },
  },
);
