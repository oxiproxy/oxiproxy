import { useEffect, useRef, useState, type ReactNode } from 'react';
import { Link, useLocation } from 'react-router-dom';
import {
  ArrowLeftRight,
  BarChart3,
  CreditCard,
  LayoutDashboard,
  LogOut,
  Menu,
  Monitor,
  Network,
  Package,
  PanelLeftClose,
  PanelLeftOpen,
  Server,
  Settings,
  ShieldCheck,
  Users,
  X,
} from 'lucide-react';
import type { LucideIcon } from 'lucide-react';
import { useAuth } from '../contexts/AuthContext';
import { Badge } from './ui/badge';
import { Button } from './ui/button';
import { Separator } from './ui/separator';
import { cn } from '../lib/utils';

interface LayoutProps {
  children: ReactNode;
}

interface NavigationItem {
  name: string;
  href: string;
  icon: LucideIcon;
}

const navigation: NavigationItem[] = [
  { name: '仪表板', href: '/', icon: LayoutDashboard },
  { name: '客户端', href: '/clients', icon: Monitor },
  { name: '代理', href: '/proxies', icon: ArrowLeftRight },
  { name: '节点', href: '/nodes', icon: Server },
  { name: '流量统计', href: '/traffic', icon: BarChart3 },
  { name: '我的订阅', href: '/my-subscription', icon: CreditCard },
];

const adminNavigation: NavigationItem[] = [
  { name: '用户管理', href: '/users', icon: Users },
  { name: '订阅套餐', href: '/subscriptions', icon: Package },
  { name: '用户订阅', href: '/user-subscriptions', icon: CreditCard },
  { name: '系统设置', href: '/settings', icon: Settings },
];

function isItemActive(pathname: string, href: string) {
  return href === '/' ? pathname === '/' : pathname.startsWith(href);
}

function Brand({ collapsed = false }: { collapsed?: boolean }) {
  return (
    <Link to="/" className={cn('flex min-w-0 items-center gap-3', collapsed && 'justify-center')}>
      <span className="flex size-9 shrink-0 items-center justify-center rounded-lg bg-primary text-primary-foreground shadow-sm">
        <Network className="size-5" aria-hidden="true" />
      </span>
      {!collapsed && (
        <span className="min-w-0">
          <span className="block truncate text-sm font-semibold tracking-tight">OxiProxy</span>
          <span className="block truncate text-xs text-muted-foreground">NETWORK CONSOLE</span>
        </span>
      )}
    </Link>
  );
}

