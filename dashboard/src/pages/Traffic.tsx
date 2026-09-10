import { useEffect, useState, useRef } from 'react';
import { useAuth } from '../contexts/AuthContext';
import { trafficService } from '../lib/services';
import type { TrafficOverview } from '../lib/types';
import { formatBytes, formatShortDate } from '../lib/utils';
import SkeletonBlock, { CardSkeleton } from '../components/Skeleton';
import {
  TableContainer,
  Table,
  TableHeader,
  TableBody,
  TableHead,
  TableRow,
  TableCell,
} from '../components/ui/table';

export default function Traffic() {
  const { user } = useAuth();
  const [traffic, setTraffic] = useState<TrafficOverview | null>(null);
  const [days, setDays] = useState(30);
  const [loading, setLoading] = useState(true);
  const [autoRefresh, setAutoRefresh] = useState(true);
  const [refreshInterval, setRefreshInterval] = useState(5); // 默认5秒刷新
  const intervalRef = useRef<ReturnType<typeof setInterval> | null>(null);

  useEffect(() => {
    loadTraffic();
  }, [days]);

  useEffect(() => {
    // 清除之前的定时器
    if (intervalRef.current) {
      clearInterval(intervalRef.current);
    }

    // 如果启用了自动刷新，设置新的定时器
    if (autoRefresh) {
      intervalRef.current = setInterval(() => {
        loadTrafficSilently();
      }, refreshInterval * 1000);
    }

    // 清理函数
    return () => {
      if (intervalRef.current) {
        clearInterval(intervalRef.current);
      }
    };
  }, [autoRefresh, refreshInterval, days]);

  const loadTraffic = async () => {
    try {
      setLoading(true);
      const response = user?.is_admin
        ? await trafficService.getTrafficOverview(days)
        : await trafficService.getUserTraffic(user!.id, days);

      if (response.success && response.data) {
        setTraffic(response.data);
      }
    } catch (error) {
      console.error('加载流量统计失败:', error);
    } finally {
      setLoading(false);
    }
  };

  const loadTrafficSilently = async () => {
    try {
      const response = user?.is_admin
        ? await trafficService.getTrafficOverview(days)
        : await trafficService.getUserTraffic(user!.id, days);

      if (response.success && response.data) {
        setTraffic(response.data);
      }
    } catch (error) {
      console.error('刷新流量统计失败:', error);
    }
  };

  if (loading) {
    return (
      <div className="space-y-6">
        <div className="flex flex-wrap items-center justify-between gap-4">
          <div className="space-y-2">
            <SkeletonBlock className="h-8 w-32" />
            <SkeletonBlock className="h-4 w-48" />
          </div>
          <SkeletonBlock className="h-10 w-36 rounded-xl" />
        </div>
        <div className="bg-card rounded-2xl shadow-sm border border-border p-6">
          <SkeletonBlock className="h-5 w-24 mb-4" />
          <div className="grid grid-cols-1 gap-5 sm:grid-cols-3">
            <CardSkeleton />
            <CardSkeleton />
            <CardSkeleton />
          </div>
        </div>
        <div className="bg-card rounded-2xl shadow-sm border border-border p-6 space-y-3">
          <SkeletonBlock className="h-5 w-28 mb-4" />
          {Array.from({ length: 5 }).map((_, i) => (
            <div key={i} className="flex items-center gap-4">
              <SkeletonBlock className="h-4 w-20" />
              <SkeletonBlock className="h-8 flex-1 rounded-full" />
            </div>
          ))}
        </div>
      </div>
    );
  }

  return (
    <div className="space-y-6">
      {/* 页面标题和控制 */}
      <div className="flex flex-wrap items-center justify-between gap-4">
        <div>
          <h2 className="text-2xl font-bold text-foreground">流量统计</h2>
          <p className="mt-1 text-sm text-muted-foreground">查看流量使用情况和趋势分析</p>
        </div>
        <div className="flex items-center gap-4">
          {/* 自动刷新控制 */}
          <div className="flex items-center gap-3 bg-card px-4 py-2 rounded-xl border border-border">
            <label className="relative inline-flex items-center cursor-pointer">
              <input
                type="checkbox"
                checked={autoRefresh}
                onChange={(e) => setAutoRefresh(e.target.checked)}
                className="sr-only peer"
              />
              <div className="w-10 h-5 bg-muted peer-focus:outline-none peer-focus:ring-4 peer-focus:ring-primary/50 rounded-full peer peer-checked:after:translate-x-full rtl:peer-checked:after:-translate-x-full peer-checked:after:border-white after:content-[''] after:absolute after:top-[2px] after:start-[2px] after:bg-card after:border-border after:border after:rounded-full after:h-4 after:w-4 after:transition-all peer-checked:bg-primary"></div>
              <span className="ms-2 text-sm font-medium text-foreground">自动刷新</span>
            </label>

            {autoRefresh && (
              <select
                value={refreshInterval}
                onChange={(e) => setRefreshInterval(Number(e.target.value))}
                className="px-2 py-1 text-sm border border-border rounded-lg focus:outline-none focus:ring-2 focus:ring-primary/20 focus:border-primary"
              >
                <option value={3}>3秒</option>
                <option value={5}>5秒</option>
                <option value={10}>10秒</option>
                <option value={30}>30秒</option>
              </select>
            )}
          </div>

          <select
            value={days}
            onChange={(e) => setDays(Number(e.target.value))}
            className="px-4 py-2.5 border border-border rounded-xl focus:outline-none focus:ring-2 focus:ring-primary/20 focus:border-primary bg-card text-sm font-medium text-foreground"
          >
            <option value={7}>最近 7 天</option>
            <option value={30}>最近 30 天</option>
            <option value={90}>最近 90 天</option>
          </select>
        </div>
      </div>

      {/* 总流量统计 */}
      <div className="bg-card rounded-2xl shadow-sm border border-border overflow-hidden">
        <div className="px-6 py-4 border-b border-border">
          <h3 className="text-lg font-semibold text-foreground">总流量统计</h3>
        </div>
        <div className="p-6">
          <div className="grid grid-cols-1 gap-5 sm:grid-cols-3">
            <TrafficCard
              title="总上传"
              value={formatBytes(traffic?.total_traffic.total_bytes_sent || 0)}
              icon={
                <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" strokeWidth={1.5} stroke="currentColor" className="w-8 h-8">
                  <path strokeLinecap="round" strokeLinejoin="round" d="M4.5 10.5L12 3m0 0l7.5 7.5M12 3v18" />
                </svg>
              }
              color="blue"
            />
            <TrafficCard
              title="总下载"
              value={formatBytes(traffic?.total_traffic.total_bytes_received || 0)}
              icon={
                <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" strokeWidth={1.5} stroke="currentColor" className="w-8 h-8">
                  <path strokeLinecap="round" strokeLinejoin="round" d="M19.5 13.5L12 21m0 0l-7.5-7.5M12 21V3" />
                </svg>
              }
              color="green"
            />
            <TrafficCard
              title="总流量"
              value={formatBytes(traffic?.total_traffic.total_bytes || 0)}
              icon={
                <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" strokeWidth={1.5} stroke="currentColor" className="w-8 h-8">
                  <path strokeLinecap="round" strokeLinejoin="round" d="M3 7.5L7.5 3m0 0L12 7.5M7.5 3v13.5m13.5-13.5L16.5 7.5m0 0L12 3m4.5 4.5v13.5" />
                </svg>
              }
              color="purple"
            />
          </div>
        </div>
      </div>

      {/* 每日流量趋势 */}
      <div className="bg-card rounded-2xl shadow-sm border border-border overflow-hidden">
        <div className="px-6 py-4 border-b border-border">
          <h3 className="text-lg font-semibold text-foreground">每日流量趋势</h3>
        </div>
        <div className="p-6">
          {traffic && traffic.daily_traffic.length > 0 ? (
            <div className="space-y-3">
              {traffic.daily_traffic.slice(-10).map((day, index) => {
                const maxBytes = Math.max(...traffic.daily_traffic.map((d) => d.total_bytes));
                const percentage = maxBytes > 0 ? (day.total_bytes / maxBytes) * 100 : 0;
                return (
                  <div key={index} className="flex items-center gap-4">
                    <div className="w-20 text-sm font-medium text-muted-foreground">{formatShortDate(day.date)}</div>
                    <div className="flex-1 bg-muted rounded-full h-8 overflow-hidden">
                      <div
                        className="h-full rounded-full flex items-center justify-end pr-3 transition-all duration-500"
                        style={{ width: `${Math.max(percentage, 8)}%`, background: 'linear-gradient(135deg, hsl(210 100% 45%), hsl(189 94% 43%))' }}
                      >
                        <span className="text-xs text-primary-foreground font-semibold whitespace-nowrap drop-shadow">
                          {formatBytes(day.total_bytes)}
                        </span>
                      </div>
                    </div>
                  </div>
                );
              })}
            </div>
          ) : (
            <div className="text-center py-12">
              <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" strokeWidth={1.5} stroke="currentColor" className="w-12 h-12 text-muted-foreground mx-auto mb-3">
                <path strokeLinecap="round" strokeLinejoin="round" d="M3 13.125C3 12.504 3.504 12 4.125 12h2.25c.621 0 1.125.504 1.125 1.125v6.75C7.5 20.496 6.996 21 6.375 21h-2.25A1.125 1.125 0 013 19.875v-6.75zM9.75 8.625c0-.621.504-1.125 1.125-1.125h2.25c.621 0 1.125.504 1.125 1.125v11.25c0 .621-.504 1.125-1.125 1.125h-2.25a1.125 1.125 0 01-1.125-1.125V8.625zM16.5 4.125c0-.621.504-1.125 1.125-1.125h2.25C20.496 3 21 3.504 21 4.125v15.75c0 .621-.504 1.125-1.125 1.125h-2.25a1.125 1.125 0 01-1.125-1.125V4.125z" />
              </svg>
              <p className="text-muted-foreground">暂无数据</p>
            </div>
          )}
        </div>
      </div>

      {/* 用户流量排行 */}
      {traffic && traffic.by_user.length > 0 && (
        <TableContainer>
          <div className="px-6 py-4 border-b border-border">
            <h3 className="text-lg font-semibold text-foreground">用户流量排行</h3>
          </div>
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>用户</TableHead>
                <TableHead>上传</TableHead>
                <TableHead>下载</TableHead>
                <TableHead>总流量</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {traffic.by_user.map((userTraffic) => (
                <TableRow key={userTraffic.user_id}>
                  <TableCell className="whitespace-nowrap">
                    <div className="flex items-center gap-3">
                      <div className="w-8 h-8 rounded-lg flex items-center justify-center text-primary-foreground text-xs font-semibold" style={{ background: 'linear-gradient(135deg, hsl(210 100% 45%), hsl(189 94% 43%))' }}>
                        {userTraffic.username.charAt(0).toUpperCase()}
                      </div>
                      <span className="text-sm font-medium text-foreground">{userTraffic.username}</span>
                    </div>
                  </TableCell>
                  <TableCell className="whitespace-nowrap text-sm text-muted-foreground">
                    {formatBytes(userTraffic.total_bytes_sent)}
                  </TableCell>
                  <TableCell className="whitespace-nowrap text-sm text-muted-foreground">
                    {formatBytes(userTraffic.total_bytes_received)}
                  </TableCell>
                  <TableCell className="whitespace-nowrap">
                    <span className="text-sm font-semibold text-foreground">{formatBytes(userTraffic.total_bytes)}</span>
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        </TableContainer>
      )}

      {/* 客户端流量排行 */}
      {traffic && traffic.by_client.length > 0 && (
        <TableContainer>
          <div className="px-6 py-4 border-b border-border">
            <h3 className="text-lg font-semibold text-foreground">节点流量排行</h3>
          </div>
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>节点</TableHead>
                <TableHead>上传</TableHead>
                <TableHead>下载</TableHead>
                <TableHead>总流量</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {traffic.by_client.map((clientTraffic) => (
                <TableRow key={clientTraffic.client_id}>
                  <TableCell className="whitespace-nowrap">
                    <div className="flex items-center gap-3">
                      <div className="w-8 h-8 bg-gradient-to-br from-green-500 to-emerald-600 rounded-lg flex items-center justify-center text-primary-foreground text-xs font-semibold">
                        {clientTraffic.client_name.charAt(0).toUpperCase()}
                      </div>
                      <span className="text-sm font-medium text-foreground">{clientTraffic.client_name}</span>
                    </div>
                  </TableCell>
                  <TableCell className="whitespace-nowrap text-sm text-muted-foreground">
                    {formatBytes(clientTraffic.total_bytes_sent)}
                  </TableCell>
                  <TableCell className="whitespace-nowrap text-sm text-muted-foreground">
                    {formatBytes(clientTraffic.total_bytes_received)}
                  </TableCell>
                  <TableCell className="whitespace-nowrap">
                    <span className="text-sm font-semibold text-foreground">{formatBytes(clientTraffic.total_bytes)}</span>
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        </TableContainer>
      )}

      {/* 代理流量排行 */}
      {traffic && traffic.by_proxy.length > 0 && (
        <TableContainer>
          <div className="px-6 py-4 border-b border-border">
            <h3 className="text-lg font-semibold text-foreground">代理流量排行</h3>
          </div>
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>代理名称</TableHead>
                <TableHead>所属节点</TableHead>
                <TableHead>上传</TableHead>
                <TableHead>下载</TableHead>
                <TableHead>总流量</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {traffic.by_proxy.map((proxyTraffic) => (
                <TableRow key={proxyTraffic.proxy_id}>
                  <TableCell className="whitespace-nowrap">
                    <div className="flex items-center gap-3">
                      <div className="w-8 h-8 bg-gradient-to-br from-purple-500 to-pink-600 rounded-lg flex items-center justify-center text-primary-foreground text-xs font-semibold">
                        {proxyTraffic.proxy_name.charAt(0).toUpperCase()}
                      </div>
                      <span className="text-sm font-medium text-foreground">{proxyTraffic.proxy_name}</span>
                    </div>
                  </TableCell>
                  <TableCell className="whitespace-nowrap text-sm text-muted-foreground">
                    {proxyTraffic.client_name}
                  </TableCell>
                  <TableCell className="whitespace-nowrap text-sm text-muted-foreground">
                    {formatBytes(proxyTraffic.total_bytes_sent)}
                  </TableCell>
                  <TableCell className="whitespace-nowrap text-sm text-muted-foreground">
                    {formatBytes(proxyTraffic.total_bytes_received)}
                  </TableCell>
                  <TableCell className="whitespace-nowrap">
                    <span className="text-sm font-semibold text-foreground">{formatBytes(proxyTraffic.total_bytes)}</span>
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        </TableContainer>
      )}
    </div>
  );
}

interface TrafficCardProps {
  title: string;
  value: string;
  icon: React.ReactNode;
  color: 'blue' | 'green' | 'purple';
}

function TrafficCard({ title, value, icon, color }: TrafficCardProps) {
  const colorClasses = {
    blue: {
      bg: 'bg-muted',
      icon: '',
      text: '',
      iconStyle: { color: 'hsl(217 91% 60%)' } as React.CSSProperties,
      textStyle: { color: 'hsl(217 91% 60%)' } as React.CSSProperties,
    },
    green: {
      bg: 'bg-muted',
      icon: '',
      text: '',
      iconStyle: { color: 'hsl(142 71% 45%)' } as React.CSSProperties,
      textStyle: { color: 'hsl(142 71% 45%)' } as React.CSSProperties,
    },
    purple: {
      bg: 'bg-muted',
      icon: '',
      text: '',
      iconStyle: { color: 'hsl(263 70% 58%)' } as React.CSSProperties,
      textStyle: { color: 'hsl(263 70% 58%)' } as React.CSSProperties,
    },
  };

  const config = colorClasses[color];

  return (
    <div className={`${config.bg} rounded-2xl p-6 text-center transition-transform hover:scale-[1.02]`}>
      <div className={`inline-flex items-center justify-center mb-3 ${config.icon}`} style={config.iconStyle}>
        {icon}
      </div>
      <div className="text-sm font-medium text-muted-foreground">{title}</div>
      <div className={`text-2xl font-bold mt-1 ${config.text}`} style={config.textStyle}>{value}</div>
    </div>
  );
}
