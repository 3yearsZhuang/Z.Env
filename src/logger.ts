// 简易日志系统：采集 console、全局错误与未处理 Promise 拒绝，供彩蛋页展示。
export interface LogEntry {
  level: "debug" | "info" | "warn" | "error";
  msg: string;
  time: number;
}

const logs: LogEntry[] = [];
const MAX = 300;

function safeString(v: unknown): string {
  try {
    return typeof v === "string" ? v : JSON.stringify(v) ?? String(v);
  } catch {
    return String(v);
  }
}

function push(level: LogEntry["level"], raw: unknown) {
  const entry: LogEntry = {
    level,
    msg: safeString(raw),
    time: Date.now(),
  };
  logs.push(entry);
  if (logs.length > MAX) logs.splice(0, logs.length - MAX);
}

/** 业务方主动写一条日志 */
export function logInfo(msg: string) {
  push("info", msg);
}
export function logWarn(msg: string) {
  push("warn", msg);
}
export function logError(msg: string) {
  push("error", msg);
}

/** 获取当前全部日志（副本） */
export function getLogs(): LogEntry[] {
  return logs.slice();
}

let inited = false;

/** 初始化采集器：包装 console + 注册错误监听。全局只执行一次。 */
export function initLogger() {
  if (inited) return;
  inited = true;

  const orig = {
    log: console.log.bind(console),
    info: console.info.bind(console),
    debug: console.debug.bind(console),
    warn: console.warn.bind(console),
    error: console.error.bind(console),
  };

  console.log = (...a: unknown[]) => {
    orig.log(...a);
    push("info", a.join(" "));
  };
  console.info = (...a: unknown[]) => {
    orig.info(...a);
    push("info", a.join(" "));
  };
  console.debug = (...a: unknown[]) => {
    orig.debug(...a);
    push("debug", a.join(" "));
  };
  console.warn = (...a: unknown[]) => {
    orig.warn(...a);
    push("warn", a.join(" "));
  };
  console.error = (...a: unknown[]) => {
    orig.error(...a);
    push("error", a.join(" "));
  };

  window.addEventListener("error", (e) => {
    push("error", `Error: ${e.message} @ ${e.filename || ""}:${e.lineno || 0}`);
  });
  window.addEventListener("unhandledrejection", (e) => {
    push("error", `Unhandled rejection: ${safeString(e.reason)}`);
  });

  push("info", "日志系统已初始化");
}