export default function Layout({ children }: LayoutProps) {
  const { user, logout, isAdmin } = useAuth();
  const location = useLocation();
  const [sidebarCollapsed, setSidebarCollapsed] = useState(false);
  const [sidebarOpen, setSidebarOpen] = useState(false);
  const allNavigation = isAdmin ? [...navigation, ...adminNavigation] : navigation;
  const activeItem = allNavigation.find((item) => isItemActive(location.pathname, item.href));
  const initials = user?.username?.slice(0, 1).toUpperCase() || '?';

  const mainRef = useRef<HTMLElement>(null);
  useEffect(() => {
    mainRef.current?.scrollTo({ top: 0 });
  }, [location.pathname]);

  const closeMobileSidebar = () => setSidebarOpen(false);

  return (
    <div className="console-shell flex h-dvh overflow-hidden bg-background text-foreground">
      {sidebarOpen && (
        <button
          type="button"
          aria-label="关闭导航菜单"
          className="fixed inset-0 z-30 bg-background/80 backdrop-blur-sm md:hidden"
          onClick={closeMobileSidebar}
        />
      )}

      <aside
        className={cn(
          'console-sidebar fixed inset-y-0 left-0 z-40 flex w-72 shrink-0 flex-col overflow-hidden border-r bg-card transition-transform duration-200 md:static md:z-auto md:translate-x-0',
          sidebarCollapsed ? 'md:w-20' : 'md:w-64',
          sidebarOpen ? 'translate-x-0' : '-translate-x-full'
        )}
      >
        <div className={cn('flex h-20 shrink-0 items-center border-b px-5', sidebarCollapsed ? 'justify-center' : 'justify-between')}>
          <Brand collapsed={sidebarCollapsed} />
          <Button
            variant="ghost"
            size="icon"
            className="md:hidden"
            aria-label="关闭导航菜单"
            onClick={closeMobileSidebar}
          >
            <X className="size-4" />
          </Button>
          {!sidebarCollapsed && (
            <Button
              variant="ghost"
              size="icon"
              className="hidden md:inline-flex"
              aria-label="收起导航栏"
              onClick={() => setSidebarCollapsed(true)}
            >
              <PanelLeftClose className="size-4" />
            </Button>
          )}
        </div>

        <nav className="min-h-0 flex-1 space-y-7 overflow-y-auto px-3 py-6" aria-label="主导航">
          <div>
            <p className={cn('mb-2 px-3 text-[11px] font-medium uppercase tracking-wider text-muted-foreground', sidebarCollapsed && 'sr-only')}>
              工作区
            </p>
            <div className="space-y-1">
              {navigation.map((item) => {
                const Icon = item.icon;
                const active = isItemActive(location.pathname, item.href);
                return (
                  <Link
                    key={item.href}
                    to={item.href}
                    onClick={closeMobileSidebar}
                    title={sidebarCollapsed ? item.name : undefined}
                    className={cn(
                      'flex h-10 items-center gap-3 rounded-md px-3 text-sm font-medium transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring',
                      sidebarCollapsed && 'justify-center px-0',
                      active ? 'bg-primary text-primary-foreground shadow-sm' : 'text-muted-foreground hover:bg-accent hover:text-accent-foreground'
                    )}
                    aria-current={active ? 'page' : undefined}
                  >
                    <Icon className="size-4 shrink-0" aria-hidden="true" />
                    {!sidebarCollapsed && <span>{item.name}</span>}
                  </Link>
                );
              })}
            </div>
          </div>

          {isAdmin && (
            <div>
              <Separator className="mb-5" />
              <p className={cn('mb-2 px-3 text-[11px] font-medium uppercase tracking-wider text-muted-foreground', sidebarCollapsed && 'sr-only')}>
                管理
              </p>
              <div className="space-y-1">
                {adminNavigation.map((item) => {
                  const Icon = item.icon;
                  const active = isItemActive(location.pathname, item.href);
                  return (
                    <Link
                      key={item.href}
                      to={item.href}
                      onClick={closeMobileSidebar}
                      title={sidebarCollapsed ? item.name : undefined}
                      className={cn(
                        'flex h-10 items-center gap-3 rounded-md px-3 text-sm font-medium transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring',
                        sidebarCollapsed && 'justify-center px-0',
                        active ? 'bg-primary text-primary-foreground shadow-sm' : 'text-muted-foreground hover:bg-accent hover:text-accent-foreground'
                      )}
                      aria-current={active ? 'page' : undefined}
                    >
                      <Icon className="size-4 shrink-0" aria-hidden="true" />
                      {!sidebarCollapsed && <span>{item.name}</span>}
                    </Link>
                  );
                })}
              </div>
            </div>
          )}
        </nav>

        <div className="shrink-0 border-t p-3">
          <div className={cn('flex items-center gap-3 px-2 py-2', sidebarCollapsed && 'justify-center')}>
            <div className="flex size-8 shrink-0 items-center justify-center rounded-md bg-muted text-sm font-semibold text-foreground ring-1 ring-border">
              {initials}
            </div>
            {!sidebarCollapsed && (
              <div className="min-w-0 flex-1">
                <p className="truncate text-sm font-medium">{user?.username}</p>
                <div className="mt-0.5 flex items-center gap-1.5">
                  <span className="size-1.5 rounded-full bg-emerald-500" aria-hidden="true" />
                  <span className="text-xs text-muted-foreground">{isAdmin ? '管理员' : '已连接'}</span>
                </div>
              </div>
            )}
          </div>
          <Button
            variant="ghost"
            className={cn('mt-1 w-full justify-start gap-3 text-muted-foreground hover:bg-destructive/10 hover:text-destructive', sidebarCollapsed && 'justify-center px-0')}
            onClick={logout}
            title={sidebarCollapsed ? '退出登录' : undefined}
          >
            <LogOut className="size-4" />
            {!sidebarCollapsed && '退出登录'}
          </Button>
        </div>
      </aside>

      <div className="flex min-h-0 min-w-0 flex-1 flex-col">
        <header className="z-20 flex h-16 shrink-0 items-center justify-between border-b bg-card/95 px-4 backdrop-blur sm:px-6 lg:px-8">
          <div className="flex min-w-0 items-center gap-3">
            <Button
              variant="ghost"
              size="icon"
              className="md:hidden"
              aria-label="打开导航菜单"
              onClick={() => setSidebarOpen(true)}
            >
              <Menu className="size-5" />
            </Button>
            {sidebarCollapsed && (
              <Button
                variant="ghost"
                size="icon"
                className="hidden md:inline-flex"
                aria-label="展开导航栏"
                onClick={() => setSidebarCollapsed(false)}
              >
                <PanelLeftOpen className="size-4" />
              </Button>
            )}
            <div className="min-w-0">
              <div className="flex items-center gap-2">
                {activeItem && <activeItem.icon className="hidden size-4 text-muted-foreground sm:block" aria-hidden="true" />}
                <h1 className="truncate text-base font-semibold tracking-tight sm:text-lg">{activeItem?.name || '页面'}</h1>
              </div>
              <p className="hidden text-xs text-muted-foreground sm:block">网络管理 / OxiProxy</p>
            </div>
          </div>
          <div className="flex items-center gap-2 sm:gap-3">
            <div className="hidden items-center gap-2 text-xs text-muted-foreground sm:flex">
              <span className="size-2 rounded-full bg-primary" aria-hidden="true" />
              管理控制台
            </div>
            <Separator orientation="vertical" className="hidden h-6 sm:block" />
            <div className="flex items-center gap-2">
              <div className="flex size-8 items-center justify-center rounded-md bg-primary text-xs font-semibold text-primary-foreground">
                {initials}
              </div>
              <div className="hidden text-sm sm:block">
                <p className="max-w-32 truncate font-medium">{user?.username}</p>
                {isAdmin && <Badge variant="secondary" className="mt-0.5 px-1.5 py-0 text-[10px]"><ShieldCheck className="mr-1 size-3" />管理员</Badge>}
              </div>
            </div>
          </div>
        </header>

        <main ref={mainRef} className="console-main min-h-0 flex-1 overflow-auto p-4 sm:p-6 lg:p-8">
          <div className="mx-auto w-full max-w-[1440px]">{children}</div>
        </main>
      </div>
    </div>
  );
}
