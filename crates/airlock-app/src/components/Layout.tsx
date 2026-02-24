import type { ReactNode } from 'react';
import { Link, useLocation } from 'react-router-dom';
import { cn } from '@/lib/utils';
import { TooltipProvider } from '@airlock-hq/design-system/react';
import { TelemetryBar } from './TelemetryBar';

interface LayoutProps {
  children: ReactNode;
}

export function Layout({ children }: LayoutProps) {
  const location = useLocation();

  return (
    <TooltipProvider>
      <div className="bg-background relative flex h-screen flex-col antialiased">
        {/* Content */}
        <div className="relative z-10 flex min-h-0 flex-1 flex-col">
          <div className="bg-background/60 flex flex-col">
            {/* Top Navigation Bar */}
            <div className="flex h-16 items-center justify-between px-8">
              {/* Left: Logo */}
              <div className="flex items-center gap-3">
                <span className="text-body text-foreground-muted font-mono tracking-[0.3em] uppercase">Airlock</span>
              </div>

              {/* Right: Navigation */}
              <nav className="flex items-center gap-6">
                <NavTab
                  to="/"
                  active={
                    location.pathname === '/' || location.pathname === '/runs' || location.pathname.includes('/runs/')
                  }
                >
                  Runs
                </NavTab>
                <NavTab
                  to="/repos"
                  active={location.pathname.startsWith('/repos') && !location.pathname.includes('/runs/')}
                >
                  Repositories
                </NavTab>
                <NavTab to="/settings" active={location.pathname === '/settings'}>
                  Settings
                </NavTab>
              </nav>
            </div>

            {/* Telemetry Status Bar */}
            <TelemetryBar />
          </div>
          {/* Main Content */}
          <div className="flex flex-1 overflow-hidden">
            <main className="flex-1 overflow-auto px-8 py-6">{children}</main>
          </div>
        </div>
      </div>
    </TooltipProvider>
  );
}

interface NavTabProps {
  to: string;
  active: boolean;
  children: ReactNode;
}

function NavTab({ to, active, children }: NavTabProps) {
  return (
    <Link
      to={to}
      className={cn(
        'text-small flex items-center transition-colors',
        active ? 'text-foreground font-medium' : 'text-foreground-muted hover:text-foreground'
      )}
    >
      {children}
    </Link>
  );
}
