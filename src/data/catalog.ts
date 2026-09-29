// 共享目录数据：工具图标映射、运行时清单、分类标签、软件渠道。
// 从 ToolsView.tsx 抽出，供“运行时工具 / 支持列表 / 设置”等页共用；仅数据与纯函数，无 UI。
/** 常见运行时对应的官方 SVG 图标（存放于 public/tools/） */
export const TOOL_ICON_PATHS: Record<string, string> = {
  node: "/tools/nodejs.svg",
  nodejs: "/tools/nodejs.svg",
  npm: "/tools/npm.svg",
  python: "/tools/python.svg",
  java: "/tools/java.svg",
  go: "/tools/go.svg",
  golang: "/tools/go.svg",
  rust: "/tools/rust.svg",
  cargo: "/tools/rust.svg",
  ruby: "/tools/ruby.svg",
  bun: "/tools/bun.svg",
  deno: "/tools/deno.svg",
  kotlin: "/tools/kotlin.svg",
  dotnet: "/tools/dotnet.svg",
  php: "/tools/php.svg",
  elixir: "/tools/elixir.svg",
  erlang: "/tools/erlang.svg",
  haskell: "/tools/haskell.svg",
  lua: "/tools/lua.svg",
  perl: "/tools/perl.svg",
  swift: "/tools/swift.svg",
  flutter: "/tools/flutter.svg",
  dart: "/tools/dart.svg",
  zig: "/tools/zig.svg",
  terraform: "/tools/terraform.svg",
  typescript: "/tools/typescript.svg",
  ts: "/tools/typescript.svg",
  javascript: "/tools/javascript.svg",
  js: "/tools/javascript.svg",
  c: "/tools/c.svg",
  cpp: "/tools/cpp.svg",
  cplusplus: "/tools/cpp.svg",
  csharp: "/tools/csharp.svg",
  cs: "/tools/csharp.svg",
  css: "/tools/css.svg",
  html: "/tools/html.svg",
  bash: "/tools/bash.svg",
  sh: "/tools/bash.svg",
  shell: "/tools/bash.svg",
  zsh: "/tools/bash.svg",
  powershell: "/tools/powershell.svg",
  pwsh: "/tools/powershell.svg",
  r: "/tools/r.svg",
  scala: "/tools/scala.svg",
  julia: "/tools/julia.svg",
  clojure: "/tools/clojure.svg",
  nim: "/tools/nim.svg",
  crystal: "/tools/crystal.svg",
  ocaml: "/tools/ocaml.svg",
  gcc: "/tools/gcc.svg",
  clang: "/tools/clang.svg",
  pnpm: "/tools/pnpm.svg",
  yarn: "/tools/yarn.svg",
  uv: "/tools/uv.svg",
  composer: "/tools/composer.svg",
  helm: "/tools/helm.svg",
  markdown: "/tools/markdown.svg",
  md: "/tools/markdown.svg",
  json: "/tools/json.svg",
  cmake: "/tools/cmake.svg",
  gradle: "/tools/gradle.svg",
  maven: "/tools/maven.svg",
  svelte: "/tools/svelte.svg",
  vue: "/tools/vue.svg",
  react: "/tools/react.svg",
};

