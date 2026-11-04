import { getEnvInfo, type EnvInfo } from "../api";

/** 进程级缓存与环境信息复用。
 *
 * `get_env_info` 每次都要起子进程读 mise/git 版本并探测包管理器；首页、本机工具、软件支持三处
 * 各自在挂载时调一次，切页就会重复采集。这里把并发调用合并成一次，并在会话内复用结果；
 * 用户显式重试或操作后需要重新判定时用 `force` 绕过缓存。
 */
let cached: EnvInfo | null = null;
let inflight: Promise<EnvInfo> | null = null;

export function readEnvInfo(options: { force?: boolean } = {}): Promise<EnvInfo> {
  if (!options.force && cached) return Promise.resolve(cached);
  // 并发调用（切页叠加）合并成同一次采集；正在飞行的请求不再重复发起。
  if (inflight) return inflight;
  inflight = getEnvInfo().then(
    (env) => {
      cached = env;
      inflight = null;
      return env;
    },
    (error: unknown) => {
      inflight = null;
      throw error;
    },
  );
  return inflight;
}

/** 仅测试与显式失效使用。 */
export function clearEnvInfoCache(): void {
  cached = null;
  inflight = null;
}
