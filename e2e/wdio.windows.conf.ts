// WebdriverIO 配置(Windows 真 Tauri E2E)
//
// 策略:WebView2 remote debugging + debuggerAddress attach
//   1. 启动 vibeterm.exe 时传 env WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9223
//      → WebView2 控件在启动时开放 DevTools/CDP 端口
//   2. msedgedriver 通过 `ms:edgeOptions.debuggerAddress=127.0.0.1:9223` attach 到那个 WebView2
//
// 这是 Windows 上驱动 Tauri 2 WebView2 的稳定方案
// (tauri-driver 2.0.6 在 msedgedriver 148 上无法注入 useWebView=true,故绕开)

import path from "node:path";
import { fileURLToPath } from "node:url";
import { spawn, type ChildProcess } from "node:child_process";
import type { Options } from "@wdio/types";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(__dirname, "..");

const applicationPath = path.join(repoRoot, "src-tauri", "target", "release", "vibeterm.exe");

let edgeDriver: ChildProcess | null = null;
let tauriApp: ChildProcess | null = null;
let killedManually = false;

const DRIVER_PORT = 4444;
const DEBUG_PORT = 9223;

export const config: Options.Testrunner = {
  runner: "local",
  specs: ["./specs/debug.spec.ts"],
  exclude: [],
  maxInstances: 1,
  capabilities: [
    {
      browserName: "MicrosoftEdge",
      "ms:edgeOptions": {
        debuggerAddress: `127.0.0.1:${DEBUG_PORT}`,
      },
    } as WebdriverIO.Capabilities,
  ],

  logLevel: "info",
  bail: 0,
  baseUrl: "http://localhost",
  waitforTimeout: 10000,
  connectionRetryTimeout: 60000,
  connectionRetryCount: 3,

  framework: "mocha",
  reporters: ["spec"],
  mochaOpts: {
    ui: "bdd",
    timeout: 60000,
  },

  hostname: "127.0.0.1",
  port: DRIVER_PORT,
  path: "/",

  onPrepare: async () => {
    const fs = await import("node:fs");
    if (!fs.existsSync(applicationPath)) {
      console.error(
        `\nFAIL: 找不到 .exe — ${applicationPath}\n` +
          `先运行:pnpm tauri build --no-bundle\n`,
      );
      process.exit(1);
    }

    // 1. start vibeterm.exe with WebView2 remote debugging
    tauriApp = spawn(applicationPath, [], {
      stdio: "inherit",
      env: {
        ...process.env,
        WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${DEBUG_PORT}`,
      },
    });
    tauriApp.on("exit", (code) => {
      if (!killedManually) {
        console.error(`[wdio] vibeterm.exe exited unexpectedly with code ${code}`);
      }
    });

    // 2. wait for the debug port to be ready
    const fetch = (await import("node:http")).default;
    const start = Date.now();
    let ready = false;
    while (Date.now() - start < 30_000) {
      try {
        await new Promise<void>((resolve, reject) => {
          const req = fetch.get(`http://127.0.0.1:${DEBUG_PORT}/json/version`, (res) => {
            if (res.statusCode === 200) {
              resolve();
            } else reject(new Error(`status ${res.statusCode}`));
            res.resume();
          });
          req.on("error", reject);
          req.setTimeout(1000, () => req.destroy(new Error("timeout")));
        });
        ready = true;
        break;
      } catch {
        await new Promise((r) => setTimeout(r, 500));
      }
    }
    if (!ready) {
      console.error(
        `\n[wdio] WebView2 debug port ${DEBUG_PORT} 未在 30s 内 ready —— 检查 WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS 是否生效\n`,
      );
      tauriApp?.kill();
      process.exit(1);
    }
    console.log(`[wdio] WebView2 debug port ${DEBUG_PORT} ready`);

    // 3. start msedgedriver
    edgeDriver = spawn("msedgedriver", [`--port=${DRIVER_PORT}`], {
      stdio: "inherit",
      env: process.env,
      shell: true,
    });
    await new Promise((r) => setTimeout(r, 2000));
  },
  onComplete: () => {
    killedManually = true;
    edgeDriver?.kill();
    tauriApp?.kill();
  },
};
