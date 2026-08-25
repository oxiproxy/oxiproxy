import type { ReactNode } from 'react';
import { Globe2, Layers3, Network, ShieldCheck, Zap } from 'lucide-react';
import type { LucideIcon } from 'lucide-react';
import { Badge } from './ui/badge';
import { Card, CardContent, CardDescription, CardFooter, CardHeader, CardTitle } from './ui/card';
import { Separator } from './ui/separator';

interface AuthShellProps {
  title: string;
  description: string;
  children: ReactNode;
  footer: ReactNode;
}

const features: { icon: LucideIcon; title: string; description: string }[] = [
  { icon: Zap, title: '低延迟连接', description: '基于 QUIC、KCP 与 TCP 的可靠隧道传输' },
  { icon: Globe2, title: '多节点部署', description: '集中管理分布式节点和边缘客户端' },
  { icon: Layers3, title: '清晰的资源视图', description: '代理、配额与流量状态统一呈现' },
];

export default function AuthShell({ title, description, children, footer }: AuthShellProps) {
  return (
    <div className="min-h-screen bg-muted/40">
      <div className="mx-auto grid min-h-screen max-w-7xl lg:grid-cols-[minmax(0,1fr)_460px]">
        <section className="hidden flex-col justify-between bg-primary p-10 text-primary-foreground lg:flex xl:p-16">
          <div>
            <div className="flex items-center gap-3">
              <span className="flex size-10 items-center justify-center rounded-lg bg-primary-foreground/10 ring-1 ring-primary-foreground/20">
                <Network className="size-5" aria-hidden="true" />
              </span>
              <div>
                <p className="text-base font-semibold tracking-tight">OxiProxy</p>
                <p className="text-xs text-primary-foreground/60">内网穿透控制台</p>
              </div>
            </div>
            <div className="mt-24 max-w-xl">
              <p className="text-sm font-medium text-primary-foreground/60">NETWORK CONTROL PLANE</p>
              <h1 className="mt-4 text-4xl font-semibold tracking-tight xl:text-5xl">让内网服务，稳定地连接到外部世界。</h1>
              <p className="mt-5 max-w-lg text-base leading-7 text-primary-foreground/70">用一个简洁的控制台管理客户端、节点和代理，专注于连接本身。</p>
            </div>
            <div className="mt-14 space-y-5">
              {features.map(({ icon: Icon, title: featureTitle, description: featureDescription }) => (
                <div key={featureTitle} className="flex items-start gap-3">
                  <span className="mt-0.5 flex size-8 shrink-0 items-center justify-center rounded-md bg-primary-foreground/10">
                    <Icon className="size-4" aria-hidden="true" />
                  </span>
                  <div>
                    <p className="text-sm font-medium">{featureTitle}</p>
                    <p className="mt-1 text-sm text-primary-foreground/60">{featureDescription}</p>
                  </div>
                </div>
              ))}
            </div>
          </div>
          <div className="flex items-center gap-2 text-xs text-primary-foreground/60">
            <Badge variant="outline" className="border-primary-foreground/20 bg-primary-foreground/10 text-primary-foreground">
              <span className="mr-1.5 size-1.5 rounded-full bg-emerald-300" />服务正常
            </Badge>
            <span>安全加密传输</span>
          </div>
        </section>

        <main className="flex items-center justify-center p-4 sm:p-8">
          <div className="w-full max-w-md">
            <div className="mb-6 flex items-center gap-3 lg:hidden">
              <span className="flex size-9 items-center justify-center rounded-lg bg-primary text-primary-foreground">
                <Network className="size-4" aria-hidden="true" />
              </span>
              <div>
                <p className="text-sm font-semibold">OxiProxy</p>
                <p className="text-xs text-muted-foreground">内网穿透控制台</p>
              </div>
            </div>
            <Card className="shadow-sm">
              <CardHeader className="space-y-1.5 p-6 sm:p-8 sm:pb-6">
                <CardTitle className="text-2xl tracking-tight">{title}</CardTitle>
                <CardDescription>{description}</CardDescription>
              </CardHeader>
              <CardContent className="px-6 sm:px-8">{children}</CardContent>
              <Separator />
              <CardFooter className="flex-col gap-3 p-6 text-center sm:p-8 sm:pt-6">{footer}</CardFooter>
            </Card>
            <p className="mt-6 flex items-center justify-center gap-1.5 text-center text-xs text-muted-foreground">
              <ShieldCheck className="size-3.5" aria-hidden="true" />
              安全登录 · 数据加密传输 · © {new Date().getFullYear()} OxiProxy
            </p>
          </div>
        </main>
      </div>
    </div>
  );
}
