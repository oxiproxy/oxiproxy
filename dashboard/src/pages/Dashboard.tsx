import { useCallback, useEffect, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import {
  ArrowDownToLine,
  ArrowLeftRight,
  ArrowUpFromLine,
  CheckCircle2,
  Gauge,
  Network,
  Server,
  TrendingUp,
  Users,
  Zap,
} from 'lucide-react';
import { useAuth } from '../contexts/AuthContext';
import { dashboardService } from '../lib/services';
import type { DashboardStats } from '../lib/types';
import { formatBytes } from '../lib/utils';
import { DashboardSkeleton } from '../components/Skeleton';
import { Badge } from '../components/ui/badge';
import { Button } from '../components/ui/button';
import { Card, CardContent, CardHeader, CardTitle } from '../components/ui/card';

export default function Dashboard() {
  const { user, isAdmin } = useAuth();
  const navigate = useNavigate();
  const [stats, setStats] = useState<DashboardStats | null>(null);
  const [loading, setLoading] = useState(true);

  const loadStats = useCallback(async () => {
    try {
      setLoading(true);
      const response = await dashboardService.getDashboardStats(user!.id);
      if (response.success && response.data) {
        setStats(response.data);
      }
    } catch (error) {
      console.error('加载统计数据失败:', error);
    } finally {
      setLoading(false);
    }
  }, [user]);

  useEffect(() => {
    if (user) {
      void loadStats();
    }
  }, [loadStats, user]);

  if (loading) {
    return <DashboardSkeleton />;
  }

  return (
    <div className="space-y-6">
      <Card className="overflow-hidden border-primary/20">
        <CardContent className="flex flex-col justify-between gap-6 p-6 sm:flex-row sm:items-center sm:p-8">
          <div className="flex items-start gap-4">
            <div className="flex size-11 shrink-0 items-center justify-center rounded-lg bg-primary text-primary-foreground">
              <Gauge className="size-5" aria-hidden="true" />
            </div>
            <div>
              <div className="flex flex-wrap items-center gap-2">
                <h2 className="text-xl font-semibold tracking-tight sm:text-2xl">欢迎回来，{user?.username}</h2>
                <Badge variant="success"><span className="mr-1.5 size-1.5 rounded-full bg-emerald-600" />运行正常</Badge>
              </div>
              <p className="mt-1 text-sm text-muted-foreground">这是您的 OxiProxy 服务概览，关键资源状态一目了然。</p>
            </div>
          </div>
          <Button variant="outline" className="shrink-0" onClick={() => navigate('/traffic')}>
            <TrendingUp className="size-4" />查看流量
          </Button>
        </CardContent>
      </Card>

      <section aria-labelledby="overview-heading">
        <div className="mb-3 flex items-center justify-between">
          <div>
            <h2 id="overview-heading" className="text-base font-semibold">资源概览</h2>
            <p className="text-sm text-muted-foreground">当前账号下的客户端、代理和配额</p>
          </div>
          <span className="text-xs text-muted-foreground">实时数据</span>
        </div>
        <div className="grid grid-cols-1 gap-3 sm:grid-cols-2 xl:grid-cols-5">
          <StatCard title="总客户端" value={stats?.total_clients || 0} icon={Network} tone="blue" />
          <StatCard title="在线客户端" value={stats?.online_clients || 0} icon={CheckCircle2} tone="green" />
          <StatCard title="总代理" value={stats?.total_proxies || 0} icon={ArrowLeftRight} tone="purple" />
          <StatCard title="启用代理" value={stats?.enabled_proxies || 0} icon={Zap} tone="amber" />
          <StatCard title="用户总配额 (GB)" value={stats?.user_total_quota_gb == null ? '无限制' : stats.user_total_quota_gb.toFixed(2)} icon={TrendingUp} tone="teal" />
        </div>
      </section>

      {isAdmin && (
        <section aria-labelledby="nodes-heading">
          <div className="mb-3 flex items-center justify-between">
            <div>
              <h2 id="nodes-heading" className="text-base font-semibold">节点状态</h2>
              <p className="text-sm text-muted-foreground">查看节点健康度和在线情况</p>
            </div>
            <Button variant="ghost" size="sm" onClick={() => navigate('/nodes')}>管理节点</Button>
          </div>
          <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
            <NodeSummary title="总节点" value={stats?.total_nodes || 0} icon={Server} tone="blue" onClick={() => navigate('/nodes')} />
            <NodeSummary title="在线节点" value={stats?.online_nodes || 0} icon={CheckCircle2} tone="green" onClick={() => navigate('/nodes')} />
          </div>
        </section>
      )}

      <Card>
        <CardHeader className="flex flex-row items-center justify-between space-y-0 border-b bg-muted/20 px-6 py-4">
          <div>
            <CardTitle className="text-base">我的流量统计</CardTitle>
            <p className="mt-1 text-sm text-muted-foreground">累计上传、下载与总流量</p>
          </div>
          <div className="flex size-9 items-center justify-center rounded-md bg-primary/10 text-primary">
            <TrendingUp className="size-4" aria-hidden="true" />
          </div>
        </CardHeader>
        <CardContent className="grid grid-cols-1 gap-3 p-6 sm:grid-cols-3">
          <TrafficStatCard title="上传流量" value={formatBytes(stats?.user_traffic.total_bytes_sent || 0)} icon={ArrowUpFromLine} />
          <TrafficStatCard title="下载流量" value={formatBytes(stats?.user_traffic.total_bytes_received || 0)} icon={ArrowDownToLine} />
          <TrafficStatCard title="总流量" value={formatBytes(stats?.user_traffic.total_bytes || 0)} icon={Users} />
        </CardContent>
      </Card>
    </div>
  );
}

type Tone = 'blue' | 'green' | 'purple' | 'amber' | 'teal';

const toneMap: Record<Tone, { icon: string; border: string }> = {
  blue: { icon: 'bg-blue-500/10 text-blue-600 dark:text-blue-400', border: 'border-blue-500/20' },
  green: { icon: 'bg-emerald-500/10 text-emerald-600 dark:text-emerald-400', border: 'border-emerald-500/20' },
  purple: { icon: 'bg-violet-500/10 text-violet-600 dark:text-violet-400', border: 'border-violet-500/20' },
  amber: { icon: 'bg-amber-500/10 text-amber-600 dark:text-amber-400', border: 'border-amber-500/20' },
  teal: { icon: 'bg-cyan-500/10 text-cyan-600 dark:text-cyan-400', border: 'border-cyan-500/20' },
};

function StatCard({ title, value, icon: Icon, tone }: { title: string; value: number | string; icon: typeof Network; tone: Tone }) {
  const colors = toneMap[tone];
  return (
    <Card className={`border ${colors.border}`}>
      <CardContent className="flex items-center justify-between gap-3 p-5">
        <div className="min-w-0">
          <p className="truncate text-sm text-muted-foreground">{title}</p>
          <p className="mt-2 truncate text-2xl font-semibold tracking-tight">{value}</p>
        </div>
        <div className={`flex size-10 shrink-0 items-center justify-center rounded-md ${colors.icon}`}>
          <Icon className="size-5" aria-hidden="true" />
        </div>
      </CardContent>
    </Card>
  );
}

function NodeSummary({ title, value, icon: Icon, tone, onClick }: { title: string; value: number; icon: typeof Server; tone: Extract<Tone, 'blue' | 'green'>; onClick: () => void }) {
  const colors = toneMap[tone];
  return (
    <Card className={`border ${colors.border}`}>
      <button type="button" onClick={onClick} className="flex w-full items-center justify-between p-5 text-left transition-colors hover:bg-accent/50 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-inset">
        <div>
          <p className="text-sm text-muted-foreground">{title}</p>
          <p className="mt-2 text-2xl font-semibold tracking-tight">{value}</p>
          <p className="mt-1 text-xs text-muted-foreground">点击查看详情</p>
        </div>
        <div className={`flex size-11 items-center justify-center rounded-md ${colors.icon}`}>
          <Icon className="size-5" aria-hidden="true" />
        </div>
      </button>
    </Card>
  );
}

function TrafficStatCard({ title, value, icon: Icon }: { title: string; value: string; icon: typeof ArrowUpFromLine }) {
  return (
    <div className="flex items-center gap-3 rounded-md border bg-muted/20 p-4">
      <div className="flex size-9 shrink-0 items-center justify-center rounded-md bg-primary/10 text-primary">
        <Icon className="size-4" aria-hidden="true" />
      </div>
      <div className="min-w-0">
        <p className="text-xs text-muted-foreground">{title}</p>
        <p className="mt-1 truncate text-lg font-semibold tracking-tight">{value}</p>
      </div>
    </div>
  );
}
