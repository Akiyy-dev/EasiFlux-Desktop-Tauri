import type { ReactNode } from "react";

export interface AppShellScaffoldProps {
  topBar?: ReactNode;
  navigationRail?: ReactNode;
  sidebar?: ReactNode;
  children?: ReactNode;
}

/**
 * Planned App Shell layout scaffold for React migration.
 * Not mounted in production — reference for upcoming PRDs.
 *
 * TopBar → NavigationRail → Sidebar → Main Content
 */
export function AppShellScaffold({
  topBar,
  navigationRail,
  sidebar,
  children,
}: AppShellScaffoldProps) {
  return (
    <div className="flex h-screen flex-col bg-background text-foreground">
      {topBar ? (
        <header className="shrink-0 border-b border-border">{topBar}</header>
      ) : null}
      <div className="flex min-h-0 flex-1">
        {navigationRail ? (
          <nav className="shrink-0 border-r border-border">{navigationRail}</nav>
        ) : null}
        {sidebar ? (
          <aside className="shrink-0 border-r border-border">{sidebar}</aside>
        ) : null}
        <main className="min-w-0 flex-1 overflow-hidden">{children}</main>
      </div>
    </div>
  );
}
