import React, { lazy, Suspense } from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import "./index.css";
import "./styles.css";

// 设计预览入口：`?preview=miuix` 只渲染风格样例，不加载正式界面与后端命令。
const MiuixPreview = lazy(() => import("./components/MiuixPreview"));
const preview = new URLSearchParams(window.location.search).get("preview");

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    {preview === "miuix" ? (
      <Suspense fallback={null}>
        <MiuixPreview />
      </Suspense>
    ) : (
      <App />
    )}
  </React.StrictMode>,
);
