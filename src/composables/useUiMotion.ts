/** Shared UI motion class presets (CSS-driven, Motion-compatible timing). */
export const uiMotion = {
  hover: 'ef-motion-hover',
  press: 'ef-motion-press',
  tab: 'ef-motion-tab',
  sidebar: 'ef-motion-sidebar',
  fade: 'ef-motion-fade',
  page: 'ef-motion-page',
} as const

export type UiMotionPreset = (typeof uiMotion)[keyof typeof uiMotion]
