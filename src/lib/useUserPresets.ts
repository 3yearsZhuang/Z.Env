// 用户预设库：存 localStorage，由「项目配置」与「环境」两页共用。
// 两页在 App 里是互斥挂载的（切换 tab 即卸载），因此挂载时读取一次即可保持一致。
import { useCallback, useEffect, useState } from "react";
import { parseStoredPresets, presetToToml, toolsOf, upsertPreset, type UserPreset } from "./preset";
import type { PresetFile } from "../api";

const STORAGE_KEY = "mise-gui:user-presets";

export interface UserPresetsApi {
  /** 当前预设库 */
  userPresets: UserPreset[];
  /** 覆盖写入预设库（同时更新 localStorage） */
  persist: (list: UserPreset[]) => void;
  /** 把当前 mise.toml 文本存为预设，同名覆盖 */
  save: (name: string, content: string, description?: string) => void;
  /** 删除一个预设 */
  remove: (name: string) => void;
  /** 收录一份导入的预设文件；同名时先确认覆盖，取消则返回 false */
  remember: (file: PresetFile) => boolean;
}

export function useUserPresets(): UserPresetsApi {
  const [userPresets, setUserPresets] = useState<UserPreset[]>([]);

  useEffect(() => {
    setUserPresets(parseStoredPresets(localStorage.getItem(STORAGE_KEY)));
  }, []);

  const persist = useCallback((list: UserPreset[]) => {
    setUserPresets(list);
    try {
      localStorage.setItem(STORAGE_KEY, JSON.stringify(list));
    } catch {
      /* 存储满等忽略 */
    }
  }, []);

  const save = useCallback(
    (name: string, content: string, description?: string) => {
      const trimmed = name.trim();
      if (!trimmed) return;
      // 同名覆盖原地替换（卡片不跳位），新名字追加到末尾
      persist(
        upsertPreset(userPresets, {
          name: trimmed,
          content,
          ...(description ? { description } : {}),
        }),
      );
    },
    [persist, userPresets],
  );

  const remove = useCallback(
    (name: string) => {
      persist(userPresets.filter((p) => p.name !== name));
    },
    [persist, userPresets],
  );

  const remember = useCallback(
    (file: PresetFile) => {
      if (
        userPresets.some((p) => p.name === file.name) &&
        !window.confirm(`已存在同名预设「${file.name}」，覆盖它吗？`)
      ) {
        return false;
      }
      persist(
        upsertPreset(userPresets, {
          name: file.name,
          content: presetToToml(toolsOf(file.tools)),
          ...(file.description ? { description: file.description } : {}),
        }),
      );
      return true;
    },
    [persist, userPresets],
  );

  return { userPresets, persist, save, remove, remember };
}
