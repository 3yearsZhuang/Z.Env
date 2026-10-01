// 预设文件的导出入口：弹保存对话框 + 写文件。
// 「项目配置」与「环境」两页的导出按钮共用。
import { save } from "@tauri-apps/plugin-dialog";
import { presetExport } from "../api";
import { presetFileName } from "./preset";

/** 导出预设为可分享文件；用户取消对话框时返回 null，失败直接抛出由调用方兜底 */
export async function exportPresetFile(
  name: string,
  description: string,
  toml: string,
): Promise<string | null> {
  const target = await save({
    title: "导出环境预设",
    defaultPath: presetFileName(name),
    filters: [{ name: "Z.Env 环境预设", extensions: ["toml"] }],
  });
  if (!target) return null;
  return presetExport(target, name, description, toml);
}
