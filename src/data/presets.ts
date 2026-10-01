// 内置技术栈环境预设：一键生成全套运行环境。
// 从 ProjectsView 抽出，供「项目配置」（装到项目）与「环境」（装到整机）两页共用；
// 仅数据，无 UI。
export interface BuiltinPreset {
  id: string;
  name: string;
  desc: string;
  tools: Record<string, string>;
}

export const PRESETS: BuiltinPreset[] = [
  {
    id: "node",
    name: "Node 前端",
    desc: "React / Vite / 前端工程",
    tools: { node: "20", typescript: "latest", bun: "latest", deno: "latest" },
  },
  {
    id: "python",
    name: "Python 数据",
    desc: "数据分析 / 脚本 / AI",
    tools: { python: "3.13", uv: "latest" },
  },
  {
    id: "java",
    name: "Java 后端",
    desc: "Spring Boot / JVM",
    tools: { java: "temurin-21", maven: "latest", gradle: "latest", kotlin: "latest" },
  },
  {
    id: "go",
    name: "Go 服务",
    desc: "微服务 / CLI",
    tools: { go: "1.27", gofumpt: "latest", staticcheck: "latest" },
  },
  {
    id: "ruby",
    name: "Ruby",
    desc: "Rails / 脚本",
    tools: { ruby: "4.0", gem: "latest", bundler: "latest" },
  },
  {
    id: "fullstack",
    name: "全栈",
    desc: "Node + Python + Go",
    tools: { node: "20", python: "3.13", go: "1.27", java: "temurin-21" },
  },
];