/** 常用支持的工具候选（用于展示未安装项）；同时供设置页自检覆盖使用 */
export const KNOWN_TOOLS = [
  // 语言 / 运行时
  "node", "python", "ruby", "go", "rust", "java", "kotlin", "dotnet",
  "php", "swift", "dart", "flutter", "haskell", "lua", "perl", "zig",
  "nim", "crystal", "scala", "julia", "clojure", "c", "cpp", "csharp",
  "r", "v", "ocaml", "typescript", "javascript", "d", "elm", "groovy",
  "haxe", "lisp", "objectivec", "prolog", "racket", "reason", "solidity",
  "pascal", "ada", "applescript", "fortran", "coq", "clojurescript",
  "coffeescript",
  // JS 生态
  "npm", "bun", "deno", "pnpm", "yarn", "nodejs",
  // 构建 / 编译
  "cmake", "gradle", "maven", "make", "ninja", "gcc", "clang", "protoc",
  "cargo", "nix",
  // 包管理 / 开发工具
  "uv", "composer", "gofumpt", "staticcheck", "jq", "yq", "helm", "kustomize",
  "buf", "actionlint", "just", "biome", "volta", "sops", "opentofu", "vault",
  "packer", "pypy", "ipython", "jupyter",
  // Shell
  "bash", "powershell", "shellcheck", "fish", "nu",
  // 配置 / 云 / 数据
  "terraform", "kubernetes", "kubectl", "graphql", "yaml", "toml",
  "curl", "wget",
  // 前端框架 / 标记
  "vue", "react", "svelte", "markdown", "json",
  // 数据库 / 数据服务
  "mysql", "postgresql", "redis", "mongodb", "sqlite", "mariadb",
  "clickhouse", "mssql", "cassandra", "couchdb", "elasticsearch", "influxdb",
  // 云 / DevOps / 容器 / 配置
  "awscli", "gcloud", "az", "gh", "git", "git-lfs", "docker",
  "ansible", "terragrunt", "kubeconform", "k9s", "kind", "k3d",
  "minikube", "helmfile", "argocd", "flux", "istioctl", "consul", "nomad",
  "age", "envsubst",
  // Go 生态工具
  "golangci-lint", "pkl", "air", "mockery", "swag", "gopls", "goimports",
  // Python 生态工具
  "poetry", "pipx", "rye", "pdm", "hatch", "ruff", "pre-commit", "twine",
  "virtualenv", "zizmor",
  // 前端工程与测试
  "eslint", "prettier", "jest", "vitest", "nx", "turbo", "vite", "webpack",
  "rollup", "parcel", "gulp", "babel",
  // 代码质量 / 监控
  "coverage", "sonar", "sentry", "talisman",
  // 更多语言运行时
  "fsharp", "raku", "idris", "pike", "smalltalk", "tcl", "rexx",
  "cobol", "eiffel", "logo",
];

/** 工具分类（支持列表页复用） */
export const TOOL_CATS: Record<string, string[]> = {
  "语言运行时": [
    "node", "python", "ruby", "go", "rust", "java", "kotlin", "dotnet",
    "php", "swift", "dart", "flutter", "haskell", "lua", "perl", "zig",
    "nim", "crystal", "scala", "julia", "clojure", "clojurescript", "c", "cpp",
    "csharp", "r", "v", "ocaml", "typescript", "javascript", "d", "elm",
    "groovy", "haxe", "lisp", "objectivec", "prolog", "racket", "reason",
    "solidity", "pascal", "ada", "applescript", "fortran", "coq", "fsharp",
    "raku", "idris", "pike", "smalltalk", "tcl", "rexx", "cobol", "eiffel",
    "logo",
  ],
  "JS 生态": ["npm", "bun", "deno", "pnpm", "yarn", "nodejs", "coffeescript"],
  "前端框架": ["vue", "react", "svelte"],
  "前端工程": [
    "eslint", "prettier", "jest", "vitest", "nx", "turbo", "vite", "webpack",
    "rollup", "parcel", "gulp", "babel",
  ],
  "构建工具": [
    "cmake", "gradle", "maven", "make", "ninja", "gcc", "clang", "protoc",
    "cargo", "nix", "terraform",
  ],
  "Shell": ["bash", "powershell", "shellcheck", "fish", "nu"],
  "包管理/工具": [
    "uv", "composer", "gofumpt", "staticcheck", "jq", "yq", "helm", "kustomize",
    "buf", "actionlint", "just", "biome", "volta", "sops", "opentofu", "vault",
    "packer", "pypy", "ipython", "jupyter",
  ],
  "数据库/服务": [
    "mysql", "postgresql", "redis", "mongodb", "sqlite", "mariadb",
    "clickhouse", "mssql", "cassandra", "couchdb", "elasticsearch", "influxdb",
  ],
  "云/DevOps": [
    "kubernetes", "kubectl", "graphql", "yaml", "toml", "curl", "wget",
    "awscli", "gcloud", "az", "gh", "git", "git-lfs", "docker", "ansible",
    "terragrunt", "kubeconform", "k9s", "kind", "k3d", "minikube", "helmfile",
    "argocd", "flux", "istioctl", "consul", "nomad", "age", "envsubst",
  ],
  "Go 工具": [
    "golangci-lint", "pkl", "air", "mockery", "swag", "gopls", "goimports",
  ],
  "Python 工具": [
    "poetry", "pipx", "rye", "pdm", "hatch", "ruff", "pre-commit", "twine",
    "virtualenv", "zizmor",
  ],
  "代码质量": ["coverage", "sonar", "sentry", "talisman"],
  "标记/数据": ["markdown", "json"],
};
const CAT_KEYS = Object.keys(TOOL_CATS);

