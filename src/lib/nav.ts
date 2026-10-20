// 侧边栏页面标识。software 保留为兼容导航意图，由 App 归一到本机工具页的兼容渠道区。
export type Tab = "dashboard" | "tools" | "projects" | "ops" | "settings" | "software";

/** 页面跳转回调（由 App 注入各页面） */
export type Navigate = (tab: Tab) => void;
