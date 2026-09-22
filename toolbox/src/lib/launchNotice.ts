/** 启动提示的 DOM 事件名（`<LaunchNotices />` 订阅它）。 */
export const LAUNCH_NOTICE_EVENT = 'launch-notice';

/** 在任意位置推送一条启动提示（进度/错误），由 `<LaunchNotices />` 统一展示。 */
export function pushLaunchNotice(text: string) {
  window.dispatchEvent(new CustomEvent(LAUNCH_NOTICE_EVENT, { detail: text }));
}
