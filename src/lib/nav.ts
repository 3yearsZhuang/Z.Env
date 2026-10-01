// 侧边栏页面标识。App 与各页面的 onNavigate 跳转回调共用，避免用裸 string 互相漂移。
export type Tab = "dashboard" | "tools" | "projects" | "ops" | "settings" | "software";

/** 页面跳转回调（由 App 注入各页面） */
export type Navigate = (tab: Tab) => void;