/** 共享标签系统：按 TOOL_CATS 判定工具归属分类；未收录的一律归入“其他”。
 *  供“运行时工具”与“支持列表”两页共用同一套标签。 */
export function categoryOf(name: string): string {
  for (const k of CAT_KEYS) {
    if ((TOOL_CATS[k] || []).includes(name)) return k;
  }
  return "其他";
}
/** 标签列表（含“其他”）；供两页共用 */
export const CAT_KEYS_ALL = [...CAT_KEYS, "其他"];

/** 可托管但非 mise 运行时的软件/服务：分离到“软件渠道”页，并提供官方下载入口 */
export interface DownloadSoft {
  name: string;
  url: string;
  desc?: string;
}
export const SOFTWARE: DownloadSoft[] = [
  { name: "docker", url: "https://docs.docker.com/get-docker/", desc: "容器平台" },
  { name: "kubectl", url: "https://kubernetes.io/docs/tasks/tools/", desc: "Kubernetes CLI" },
  { name: "kind", url: "https://kind.sigs.k8s.io/", desc: "本地 Kubernetes" },
  { name: "k3d", url: "https://k3d.io/", desc: "本地 Kubernetes" },
  { name: "minikube", url: "https://minikube.sigs.k8s.io/docs/start/", desc: "本地 Kubernetes" },
  { name: "k9s", url: "https://k9scli.io/", desc: "K8s 终端 UI" },
  { name: "helmfile", url: "https://github.com/helmfile/helmfile", desc: "Helm 编排" },
  { name: "kubeconform", url: "https://github.com/yannh/kubeconform", desc: "K8s 校验" },
  { name: "argocd", url: "https://argoproj.github.io/cd/", desc: "GitOps CD" },
  { name: "istioctl", url: "https://istio.io/latest/docs/setup/install/", desc: "服务网格 CLI" },
  { name: "consul", url: "https://www.consul.io/downloads", desc: "服务发现" },
  { name: "nomad", url: "https://www.nomadproject.io/downloads", desc: "调度器" },
  { name: "mysql", url: "https://dev.mysql.com/downloads/mysql/", desc: "数据库" },
  { name: "postgresql", url: "https://www.postgresql.org/download/", desc: "数据库" },
  { name: "redis", url: "https://redis.io/download/", desc: "缓存/数据库" },
  { name: "mongodb", url: "https://www.mongodb.com/try/download/community", desc: "文档数据库" },
  { name: "sqlite", url: "https://www.sqlite.org/download.html", desc: "嵌入式数据库" },
  { name: "mariadb", url: "https://mariadb.org/download/", desc: "数据库" },
  { name: "clickhouse", url: "https://clickhouse.com/docs/en/install", desc: "分析数据库" },
  { name: "mssql", url: "https://www.microsoft.com/en-us/sql-server/sql-server-downloads", desc: "关系数据库" },
  { name: "cassandra", url: "https://cassandra.apache.org/_/download.html", desc: "分布式 DB" },
  { name: "couchdb", url: "https://couchdb.apache.org/#download", desc: "文档数据库" },
  { name: "elasticsearch", url: "https://www.elastic.co/downloads/elasticsearch", desc: "搜索/分析" },
  { name: "influxdb", url: "https://www.influxdata.com/downloads/", desc: "时序数据库" },
  { name: "awscli", url: "https://aws.amazon.com/cli/", desc: "AWS CLI" },
  { name: "gcloud", url: "https://cloud.google.com/sdk/docs/install", desc: "Google Cloud CLI" },
  { name: "az", url: "https://learn.microsoft.com/cli/azure/install-azure-cli", desc: "Azure CLI" },
  { name: "gh", url: "https://github.com/cli/cli/releases", desc: "GitHub CLI" },
  { name: "git-lfs", url: "https://git-lfs.com/", desc: "Git 大文件" },
  { name: "age", url: "https://github.com/FiloSottile/age", desc: "文件加密" },
  { name: "envsubst", url: "https://github.com/a8m/envsubst", desc: "环境变量替换" },
  { name: "wget", url: "https://www.gnu.org/software/wget/", desc: "网络下载" },
  { name: "curl", url: "https://curl.se/download.html", desc: "网络传输" },
  { name: "sonar", url: "https://www.sonarsource.com/products/sonarqube/downloads/", desc: "代码质量" },
  { name: "sentry", url: "https://sentry.io/", desc: "错误监控" },
  { name: "talisman", url: "https://github.com/thought-machine/talisman", desc: "秘钥守护" },
  { name: "git", url: "https://git-scm.com/downloads", desc: "版本控制" },
  // ---- 无 mise 版本源、但有官方下载渠道的语言与工具 ----
  { name: "haskell", url: "https://www.haskell.org/downloads/", desc: "GHC 工具链" },
  { name: "nim", url: "https://nim-lang.org/install.html", desc: "语言" },
  { name: "csharp", url: "https://dotnet.microsoft.com/download", desc: ".NET/C# 运行时" },
  { name: "r", url: "https://cran.r-project.org/", desc: "统计语言" },
  { name: "ocaml", url: "https://ocaml.org/install", desc: "语言" },
  { name: "typescript", url: "https://www.typescriptlang.org/download", desc: "TS 编译器" },
  { name: "d", url: "https://dlang.org/download.html", desc: "D 语言" },
  { name: "lisp", url: "https://www.sbcl.org/platform-table.html", desc: "Common Lisp(SBCL)" },
  { name: "objectivec", url: "https://developer.apple.com/xcode/", desc: "Xcode 工具链" },
  { name: "prolog", url: "https://www.swi-prolog.org/download/stable", desc: "SWI-Prolog" },
  { name: "reason", url: "https://reasonml.github.io/", desc: "ReasonML" },
  { name: "pascal", url: "https://www.freepascal.org/download.html", desc: "Free Pascal" },
  { name: "ada", url: "https://www.adacore.com/download", desc: "Ada/GNAT" },
  { name: "fortran", url: "https://gcc.gnu.org/fortran/", desc: "gfortran" },
  { name: "coq", url: "https://coq.inria.fr/download", desc: "证明助手" },
  { name: "clojurescript", url: "https://github.com/clojure/clojurescript", desc: "ClojureScript" },
  { name: "coffeescript", url: "https://coffeescript.org/", desc: "CoffeeScript" },
  { name: "vue", url: "https://vuejs.org/guide/quick-start", desc: "前端框架" },
  { name: "react", url: "https://react.dev/learn/installation", desc: "前端框架" },
  { name: "svelte", url: "https://svelte.dev/docs/quick-start", desc: "前端框架" },
  { name: "eslint", url: "https://eslint.org/docs/latest/use/getting-started", desc: "JS lint" },
  { name: "jest", url: "https://jestjs.io/docs/getting-started", desc: "JS 测试" },
  { name: "vitest", url: "https://vitest.dev/guide/", desc: "前端测试" },
  { name: "nx", url: "https://nx.dev/getting-started/installation", desc: "构建缓存" },
  { name: "vite", url: "https://vitejs.dev/guide/", desc: "构建工具" },
  { name: "webpack", url: "https://webpack.js.org/guides/getting-started", desc: "打包器" },
  { name: "rollup", url: "https://rollupjs.org/guide/en/", desc: "打包器" },
  { name: "parcel", url: "https://parceljs.org/getting-started/webapp/", desc: "打包器" },
  { name: "gulp", url: "https://gulpjs.com/docs/en/getting-started/quick-start", desc: "任务流" },
  { name: "babel", url: "https://babeljs.io/docs/setup/", desc: "JS 转译" },
  { name: "gcc", url: "https://gcc.gnu.org/install/", desc: "编译器" },
  { name: "cargo", url: "https://doc.rust-lang.org/cargo/getting-started/installation.html", desc: "Rust 包管理器" },
  { name: "nix", url: "https://nixos.org/download/", desc: "包管理器" },
  { name: "bash", url: "https://www.gnu.org/software/bash/", desc: "Shell" },
  { name: "fish", url: "https://fishshell.com/", desc: "Shell" },
  { name: "nu", url: "https://www.nushell.sh/install/", desc: "Shell" },
  { name: "composer", url: "https://getcomposer.org/download/", desc: "PHP 依赖" },
  { name: "volta", url: "https://docs.volta.sh/guide/getting-started", desc: "Node 版本管理" },
  { name: "pypy", url: "https://www.pypy.org/download.html", desc: "Python 实现" },
  { name: "ipython", url: "https://ipython.org/install.html", desc: "Python 交互" },
  { name: "jupyter", url: "https://jupyter.org/install", desc: "Notebook" },
  { name: "twine", url: "https://twine.readthedocs.io/en/stable/", desc: "PyPI 上传" },
  { name: "virtualenv", url: "https://virtualenv.pypa.io/en/latest/installation.html", desc: "Python 环境" },
  { name: "gopls", url: "https://pkg.go.dev/golang.org/x/tools/gopls", desc: "Go LSP" },
  { name: "goimports", url: "https://pkg.go.dev/golang.org/x/tools/cmd/goimports", desc: "Go 导入整理" },
  { name: "fsharp", url: "https://dotnet.microsoft.com/languages/fsharp", desc: ".NET/F# 运行时" },
  { name: "raku", url: "https://rakudo.org/files", desc: "Raku" },
  { name: "idris", url: "https://www.idris-lang.org/pages/download.html", desc: "类型化语言" },
  { name: "pike", url: "https://pike.lysator.liu.se/download/", desc: "脚本语言" },
  { name: "smalltalk", url: "https://pharo.org/download", desc: "Pharo/Smalltalk" },
  { name: "tcl", url: "https://www.tcl.tk/software/tcltk/", desc: "Tcl/Tk" },
  { name: "cobol", url: "https://gnucobol.sourceforge.io/", desc: "GnuCOBOL" },
  { name: "eiffel", url: "https://www.eiffel.org/", desc: "EiffelSTudio" },
  { name: "kubernetes", url: "https://kubernetes.io/releases/", desc: "容器编排" },
  { name: "flux", url: "https://fluxcd.io/flux/installation/", desc: "GitOps CLI" },
];
export const SOFT_SET = new Set(SOFTWARE.map((s) => s.name));

