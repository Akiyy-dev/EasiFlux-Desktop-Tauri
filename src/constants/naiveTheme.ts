import type { GlobalThemeOverrides } from 'naive-ui'

/** Naive UI theme aligned with EasiFlux design tokens. */
export const naiveThemeOverrides: GlobalThemeOverrides = {
  common: {
    fontFamily: 'var(--font-sans)',
    fontFamilyMono: 'var(--font-mono)',
    borderRadius: '8px',
    borderRadiusSmall: '6px',
    primaryColor: '#58a6ff',
    primaryColorHover: '#79b8ff',
    primaryColorPressed: '#388bfd',
  },
  Dialog: {
    borderRadius: '12px',
  },
  Message: {
    borderRadius: '8px',
  },
  Notification: {
    borderRadius: '8px',
  },
}