// 纯标记/概念（无安装实体），从展示移除，不属运行时也不属软件下载
const HIDDEN_SET = new Set(["markdown", "json", "yaml", "toml", "logo", "coverage"]);

// 实测 `mise ls-remote <n>` 无版本源的运行时/工具，避免在运行时页展示为“可安装”误导
const NO_SRC_SET = new Set([
  "haskell", "nim", "c", "cpp", "csharp", "r", "ocaml", "typescript", "javascript",
  "d", "lisp", "objectivec", "prolog", "reason", "pascal", "ada", "applescript",
  "fortran", "coq", "clojurescript", "coffeescript", "vue", "react", "svelte",
  "eslint", "jest", "vitest", "nx", "vite", "webpack", "rollup", "parcel", "gulp",
  "babel", "gcc", "cargo", "nix", "bash", "fish", "nu", "composer", "volta",
  "pypy", "ipython", "jupyter", "twine", "virtualenv", "gopls", "goimports",
  "fsharp", "raku", "idris", "pike", "smalltalk", "tcl", "rexx", "cobol", "eiffel",
  "kubernetes", "graphql", "flux",
]);

/** 仅运行时集合（供展示与设置页自检；排除软件、纯格式项及实测无源项） */
export const KNOWN_RUNTIMES = KNOWN_TOOLS.filter(
  (n) => !SOFT_SET.has(n) && !HIDDEN_SET.has(n) && !NO_SRC_SET.has(n)
);
