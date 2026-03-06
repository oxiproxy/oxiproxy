import { useEffect, useState } from 'react';
import { clientService, userService, systemService } from '../lib/services';
import type { Client, LogEntry } from '../lib/types';
import { formatBytes, formatDate, copyToClipboard } from '../lib/utils';
import { useToast } from '../contexts/ToastContext';
import ConfirmDialog from '../components/ConfirmDialog';
import { TableSkeleton } from '../components/Skeleton';
import {
  TableContainer,
  Table,
  TableHeader,
  TableBody,
  TableHead,
  TableRow,
  TableCell,
} from '../components/ui/table';

export default function Clients() {
  const { showToast } = useToast();
  const [clients, setClients] = useState<Client[]>([]);
  const [loading, setLoading] = useState(true);
  const [showCreateModal, setShowCreateModal] = useState(false);
  const [newClientName, setNewClientName] = useState('');
  const [newClientRegion, setNewClientRegion] = useState('');  const [confirmDialog, setConfirmDialog] = useState<{ open: boolean; title: string; message: string; onConfirm: () => void }>({ open: false, title: '', message: '', onConfirm: () => {} });

  // 日志相关状态
  const [showLogsModal, setShowLogsModal] = useState(false);
  const [selectedClient, setSelectedClient] = useState<Client | null>(null);
  const [logs, setLogs] = useState<LogEntry[]>([]);
  const [logsLoading, setLogsLoading] = useState(false);

  // 配额分配相关状态
  const [showQuotaModal, setShowQuotaModal] = useState(false);
  const [quotaFormData, setQuotaFormData] = useState({ quotaGb: '' });
  const [quotaSaving, setQuotaSaving] = useState(false);
  const [userQuotaInfo, setUserQuotaInfo] = useState<any>(null);

  // 命令生成相关状态
  const [showCommandModal, setShowCommandModal] = useState(false);
  const [commandClient, setCommandClient] = useState<Client | null>(null);
  const [selectedPlatform, setSelectedPlatform] = useState<'windows' | 'linux' | 'macos'>('linux');
  const [controllerUrl, setControllerUrl] = useState(() => {
    const hostname = window.location.hostname;
    return hostname === 'localhost' ? 'localhost:3100' : `${hostname}:3100`;
  });
  const [grpcTlsEnabled, setGrpcTlsEnabled] = useState(false);

  // 版本更新相关状态
  const [latestVersion, setLatestVersion] = useState<string | null>(null);
  const [updatingClientId, setUpdatingClientId] = useState<number | null>(null);
  const [batchUpdating, setBatchUpdating] = useState(false);

  useEffect(() => {
    loadClients();
    loadUserQuotaInfo();

    // 获取最新版本信息（仅管理员）
    const authUser = JSON.parse(localStorage.getItem('user') || '{}');
    if (authUser.is_admin) {
      systemService.getLatestVersion().then(res => {
        if (res.success && res.data) {
          setLatestVersion(res.data.latestVersion);
        }
      }).catch((err) => {
        console.warn('获取最新版本信息失败:', err);
      });
    }
  }, []);

  const loadClients = async () => {
    try {
      setLoading(true);
      const response = await clientService.getClients();
      if (response.success && response.data) {
        setClients(response.data);
      }
    } catch (error) {
      console.error('加载客户端失败:', error);
      showToast('加载失败', 'error');
    } finally {
      setLoading(false);
    }
  };

  const loadUserQuotaInfo = async () => {
    try {
      const authUser = JSON.parse(localStorage.getItem('user') || '{}');
      if (authUser.id) {
        const response = await userService.getQuotaInfo(authUser.id);
        if (response.success && response.data) {
          setUserQuotaInfo(response.data);
        }
      }
    } catch (error) {
      console.error('加载用户配额信息失败:', error);
    }
  };

  const handleCreateClient = async () => {
    if (!newClientName.trim()) {
      showToast('请输入客户端名称', 'error');
      return;
    }

    try {
      const response = await clientService.createClient({ name: newClientName, region: newClientRegion || undefined });
      if (response.success) {
        showToast('客户端创建成功', 'success');
        setNewClientName('');
        setNewClientRegion('');
        setShowCreateModal(false);
        loadClients();
      } else {
        showToast(response.message || '创建失败', 'error');
      }
    } catch (error) {
      console.error('创建客户端失败:', error);
      showToast('创建失败', 'error');
    }
  };

  const handleDeleteClient = (id: number) => {
    setConfirmDialog({
      open: true,
      title: '删除客户端',
      message: '确定要删除这个客户端吗？',
      onConfirm: async () => {
        try {
          const response = await clientService.deleteClient(id);
          if (response.success) {
            showToast('客户端删除成功', 'success');
            loadClients();
          } else {
            showToast(response.message || '删除失败', 'error');
          }
        } catch (error) {
          console.error('删除客户端失败:', error);
          showToast('删除失败', 'error');
        }
      },
    });
  };

  const handleViewLogs = async (client: Client) => {
    if (!client.is_online) {
      showToast('客户端离线，无法获取日志', 'error');
      return;
    }

    setSelectedClient(client);
    setShowLogsModal(true);
    setLogsLoading(true);

    try {
      const response = await clientService.getClientLogs(client.id);
      if (response.success && response.data) {
        setLogs(response.data);
      } else {
        showToast(response.message || '获取日志失败', 'error');
        setLogs([]);
      }
    } catch (error) {
      console.error('获取日志失败:', error);
      showToast('获取日志失败', 'error');
      setLogs([]);
    } finally {
      setLogsLoading(false);
    }
  };

  const handleAllocateQuota = (client: Client) => {
    setSelectedClient(client);
    setQuotaFormData({
      quotaGb: client.trafficQuotaGb !== null ? String(client.trafficQuotaGb) : '',
    });
    setShowQuotaModal(true);
  };

  const handleSaveQuota = async () => {
    if (!selectedClient) return;

    if (!quotaFormData.quotaGb || parseFloat(quotaFormData.quotaGb) < 0) {
      showToast('请输入有效的配额值', 'error');
      return;
    }

    const requestedQuota = parseFloat(quotaFormData.quotaGb);

    // 前端验证：检查是否超过用户可用配额
    if (userQuotaInfo && userQuotaInfo.total_quota_gb !== null) {
      const currentClientQuota = selectedClient.trafficQuotaGb || 0;
      const quotaDiff = requestedQuota - currentClientQuota;

      if (quotaDiff > userQuotaInfo.available_gb) {
        showToast(`配额不足：可用 ${userQuotaInfo.available_gb.toFixed(2)} GB，需要 ${quotaDiff.toFixed(2)} GB`, 'error');
        return;
      }
    }

    setQuotaSaving(true);
    try {
      const response = await clientService.allocateQuota(selectedClient.id, requestedQuota);

      if (response.success) {
        showToast('配额分配成功', 'success');
        setShowQuotaModal(false);
        setSelectedClient(null);
        loadClients();
        loadUserQuotaInfo(); // 重新加载用户配额信息
      } else {
        showToast(response.message || '分配失败', 'error');
      }
    } catch (error) {
      console.error('分配配额失败:', error);
      showToast('分配失败', 'error');
    } finally {
      setQuotaSaving(false);
    }
  };

  const handleResetTrafficExceeded = (client: Client) => {
    setConfirmDialog({
      open: true,
      title: '重置流量超限',
      message: '确定要重置该客户端的流量超限状态吗？',
      onConfirm: async () => {
        try {
          const response = await clientService.updateClient(client.id, {
            is_traffic_exceeded: false,
          });

          if (response.success) {
            showToast('已重置流量超限状态', 'success');
            loadClients();
          } else {
            showToast(response.message || '重置失败', 'error');
          }
        } catch (error) {
          console.error('重置流量超限状态失败:', error);
          showToast('重置失败', 'error');
        }
      },
    });
  };

  const getLevelColor = (level: string) => {
    switch (level.toUpperCase()) {
      case 'ERROR':
        return 'bg-red-100 text-red-700';
      case 'WARN':
        return 'bg-amber-100 text-amber-700';
      case 'INFO':
        return 'bg-muted text-primary';
      case 'DEBUG':
        return 'bg-muted text-foreground';
      default:
        return 'bg-muted text-foreground';
    }
  };

  // 生成客户端启动命令
  const getClientStartupCommand = (client?: Client, platform: 'windows' | 'linux' | 'macos' = 'linux') => {
    if (!client) return '';
    const url = controllerUrl || `${window.location.hostname}:3100`;
    const protocol = grpcTlsEnabled ? 'https' : 'http';
    const token = client.token;

    if (platform === 'windows') {
      return `client.exe start --controller-url ${protocol}://${url} --token ${token}`;
    } else {
      return `./client start --controller-url ${protocol}://${url} --token ${token}`;
    }
  };

  // 生成客户端后台运行命令
  const getClientDaemonCommand = (client?: Client, platform: 'windows' | 'linux' | 'macos' = 'linux') => {
    if (!client) return '';
    const url = controllerUrl || `${window.location.hostname}:3100`;
    const protocol = grpcTlsEnabled ? 'https' : 'http';
    const token = client.token;

    if (platform === 'windows') {
      return `client.exe install-service --controller-url ${protocol}://${url} --token ${token}`;
    } else {
      return `./client daemon --controller-url ${protocol}://${url} --token ${token}`;
    }
  };

  const handleClientUpdate = async (clientId: number) => {
    setUpdatingClientId(clientId);
    try {
      const response = await clientService.triggerUpdate(clientId);
      if (response.success && response.data?.success) {
        showToast(`客户端已更新到 v${response.data.newVersion}，正在重启...`, 'success');
        setTimeout(() => loadClients(), 10000);
      } else {
        showToast(response.data?.error || response.message || '更新失败', 'error');
      }
    } catch (err: any) {
      const msg = err?.response?.data?.message || '请求失败';
      showToast(msg, 'error');
    } finally {
      setUpdatingClientId(null);
    }
  };

  const handleBatchUpdate = () => {
    const updatableCount = clients.filter(c => c.is_online && c.version).length;
    if (updatableCount === 0) {
      showToast('没有可更新的在线客户端', 'error');
      return;
    }
    setConfirmDialog({
      open: true,
      title: '批量更新客户端',
      message: `确定要更新所有在线客户端 (${updatableCount} 个) ${latestVersion ? `到最新版本 v${latestVersion}` : '到最新版本'}吗？更新后客户端将自动重启。`,
      onConfirm: async () => {
        setConfirmDialog(prev => ({ ...prev, open: false }));
        setBatchUpdating(true);
        try {
          const response = await clientService.batchUpdate();
          if (response.success && response.data?.results) {
            const results = response.data.results;
            const successCount = results.filter(r => r.success).length;
            const failCount = results.filter(r => !r.success).length;
            if (failCount === 0) {
              showToast(`全部 ${successCount} 个客户端更新成功，正在重启...`, 'success');
            } else {
              const failNames = results.filter(r => !r.success).map(r => r.name || `#${r.id}`).join(', ');
              showToast(`${successCount} 个成功, ${failCount} 个失败 (${failNames})`, 'error');
            }
            setTimeout(() => loadClients(), 10000);
          } else {
            showToast(response.message || '批量更新失败', 'error');
          }
        } catch (err: any) {
          const msg = err?.response?.data?.message || '批量更新请求失败';
          showToast(msg, 'error');
        } finally {
          setBatchUpdating(false);
        }
      },
    });
  };

  const handleShowCommand = async (client: Client) => {
    setCommandClient(client);
    setShowCommandModal(true);
    try {
      const tlsStatus = await systemService.getGrpcTlsStatus();
      setGrpcTlsEnabled(tlsStatus.enabled);
    } catch {
      setGrpcTlsEnabled(false);
    }
  };

  return (
    <div className="space-y-6">
      {/* 页面标题 */}
      <div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
        <div>
          <h2 className="text-2xl font-bold text-foreground">客户端管理</h2>
          <p className="mt-1 text-sm text-muted-foreground">管理所有客户端连接</p>
        </div>
        <div className="flex items-center gap-2">
          {clients.some(c => c.is_online && c.version) && (
            <button
              onClick={handleBatchUpdate}
              disabled={batchUpdating}
              className="inline-flex items-center gap-2 px-4 py-2.5 text-sm font-medium rounded-xl border border-amber-300 text-amber-700 bg-amber-50 hover:bg-amber-100 focus:outline-none focus:ring-2 focus:ring-amber-400/40 transition-all duration-200 disabled:opacity-50 disabled:cursor-not-allowed"
            >
              {batchUpdating ? (
                <>
                  <svg className="animate-spin w-4 h-4" xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24">
                    <circle className="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="4"></circle>
                    <path className="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"></path>
                  </svg>
                  批量更新中...
                </>
              ) : (
                <>
                  <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" strokeWidth={2} stroke="currentColor" className="w-4 h-4">
                    <path strokeLinecap="round" strokeLinejoin="round" d="M3 16.5v2.25A2.25 2.25 0 005.25 21h13.5A2.25 2.25 0 0021 18.75V16.5M16.5 12L12 16.5m0 0L7.5 12m4.5 4.5V3" />
                  </svg>
                  一键更新全部
                </>
              )}
            </button>
          )}
          <button
            onClick={() => setShowCreateModal(true)}
            className="inline-flex items-center gap-2 px-5 py-2.5 text-primary-foreground text-sm font-medium rounded-xl focus:outline-none focus:ring-2 focus:ring-primary/40 shadow-sm transition-all duration-200 hover:opacity-90"
            style={{ background: 'linear-gradient(135deg, hsl(210 100% 45%), hsl(189 94% 43%))' }}
          >
            <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" strokeWidth={2} stroke="currentColor" className="w-4 h-4">
              <path strokeLinecap="round" strokeLinejoin="round" d="M12 4.5v15m7.5-7.5h-15" />
            </svg>
            新建客户端
          </button>
        </div>
      </div>

      {loading ? (
        <TableSkeleton rows={5} cols={9} />
      ) : (
        <TableContainer>
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>状态</TableHead>
                <TableHead>公网 IP</TableHead>
                <TableHead>名称</TableHead>
                <TableHead>地区</TableHead>
                <TableHead>流量统计</TableHead>
                <TableHead>流量限制</TableHead>
                <TableHead>版本</TableHead>
                <TableHead>创建时间</TableHead>
                <TableHead className="text-right">操作</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {clients.length === 0 ? (
                <TableRow>
                  <TableCell colSpan={9} className="px-6 py-16 text-center">
                    <div className="flex flex-col items-center gap-3">
                      <div className="w-16 h-16 bg-muted rounded-full flex items-center justify-center">
                        <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" strokeWidth={1.5} stroke="currentColor" className="w-8 h-8 text-muted-foreground">
                          <path strokeLinecap="round" strokeLinejoin="round" d="M21 7.5l-9-5.25L3 7.5m18 0l-9 5.25m9-5.25v9l-9 5.25M3 7.5l9 5.25M3 7.5v9l9 5.25m0-9v9" />
                        </svg>
                      </div>
                      <p className="text-muted-foreground">暂无客户端数据</p>
                      <button
                        onClick={() => setShowCreateModal(true)}
                        className="text-sm text-primary hover:text-primary/80 font-medium"
                      >
                        创建第一个客户端
                      </button>
                    </div>
                  </TableCell>
                </TableRow>
              ) : (
                clients.map((client) => (
                  <TableRow key={client.id}>
                    <TableCell className="whitespace-nowrap">
                      <div className="flex items-center gap-2">
                        <span className={`relative flex h-3 w-3`}>
                          {client.is_online && (
                            <span className="animate-ping absolute inline-flex h-full w-full rounded-full bg-green-400 opacity-75"></span>
                          )}
                          <span className={`relative inline-flex rounded-full h-3 w-3 ${client.is_online ? '' : ''}`} style={{ background: client.is_online ? 'hsl(142 71% 45%)' : 'hsl(0 84.2% 60.2%)' }}></span>
                        </span>
                        <span className={`text-sm font-medium`} style={{ color: client.is_online ? 'hsl(142 71% 45%)' : 'hsl(0 84.2% 60.2%)' }}>
                          {client.is_online ? '在线' : '离线'}
                        </span>
                      </div>
                    </TableCell>
                    <TableCell className="whitespace-nowrap">
                      {client.publicIp ? (
                        <span className="inline-flex items-center gap-1.5 px-2.5 py-1 text-xs font-medium rounded-lg bg-emerald-50 text-emerald-700 font-mono">
                          <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" strokeWidth={2} stroke="currentColor" className="w-3.5 h-3.5">
                            <path strokeLinecap="round" strokeLinejoin="round" d="M12 21a9.004 9.004 0 008.716-6.747M12 21a9.004 9.004 0 01-8.716-6.747M12 21c2.485 0 4.5-4.03 4.5-9S14.485 3 12 3m0 18c-2.485 0-4.5-4.03-4.5-9S9.515 3 12 3m0 0a8.997 8.997 0 017.843 4.582M12 3a8.997 8.997 0 00-7.843 4.582m15.686 0A11.953 11.953 0 0112 10.5c-2.998 0-5.74-1.1-7.843-2.918m15.686 0A8.959 8.959 0 0121 12c0 .778-.099 1.533-.284 2.253m0 0A17.919 17.919 0 0112 16.5c-3.162 0-6.133-.815-8.716-2.247m0 0A9.015 9.015 0 013 12c0-1.605.42-3.113 1.157-4.418" />
                          </svg>
                          {client.publicIp}
                        </span>
                      ) : (
                        <span className="text-xs text-muted-foreground">-</span>
                      )}
                    </TableCell>
                    <TableCell className="whitespace-nowrap">
                      <div className="flex items-center gap-3">
                        <div className="w-9 h-9 rounded-lg flex items-center justify-center text-primary-foreground text-sm font-semibold shadow-sm" style={{ background: 'linear-gradient(135deg, hsl(210 100% 45%), hsl(189 94% 43%))' }}>
                          {client.name.charAt(0).toUpperCase()}
                        </div>
                        <span className="text-sm font-semibold text-foreground">{client.name}</span>
                      </div>
                    </TableCell>
                    <TableCell className="whitespace-nowrap">
                      {client.region ? (
                        <span className="inline-flex items-center gap-1.5 px-2.5 py-1 text-xs font-medium rounded-lg bg-blue-50 text-blue-700">
                          <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" strokeWidth={2} stroke="currentColor" className="w-3.5 h-3.5">
                            <path strokeLinecap="round" strokeLinejoin="round" d="M15 10.5a3 3 0 11-6 0 3 3 0 016 0z" />
                            <path strokeLinecap="round" strokeLinejoin="round" d="M19.5 10.5c0 7.142-7.5 11.25-7.5 11.25S4.5 17.642 4.5 10.5a7.5 7.5 0 1115 0z" />
                          </svg>
                          {client.region}
                        </span>
                      ) : (
                        <span className="text-xs text-muted-foreground">-</span>
                      )}
                    </TableCell>
                    <TableCell className="whitespace-nowrap">
                      <div className="flex flex-col gap-1">
                        <div className="flex items-center gap-1.5 text-xs">
                          <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" strokeWidth={2} stroke="currentColor" className="w-3.5 h-3.5" style={{ color: 'hsl(217 91% 60%)' }}>
                            <path strokeLinecap="round" strokeLinejoin="round" d="M4.5 10.5L12 3m0 0l7.5 7.5M12 3v18" />
                          </svg>
                          <span className="text-muted-foreground">{formatBytes(client.totalBytesSent)}</span>
                        </div>
                        <div className="flex items-center gap-1.5 text-xs">
                          <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" strokeWidth={2} stroke="currentColor" className="w-3.5 h-3.5" style={{ color: 'hsl(142 71% 45%)' }}>
                            <path strokeLinecap="round" strokeLinejoin="round" d="M19.5 13.5L12 21m0 0l-7.5-7.5M12 21V3" />
                          </svg>
                          <span className="text-muted-foreground">{formatBytes(client.totalBytesReceived)}</span>
                        </div>
                      </div>
                    </TableCell>
                    <TableCell className="whitespace-nowrap">
                      <div className="flex flex-col gap-1">
                        {client.isTrafficExceeded && (
                          <span className="inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-xs font-medium bg-red-100 text-red-700 mb-1">
                            <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" strokeWidth={2} stroke="currentColor" className="w-3 h-3">
                              <path strokeLinecap="round" strokeLinejoin="round" d="M12 9v3.75m-9.303 3.376c-.866 1.5.217 3.374 1.948 3.374h14.71c1.73 0 2.813-1.874 1.948-3.374L13.949 3.378c-.866-1.5-3.032-1.5-3.898 0L2.697 16.126zM12 15.75h.007v.008H12v-.008z" />
                            </svg>
                            配额已用尽
                          </span>
                        )}
                        {client.trafficQuotaGb ? (
                          <>
                            <div className="text-xs font-medium text-foreground">
                              配额: {client.trafficQuotaGb} GB
                            </div>
                            <div className="text-xs text-green-600">
                              剩余: {(client.trafficQuotaGb - (client.totalBytesSent + client.totalBytesReceived) / (1024 * 1024 * 1024)).toFixed(2)} GB
                            </div>
                            <div className="w-full bg-muted rounded-full h-1.5 mt-1">
                              <div
                                className={`h-1.5 rounded-full ${
                                  ((client.trafficQuotaGb - (client.totalBytesSent + client.totalBytesReceived) / (1024 * 1024 * 1024)) / client.trafficQuotaGb) < 0.2
                                    ? 'bg-red-500'
                                    : ((client.trafficQuotaGb - (client.totalBytesSent + client.totalBytesReceived) / (1024 * 1024 * 1024)) / client.trafficQuotaGb) < 0.5
                                    ? 'bg-yellow-500'
                                    : 'bg-green-500'
                                }`}
                                style={{
                                  width: `${Math.max(0, Math.min(100, ((client.trafficQuotaGb - (client.totalBytesSent + client.totalBytesReceived) / (1024 * 1024 * 1024)) / client.trafficQuotaGb) * 100))}%`,
                                }}
                              ></div>
                            </div>
                          </>
                        ) : (
                          <div className="text-xs text-muted-foreground">
                            无配额限制
                          </div>
                        )}
                      </div>
                    </TableCell>
                    <TableCell className="whitespace-nowrap">
                      <div className="flex items-center gap-2">
                        {!client.version ? (
                          <span className="text-xs text-muted-foreground">-</span>
                        ) : client.version === latestVersion ? (
                          <span className="inline-flex items-center gap-1.5 px-2.5 py-1 text-xs font-medium rounded-lg bg-green-50 text-green-700">
                            v{client.version}
                          </span>
                        ) : latestVersion ? (
                          <span className="inline-flex items-center gap-1.5 px-2.5 py-1 text-xs font-medium rounded-lg bg-amber-50 text-amber-700">
                            v{client.version}
                          </span>
                        ) : (
                          <span className="inline-flex items-center px-2.5 py-1 text-xs font-medium rounded-lg bg-muted text-muted-foreground">
                            v{client.version}
                          </span>
                        )}
                        {client.is_online && client.version && (
                          <button
                            onClick={() => handleClientUpdate(client.id)}
                            disabled={updatingClientId === client.id}
                            className={`inline-flex items-center gap-1 px-2 py-1 text-xs font-medium rounded-lg transition-colors disabled:opacity-50 disabled:cursor-not-allowed ${
                              client.version !== latestVersion && latestVersion
                                ? 'text-amber-600 hover:bg-amber-50'
                                : 'text-muted-foreground hover:bg-muted'
                            }`}
                            title={latestVersion ? `更新到 v${latestVersion}` : '重新安装当前版本'}
                          >
                            {updatingClientId === client.id ? (
                              <>
                                <svg className="animate-spin w-3.5 h-3.5" xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24">
                                  <circle className="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="4"></circle>
                                  <path className="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"></path>
                                </svg>
                                更新中...
                              </>
                            ) : (
                              <>
                                <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" strokeWidth={2} stroke="currentColor" className="w-3.5 h-3.5">
                                  <path strokeLinecap="round" strokeLinejoin="round" d="M3 16.5v2.25A2.25 2.25 0 005.25 21h13.5A2.25 2.25 0 0021 18.75V16.5M16.5 12L12 16.5m0 0L7.5 12m4.5 4.5V3" />
                                </svg>
                                {client.version !== latestVersion && latestVersion ? '更新' : '重装'}
                              </>
                            )}
                          </button>
                        )}
                      </div>
                    </TableCell>
                    <TableCell className="whitespace-nowrap text-sm text-muted-foreground">
                      {formatDate(client.created_at)}
                    </TableCell>
                    <TableCell className="whitespace-nowrap text-right">
                      <div className="flex flex-wrap items-center justify-end gap-1.5">
                        <button
                          onClick={() => handleShowCommand(client)}
                          className="inline-flex items-center gap-1.5 px-3 py-1.5 text-xs font-medium text-purple-600 hover:bg-purple-50 rounded-lg transition-colors"
                          title="查看启动命令"
                        >
                          <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" strokeWidth={1.5} stroke="currentColor" className="w-4 h-4">
                            <path strokeLinecap="round" strokeLinejoin="round" d="M6.75 7.5l3 2.25-3 2.25m4.5 0h3m-9 8.25h13.5A2.25 2.25 0 0021 18V6a2.25 2.25 0 00-2.25-2.25H5.25A2.25 2.25 0 003 6v12a2.25 2.25 0 002.25 2.25z" />
                          </svg>
                          启动命令
                        </button>
                        <button
                          onClick={() => handleAllocateQuota(client)}
                          className="inline-flex items-center gap-1.5 px-3 py-1.5 text-xs font-medium text-primary hover:bg-accent rounded-lg transition-colors"
                          title="分配流量配额"
                        >
                          <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" strokeWidth={1.5} stroke="currentColor" className="w-4 h-4">
                            <path strokeLinecap="round" strokeLinejoin="round" d="M3 13.125C3 12.504 3.504 12 4.125 12h2.25c.621 0 1.125.504 1.125 1.125v6.75C7.5 20.496 6.996 21 6.375 21h-2.25A1.125 1.125 0 013 19.875v-6.75zM9.75 8.625c0-.621.504-1.125 1.125-1.125h2.25c.621 0 1.125.504 1.125 1.125v11.25c0 .621-.504 1.125-1.125 1.125h-2.25a1.125 1.125 0 01-1.125-1.125V8.625zM16.5 4.125c0-.621.504-1.125 1.125-1.125h2.25C20.496 3 21 3.504 21 4.125v15.75c0 .621-.504 1.125-1.125 1.125h-2.25a1.125 1.125 0 01-1.125-1.125V4.125z" />
                          </svg>
                          配额
                        </button>
                        {client.isTrafficExceeded && (
                          <button
                            onClick={() => handleResetTrafficExceeded(client)}
                            className="inline-flex items-center gap-1.5 px-3 py-1.5 text-xs font-medium text-amber-600 hover:bg-amber-50 rounded-lg transition-colors"
                            title="重置流量超限状态"
                          >
                            <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" strokeWidth={1.5} stroke="currentColor" className="w-4 h-4">
                              <path strokeLinecap="round" strokeLinejoin="round" d="M16.023 9.348h4.992v-.001M2.985 19.644v-4.992m0 0h4.992m-4.993 0l3.181 3.183a8.25 8.25 0 0013.803-3.7M4.031 9.865a8.25 8.25 0 0113.803-3.7l3.181 3.182m0-4.991v4.99" />
                            </svg>
                            重置
                          </button>
                        )}
                        <button
                          onClick={() => handleViewLogs(client)}
                          disabled={!client.is_online}
                          className={`inline-flex items-center gap-1.5 px-3 py-1.5 text-xs font-medium rounded-lg transition-colors ${
                            client.is_online
                              ? 'text-primary hover:bg-accent'
                              : 'text-muted-foreground cursor-not-allowed'
                          }`}
                          title={client.is_online ? '查看日志' : '客户端离线'}
                        >
                          <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" strokeWidth={1.5} stroke="currentColor" className="w-4 h-4">
                            <path strokeLinecap="round" strokeLinejoin="round" d="M19.5 14.25v-2.625a3.375 3.375 0 00-3.375-3.375h-1.5A1.125 1.125 0 0113.5 7.125v-1.5a3.375 3.375 0 00-3.375-3.375H8.25m0 12.75h7.5m-7.5 3H12M10.5 2.25H5.625c-.621 0-1.125.504-1.125 1.125v17.25c0 .621.504 1.125 1.125 1.125h12.75c.621 0 1.125-.504 1.125-1.125V11.25a9 9 0 00-9-9z" />
                          </svg>
                          日志
                        </button>
                        <button
                          onClick={() => handleDeleteClient(client.id)}
                          className="inline-flex items-center gap-1.5 px-3 py-1.5 text-xs font-medium text-red-600 hover:bg-red-50 rounded-lg transition-colors"
                        >
                          <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" strokeWidth={1.5} stroke="currentColor" className="w-4 h-4">
                            <path strokeLinecap="round" strokeLinejoin="round" d="M14.74 9l-.346 9m-4.788 0L9.26 9m9.968-3.21c.342.052.682.107 1.022.166m-1.022-.165L18.16 19.673a2.25 2.25 0 01-2.244 2.077H8.084a2.25 2.25 0 01-2.244-2.077L4.772 5.79m14.456 0a48.108 48.108 0 00-3.478-.397m-12 .562c.34-.059.68-.114 1.022-.165m0 0a48.11 48.11 0 013.478-.397m7.5 0v-.916c0-1.18-.91-2.164-2.09-2.201a51.964 51.964 0 00-3.32 0c-1.18.037-2.09 1.022-2.09 2.201v.916m7.5 0a48.667 48.667 0 00-7.5 0" />
                          </svg>
                          删除
                        </button>
                      </div>
                    </TableCell>
                  </TableRow>
                ))
              )}
            </TableBody>
          </Table>
        </TableContainer>
      )}

      {/* 创建节点模态框 */}
      {showCreateModal && (
        <div className="fixed inset-0 bg-black/50 backdrop-blur-sm overflow-y-auto h-full w-full flex items-center justify-center z-50">
          <div className="relative bg-card rounded-2xl shadow-2xl w-full max-w-md mx-4 transform transition-all">
            <div className="p-6">
              <div className="flex items-center gap-3 mb-6">
                <div className="w-10 h-10 rounded-xl flex items-center justify-center" style={{ background: 'linear-gradient(135deg, hsl(210 100% 45%), hsl(189 94% 43%))' }}>
                  <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" strokeWidth={1.5} stroke="currentColor" className="w-5 h-5 text-primary-foreground">
                    <path strokeLinecap="round" strokeLinejoin="round" d="M12 4.5v15m7.5-7.5h-15" />
                  </svg>
                </div>
                <div>
                  <h3 className="text-lg font-bold text-foreground">创建新客户端</h3>
                  <p className="text-sm text-muted-foreground">添加一个新的客户端</p>
                </div>
              </div>
              <div className="space-y-4">
                <div>
                  <label className="block text-sm font-medium text-foreground mb-1.5">客户端名称</label>
                  <input
                    type="text"
                    value={newClientName}
                    onChange={(e) => setNewClientName(e.target.value)}
                    placeholder="请输入客户端名称"
                    className="w-full px-4 py-3 border border-border rounded-xl text-foreground placeholder-muted-foreground focus:outline-none focus:ring-2 focus:ring-primary/20 focus:border-primary transition-all bg-muted/50 hover:bg-card"
                    autoFocus
                    onKeyDown={(e) => e.key === 'Enter' && handleCreateClient()}
                  />
                </div>
                <div>
                  <label className="block text-sm font-medium text-foreground mb-1.5">地区 <span className="text-muted-foreground font-normal">(可选)</span></label>
                  <input
                    type="text"
                    value={newClientRegion}
                    onChange={(e) => setNewClientRegion(e.target.value)}
                    placeholder="例如：北京、上海、广州"
                    className="w-full px-4 py-3 border border-border rounded-xl text-foreground placeholder-muted-foreground focus:outline-none focus:ring-2 focus:ring-primary/20 focus:border-primary transition-all bg-muted/50 hover:bg-card"
                    onKeyDown={(e) => e.key === 'Enter' && handleCreateClient()}
                  />
                </div>
              </div>
              <div className="mt-6 flex gap-3">
                <button
                  onClick={() => {
                    setShowCreateModal(false);
                    setNewClientName('');
                    setNewClientRegion('');
                  }}
                  className="flex-1 px-4 py-2.5 bg-muted text-foreground font-medium rounded-xl hover:bg-accent transition-colors"
                >
                  取消
                </button>
                <button
                  onClick={handleCreateClient}
                  className="flex-1 px-4 py-2.5 bg-primary text-primary-foreground font-medium rounded-xl hover:bg-primary/90 shadow-sm transition-all"
                >
                  创建
                </button>
              </div>
            </div>
          </div>
        </div>
      )}

      {/* 日志查看模态框 */}
      {showLogsModal && selectedClient && (
        <div className="fixed inset-0 bg-black/50 backdrop-blur-sm overflow-y-auto h-full w-full flex items-center justify-center z-50">
          <div className="relative bg-card rounded-2xl shadow-2xl w-full max-w-4xl mx-4 max-h-[85vh] flex flex-col transform transition-all">
            <div className="flex items-center justify-between p-6 border-b border-border">
              <div className="flex items-center gap-3">
                <div className="w-10 h-10 rounded-xl flex items-center justify-center" style={{ background: 'linear-gradient(135deg, hsl(210 100% 45%), hsl(189 94% 43%))' }}>
                  <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" strokeWidth={1.5} stroke="currentColor" className="w-5 h-5 text-primary-foreground">
                    <path strokeLinecap="round" strokeLinejoin="round" d="M19.5 14.25v-2.625a3.375 3.375 0 00-3.375-3.375h-1.5A1.125 1.125 0 0113.5 7.125v-1.5a3.375 3.375 0 00-3.375-3.375H8.25m0 12.75h7.5m-7.5 3H12M10.5 2.25H5.625c-.621 0-1.125.504-1.125 1.125v17.25c0 .621.504 1.125 1.125 1.125h12.75c.621 0 1.125-.504 1.125-1.125V11.25a9 9 0 00-9-9z" />
                  </svg>
                </div>
                <div>
                  <h3 className="text-lg font-bold text-foreground">客户端日志</h3>
                  <p className="text-sm text-muted-foreground">{selectedClient.name}</p>
                </div>
              </div>
              <button
                onClick={() => {
                  setShowLogsModal(false);
                  setSelectedClient(null);
                  setLogs([]);
                }}
                className="p-2 text-muted-foreground hover:text-muted-foreground hover:bg-accent rounded-lg transition-colors"
              >
                <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" strokeWidth={2} stroke="currentColor" className="w-5 h-5">
                  <path strokeLinecap="round" strokeLinejoin="round" d="M6 18L18 6M6 6l12 12" />
                </svg>
              </button>
            </div>

            <div className="flex-1 overflow-y-auto p-6">
              {logsLoading ? (
                <div className="flex items-center justify-center h-64">
                  <div className="flex flex-col items-center gap-3">
                    <div className="w-10 h-10 border-4 border-border border-t-primary rounded-full animate-spin"></div>
                    <span className="text-sm text-muted-foreground">加载日志中...</span>
                  </div>
                </div>
              ) : logs.length === 0 ? (
                <div className="flex flex-col items-center justify-center h-64 text-muted-foreground">
                  <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" strokeWidth={1.5} stroke="currentColor" className="w-12 h-12 text-muted-foreground mb-3">
                    <path strokeLinecap="round" strokeLinejoin="round" d="M19.5 14.25v-2.625a3.375 3.375 0 00-3.375-3.375h-1.5A1.125 1.125 0 0113.5 7.125v-1.5a3.375 3.375 0 00-3.375-3.375H8.25m0 12.75h7.5m-7.5 3H12M10.5 2.25H5.625c-.621 0-1.125.504-1.125 1.125v17.25c0 .621.504 1.125 1.125 1.125h12.75c.621 0 1.125-.504 1.125-1.125V11.25a9 9 0 00-9-9z" />
                  </svg>
                  暂无日志数据
                </div>
              ) : (
                <div className="space-y-2 font-mono text-sm">
                  {logs.map((log, index) => (
                    <div
                      key={index}
                      className={`p-3 rounded-lg border-l-4 ${
                        log.level === 'ERROR' ? 'bg-red-50 border-red-500' :
                        log.level === 'WARN' ? 'bg-amber-50 border-amber-500' :
                        log.level === 'INFO' ? 'bg-muted border-border' : 'bg-muted border-muted-foreground'
                      }`}
                    >
                      <div className="flex items-start gap-3">
                        <span className="text-muted-foreground text-xs whitespace-nowrap">
                          {new Date(log.timestamp).toLocaleString('zh-CN')}
                        </span>
                        <span className={`font-semibold text-xs whitespace-nowrap px-1.5 py-0.5 rounded ${getLevelColor(log.level)}`}>
                          {log.level}
                        </span>
                        <span className="text-foreground flex-1 break-all">
                          {log.message.replace(/^"|"$/g, '')}
                        </span>
                      </div>
                    </div>
                  ))}
                </div>
              )}
            </div>

            <div className="p-4 border-t border-border flex justify-end">
              <button
                onClick={() => {
                  setShowLogsModal(false);
                  setSelectedClient(null);
                  setLogs([]);
                }}
                className="px-5 py-2.5 bg-muted text-foreground font-medium rounded-xl hover:bg-accent transition-colors"
              >
                关闭
              </button>
            </div>
          </div>
        </div>
      )}

      {/* 配额分配模态框 */}
      {showQuotaModal && selectedClient && (
        <div className="fixed inset-0 bg-black/50 backdrop-blur-sm overflow-y-auto h-full w-full flex items-center justify-center z-50">
          <div className="relative bg-card rounded-2xl shadow-2xl w-full max-w-md mx-4 transform transition-all">
            <div className="p-6">
              <div className="flex items-center gap-3 mb-6">
                <div className="w-10 h-10 rounded-xl flex items-center justify-center" style={{ background: 'linear-gradient(135deg, hsl(210 100% 45%), hsl(189 94% 43%))' }}>
                  <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" strokeWidth={1.5} stroke="currentColor" className="w-5 h-5 text-primary-foreground">
                    <path strokeLinecap="round" strokeLinejoin="round" d="M3 13.125C3 12.504 3.504 12 4.125 12h2.25c.621 0 1.125.504 1.125 1.125v6.75C7.5 20.496 6.996 21 6.375 21h-2.25A1.125 1.125 0 013 19.875v-6.75zM9.75 8.625c0-.621.504-1.125 1.125-1.125h2.25c.621 0 1.125.504 1.125 1.125v11.25c0 .621-.504 1.125-1.125 1.125h-2.25a1.125 1.125 0 01-1.125-1.125V8.625zM16.5 4.125c0-.621.504-1.125 1.125-1.125h2.25C20.496 3 21 3.504 21 4.125v15.75c0 .621-.504 1.125-1.125 1.125h-2.25a1.125 1.125 0 01-1.125-1.125V4.125z" />
                  </svg>
                </div>
                <div>
                  <h3 className="text-lg font-bold text-foreground">分配流量配额</h3>
                  <p className="text-sm text-muted-foreground">{selectedClient.name}</p>
                </div>
              </div>

              {/* 用户配额信息 */}
              {userQuotaInfo && userQuotaInfo.total_quota_gb !== null && (
                <div className="mb-4 p-4 bg-muted rounded-xl border border-border">
                  <div className="flex items-start gap-2 mb-3">
                    <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" strokeWidth={2} stroke="currentColor" className="w-5 h-5 text-primary flex-shrink-0 mt-0.5">
                      <path strokeLinecap="round" strokeLinejoin="round" d="M15.75 6a3.75 3.75 0 11-7.5 0 3.75 3.75 0 017.5 0zM4.501 20.118a7.5 7.5 0 0114.998 0A17.933 17.933 0 0112 21.75c-2.676 0-5.216-.584-7.499-1.632z" />
                    </svg>
                    <div className="flex-1">
                      <p className="text-sm font-bold text-primary">您的配额信息</p>
                      <div className="mt-2 space-y-1 text-xs text-primary">
                        <div className="flex justify-between">
                          <span>总配额:</span>
                          <span className="font-semibold">{userQuotaInfo.total_quota_gb.toFixed(2)} GB</span>
                        </div>
                        <div className="flex justify-between">
                          <span>已使用:</span>
                          <span className="font-semibold">{userQuotaInfo.used_gb.toFixed(2)} GB</span>
                        </div>
                        <div className="flex justify-between">
                          <span>已分配给客户端:</span>
                          <span className="font-semibold">{userQuotaInfo.allocated_to_clients_gb.toFixed(2)} GB</span>
                        </div>
                        <div className="flex justify-between pt-1 border-t border-border">
                          <span className="font-bold">可用配额:</span>
                          <span className="font-bold text-green-600">{userQuotaInfo.available_gb.toFixed(2)} GB</span>
                        </div>
                      </div>
                    </div>
                  </div>
                  <div className="w-full bg-muted rounded-full h-2">
                    <div
                      className="h-2 rounded-full bg-primary transition-all"
                      style={{
                        width: `${Math.min(100, (userQuotaInfo.quota_usage_percent || 0))}%`,
                      }}
                    ></div>
                  </div>
                </div>
              )}

              <div className="space-y-4">
                <div>
                  <label className="block text-sm font-medium text-foreground mb-1.5">流量配额 (GB)</label>
                  <input
                    type="number"
                    step="0.1"
                    min="0"
                    max={userQuotaInfo && userQuotaInfo.total_quota_gb !== null ?
                      (userQuotaInfo.available_gb + (selectedClient.trafficQuotaGb || 0)) : undefined}
                    value={quotaFormData.quotaGb}
                    onChange={(e) => setQuotaFormData({ quotaGb: e.target.value })}
                    placeholder="请输入配额大小"
                    className="w-full px-4 py-3 border border-border rounded-xl text-foreground placeholder-muted-foreground focus:outline-none focus:ring-2 focus:ring-primary/20 focus:border-primary transition-all bg-muted/50 hover:bg-card"
                    autoFocus
                  />
                  <p className="mt-2 text-xs text-muted-foreground">
                    设置此客户端的总流量配额（上传+下载）
                    {userQuotaInfo && userQuotaInfo.total_quota_gb !== null && (
                      <span className="block mt-1 text-primary font-medium">
                        最多可分配: {(userQuotaInfo.available_gb + (selectedClient.trafficQuotaGb || 0)).toFixed(2)} GB
                      </span>
                    )}
                  </p>
                </div>
                {selectedClient.trafficQuotaGb && (
                  <div className="p-4 bg-muted rounded-xl">
                    <div className="flex items-start gap-2">
                      <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" strokeWidth={2} stroke="currentColor" className="w-5 h-5 text-primary flex-shrink-0 mt-0.5">
                        <path strokeLinecap="round" strokeLinejoin="round" d="M11.25 11.25l.041-.02a.75.75 0 011.063.852l-.708 2.836a.75.75 0 001.063.853l.041-.021M21 12a9 9 0 11-18 0 9 9 0 0118 0zm-9-3.75h.008v.008H12V8.25z" />
                      </svg>
                      <div className="flex-1">
                        <p className="text-sm font-medium text-primary">当前客户端配额</p>
                        <p className="text-xs text-primary mt-1">
                          当前配额: {selectedClient.trafficQuotaGb} GB<br />
                          已使用: {((selectedClient.totalBytesSent + selectedClient.totalBytesReceived) / (1024 * 1024 * 1024)).toFixed(2)} GB<br />
                          剩余: {(selectedClient.trafficQuotaGb - (selectedClient.totalBytesSent + selectedClient.totalBytesReceived) / (1024 * 1024 * 1024)).toFixed(2)} GB
                        </p>
                      </div>
                    </div>
                  </div>
                )}
              </div>
              <div className="mt-6 flex gap-3">
                <button
                  onClick={() => {
                    setShowQuotaModal(false);
                    setSelectedClient(null);
                  }}
                  className="flex-1 px-4 py-2.5 bg-muted text-foreground font-medium rounded-xl hover:bg-accent transition-colors"
                  disabled={quotaSaving}
                >
                  取消
                </button>
                <button
                  onClick={handleSaveQuota}
                  disabled={quotaSaving}
                  className="flex-1 px-4 py-2.5 bg-primary text-primary-foreground font-medium rounded-xl hover:bg-primary/90 shadow-sm transition-all disabled:opacity-50"
                >
                  {quotaSaving ? '分配中...' : '确认分配'}
                </button>
              </div>
            </div>
          </div>
        </div>
      )}

      {/* 启动命令教程模态框 */}
      {showCommandModal && commandClient && (
        <div className="fixed inset-0 bg-black/50 backdrop-blur-sm overflow-y-auto h-full w-full flex items-center justify-center z-50">
          <div className="relative bg-card rounded-2xl shadow-2xl w-full max-w-3xl mx-4 max-h-[90vh] overflow-y-auto transform transition-all">
            <div className="sticky top-0 bg-card border-b border-border p-6 z-10">
              <div className="flex items-center justify-between">
                <div className="flex items-center gap-3">
                  <div className="w-10 h-10 rounded-xl flex items-center justify-center" style={{ background: 'linear-gradient(135deg, hsl(210 100% 45%), hsl(189 94% 43%))' }}>
                    <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" strokeWidth={1.5} stroke="currentColor" className="w-5 h-5 text-primary-foreground">
                      <path strokeLinecap="round" strokeLinejoin="round" d="M6.75 7.5l3 2.25-3 2.25m4.5 0h3m-9 8.25h13.5A2.25 2.25 0 0021 18V6a2.25 2.25 0 00-2.25-2.25H5.25A2.25 2.25 0 003 6v12a2.25 2.25 0 002.25 2.25z" />
                    </svg>
                  </div>
                  <div>
                    <h3 className="text-lg font-bold text-foreground">客户端启动教程</h3>
                    <p className="text-sm text-muted-foreground">{commandClient.name}</p>
                  </div>
                </div>
                <button
                  onClick={() => {
                    setShowCommandModal(false);
                    setCommandClient(null);
                  }}
                  className="p-2 text-muted-foreground hover:text-muted-foreground hover:bg-accent rounded-lg transition-colors"
                >
                  <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" strokeWidth={2} stroke="currentColor" className="w-5 h-5">
                    <path strokeLinecap="round" strokeLinejoin="round" d="M6 18L18 6M6 6l12 12" />
                  </svg>
                </button>
              </div>

              {/* 平台选择器 */}
              <div className="mt-4 flex gap-2">
                <button
                  onClick={() => setSelectedPlatform('windows')}
                  className={`flex-1 px-4 py-2.5 rounded-lg font-medium transition-all ${
                    selectedPlatform === 'windows'
                      ? 'bg-primary text-primary-foreground shadow-sm'
                      : 'bg-muted text-foreground hover:bg-accent'
                  }`}
                >
                  🪟 Windows
                </button>
                <button
                  onClick={() => setSelectedPlatform('linux')}
                  className={`flex-1 px-4 py-2.5 rounded-lg font-medium transition-all ${
                    selectedPlatform === 'linux'
                      ? 'bg-primary text-primary-foreground shadow-sm'
                      : 'bg-muted text-foreground hover:bg-accent'
                  }`}
                >
                  🐧 Linux
                </button>
                <button
                  onClick={() => setSelectedPlatform('macos')}
                  className={`flex-1 px-4 py-2.5 rounded-lg font-medium transition-all ${
                    selectedPlatform === 'macos'
                      ? 'bg-primary text-primary-foreground shadow-sm'
                      : 'bg-muted text-foreground hover:bg-accent'
                  }`}
                >
                  🍎 macOS
                </button>
              </div>
            </div>

            <div className="p-6 space-y-6">
              {/* Controller 地址 */}
              <div className="space-y-2">
                <label className="block text-sm font-medium text-foreground">Controller 地址</label>
                <input
                  type="text"
                  value={controllerUrl}
                  onChange={(e) => setControllerUrl(e.target.value)}
                  placeholder="例如: 192.168.1.100:3100"
                  className="w-full px-4 py-2.5 border border-border rounded-xl text-foreground focus:outline-none focus:ring-2 focus:ring-primary/20 focus:border-primary transition-all bg-muted/50 hover:bg-card font-mono text-sm"
                />
                <p className="text-xs text-muted-foreground">
                  修改为客户端可以访问的 Controller 地址（IP:端口）
                </p>
                <div className={`flex items-center gap-2 px-3 py-2 rounded-lg text-xs font-medium ${grpcTlsEnabled ? 'bg-green-50 text-green-700 border border-green-200' : 'bg-amber-50 text-amber-700 border border-amber-200'}`}>
                  <span className={`w-2 h-2 rounded-full ${grpcTlsEnabled ? 'bg-green-500' : 'bg-amber-500'}`}></span>
                  {grpcTlsEnabled ? 'gRPC TLS 已启用，将使用 https:// 协议连接' : 'gRPC TLS 未启用，将使用 http:// 协议连接'}
                </div>
              </div>

              {/* 步骤 1: 下载 */}
              <div className="space-y-3">
                <div className="flex items-center gap-2">
                  <div className="w-7 h-7 bg-muted text-primary rounded-full flex items-center justify-center text-sm font-bold">1</div>
                  <h4 className="text-base font-bold text-foreground">下载客户端程序</h4>
                </div>
                <div className="ml-9 space-y-2">
                  <p className="text-sm text-muted-foreground">从 GitHub Releases 下载对应平台的客户端程序：</p>
                  <a
                    href="https://github.com/oxiproxy/oxiproxy/releases/latest"
                    target="_blank"
                    rel="noopener noreferrer"
                    className="inline-flex items-center gap-2 px-4 py-2 bg-primary text-primary-foreground text-sm font-medium rounded-lg hover:bg-primary/90 transition-colors"
                  >
                    <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" strokeWidth={1.5} stroke="currentColor" className="w-4 h-4">
                      <path strokeLinecap="round" strokeLinejoin="round" d="M3 16.5v2.25A2.25 2.25 0 005.25 21h13.5A2.25 2.25 0 0021 18.75V16.5M16.5 12L12 16.5m0 0L7.5 12m4.5 4.5V3" />
                    </svg>
                    前往 GitHub Releases
                  </a>
                  <div className="mt-2 p-3 bg-muted rounded-lg">
                    <p className="text-xs text-primary font-medium">
                      {selectedPlatform === 'windows' && '下载文件: oxiproxy-client-windows-amd64.exe'}
                      {selectedPlatform === 'linux' && '下载文件: oxiproxy-client-linux-amd64'}
                      {selectedPlatform === 'macos' && '下载文件: oxiproxy-client-darwin-amd64 或 oxiproxy-client-darwin-arm64 (M系列芯片)'}
                    </p>
                  </div>
                  {selectedPlatform !== 'windows' && (
                    <div className="mt-2 p-3 bg-amber-50 rounded-lg border border-amber-200">
                      <p className="text-xs text-amber-900">
                        <span className="font-bold">重要：</span>下载后需要添加执行权限：
                      </p>
                      <code className="block mt-1 text-xs bg-amber-100 text-amber-900 px-2 py-1 rounded font-mono">
                        chmod +x client
                      </code>
                    </div>
                  )}
                </div>
              </div>

              {/* 步骤 2: 前台启动 */}
              <div className="space-y-3">
                <div className="flex items-center gap-2">
                  <div className="w-7 h-7 bg-green-100 text-green-600 rounded-full flex items-center justify-center text-sm font-bold">2</div>
                  <h4 className="text-base font-bold text-foreground">前台启动（测试用）</h4>
                </div>
                <div className="ml-9 space-y-2">
                  <p className="text-sm text-muted-foreground">在终端中运行以下命令启动客户端：</p>
                  <div className="relative">
                    <pre className="bg-primary text-muted p-4 rounded-lg text-sm font-mono overflow-x-auto select-text cursor-text">
                      {getClientStartupCommand(commandClient, selectedPlatform)}
                    </pre>
                    <button
                      onClick={() => {
                        copyToClipboard(getClientStartupCommand(commandClient, selectedPlatform));
                        showToast('命令已复制', 'success');
                      }}
                      className="absolute top-2 right-2 p-2 bg-primary/90 hover:bg-primary/80 text-muted-foreground rounded-lg transition-colors"
                      title="复制命令"
                    >
                      <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" strokeWidth={1.5} stroke="currentColor" className="w-4 h-4">
                        <path strokeLinecap="round" strokeLinejoin="round" d="M15.666 3.888A2.25 2.25 0 0013.5 2.25h-3c-1.03 0-1.9.693-2.166 1.638m7.332 0c.055.194.084.4.084.612v0a.75.75 0 01-.75.75H9a.75.75 0 01-.75-.75v0c0-.212.03-.418.084-.612m7.332 0c.646.049 1.288.11 1.927.184 1.1.128 1.907 1.077 1.907 2.185V19.5a2.25 2.25 0 01-2.25 2.25H6.75A2.25 2.25 0 014.5 19.5V6.257c0-1.108.806-2.057 1.907-2.185a48.208 48.208 0 011.927-.184" />
                      </svg>
                    </button>
                  </div>
                  <p className="text-xs text-muted-foreground">前台运行可以直接看到日志输出，适合测试和调试。按 Ctrl+C 可停止运行。</p>
                </div>
              </div>

              {/* 步骤 3: 后台运行 */}
              <div className="space-y-3">
                <div className="flex items-center gap-2">
                  <div className="w-7 h-7 bg-purple-100 text-purple-600 rounded-full flex items-center justify-center text-sm font-bold">3</div>
                  <h4 className="text-base font-bold text-foreground">后台运行（生产环境）</h4>
                </div>
                <div className="ml-9 space-y-2">
                  <p className="text-sm text-muted-foreground">
                    {selectedPlatform === 'windows'
                      ? '在 Windows 上，可以将客户端安装为系统服务：'
                      : '在 Linux/macOS 上，使用 --daemon 参数后台运行：'
                    }
                  </p>
                  <div className="relative">
                    <pre className="bg-primary text-muted p-4 rounded-lg text-sm font-mono overflow-x-auto">
                      {getClientDaemonCommand(commandClient, selectedPlatform)}
                    </pre>
                    <button
                      onClick={() => {
                        copyToClipboard(getClientDaemonCommand(commandClient, selectedPlatform));
                        showToast('命令已复制', 'success');
                      }}
                      className="absolute top-2 right-2 p-2 bg-primary/90 hover:bg-primary/80 text-muted-foreground rounded-lg transition-colors"
                      title="复制命令"
                    >
                      <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" strokeWidth={1.5} stroke="currentColor" className="w-4 h-4">
                        <path strokeLinecap="round" strokeLinejoin="round" d="M15.666 3.888A2.25 2.25 0 0013.5 2.25h-3c-1.03 0-1.9.693-2.166 1.638m7.332 0c.055.194.084.4.084.612v0a.75.75 0 01-.75.75H9a.75.75 0 01-.75-.75v0c0-.212.03-.418.084-.612m7.332 0c.646.049 1.288.11 1.927.184 1.1.128 1.907 1.077 1.907 2.185V19.5a2.25 2.25 0 01-2.25 2.25H6.75A2.25 2.25 0 014.5 19.5V6.257c0-1.108.806-2.057 1.907-2.185a48.208 48.208 0 011.927-.184" />
                      </svg>
                    </button>
                  </div>
                  {selectedPlatform === 'windows' ? (
                    <div className="space-y-2">
                      <p className="text-xs text-muted-foreground">安装为服务后，客户端会在系统启动时自动运行。</p>
                      <div className="p-3 bg-muted rounded-lg">
                        <p className="text-xs text-primary font-medium mb-1">服务管理命令：</p>
                        <code className="block text-xs bg-muted text-primary px-2 py-1 rounded font-mono mb-1">
                          client.exe uninstall-service  # 卸载服务
                        </code>
                        <code className="block text-xs bg-muted text-primary px-2 py-1 rounded font-mono">
                          sc query OxiProxyClient  # 查看服务状态
                        </code>
                      </div>
                    </div>
                  ) : (
                    <div className="space-y-2">
                      <p className="text-xs text-muted-foreground">后台运行后，日志会写入指定的日志文件。</p>
                      <div className="p-3 bg-muted rounded-lg">
                        <p className="text-xs text-primary font-medium mb-1">管理后台进程：</p>
                        <code className="block text-xs bg-muted text-primary px-2 py-1 rounded font-mono mb-1">
                          cat /var/run/oxiproxy-client.pid  # 查看进程 ID
                        </code>
                        <code className="block text-xs bg-muted text-primary px-2 py-1 rounded font-mono mb-1">
                          kill $(cat /var/run/oxiproxy-client.pid)  # 停止客户端
                        </code>
                        <code className="block text-xs bg-muted text-primary px-2 py-1 rounded font-mono">
                          tail -f /var/log/oxiproxy-client.log  # 查看日志
                        </code>
                      </div>
                    </div>
                  )}
                </div>
              </div>

              {/* 步骤 4: 验证 */}
              <div className="space-y-3">
                <div className="flex items-center gap-2">
                  <div className="w-7 h-7 bg-emerald-100 text-emerald-600 rounded-full flex items-center justify-center text-sm font-bold">4</div>
                  <h4 className="text-base font-bold text-foreground">验证客户端状态</h4>
                </div>
                <div className="ml-9 space-y-2">
                  <p className="text-sm text-muted-foreground">启动后，在本页面查看客户端状态：</p>
                  <div className="p-4 bg-gradient-to-r from-green-50 to-emerald-50 rounded-lg border border-green-200">
                    <div className="flex items-start gap-3">
                      <div className="flex-shrink-0 mt-0.5">
                        <span className="relative flex h-3 w-3">
                          <span className="animate-ping absolute inline-flex h-full w-full rounded-full bg-green-400 opacity-75"></span>
                          <span className="relative inline-flex rounded-full h-3 w-3 bg-green-500"></span>
                        </span>
                      </div>
                      <div className="flex-1">
                        <p className="text-sm font-medium text-green-900">客户端在线</p>
                        <p className="text-xs text-green-700 mt-1">
                          如果看到绿色的"在线"状态，说明客户端已成功连接到 Controller。
                        </p>
                      </div>
                    </div>
                  </div>
                </div>
              </div>

              {/* 常见问题 */}
              <div className="space-y-3">
                <div className="flex items-center gap-2">
                  <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" strokeWidth={1.5} stroke="currentColor" className="w-5 h-5 text-amber-600">
                    <path strokeLinecap="round" strokeLinejoin="round" d="M9.879 7.519c1.171-1.025 3.071-1.025 4.242 0 1.172 1.025 1.172 2.687 0 3.712-.203.179-.43.326-.67.442-.745.361-1.45.999-1.45 1.827v.75M21 12a9 9 0 11-18 0 9 9 0 0118 0zm-9 5.25h.008v.008H12v-.008z" />
                  </svg>
                  <h4 className="text-base font-bold text-foreground">常见问题</h4>
                </div>
                <div className="ml-7 space-y-3">
                  <div className="p-3 bg-muted rounded-lg">
                    <p className="text-sm font-medium text-foreground mb-1">❓ 客户端显示离线？</p>
                    <p className="text-xs text-muted-foreground">
                      检查 Controller URL 是否正确，确保网络连接正常，查看客户端日志排查错误。
                    </p>
                  </div>
                  <div className="p-3 bg-muted rounded-lg">
                    <p className="text-sm font-medium text-foreground mb-1">❓ Token 无效？</p>
                    <p className="text-xs text-muted-foreground">
                      确认复制的 Token 完整无误，没有多余的空格或换行符。
                    </p>
                  </div>
                  <div className="p-3 bg-muted rounded-lg">
                    <p className="text-sm font-medium text-foreground mb-1">❓ 如何查看客户端日志？</p>
                    <p className="text-xs text-muted-foreground">
                      前台运行时日志直接输出到终端；后台运行时查看日志文件；或在本页面点击"日志"按钮查看在线日志。
                    </p>
                  </div>
                </div>
              </div>
            </div>

            <div className="sticky bottom-0 bg-card border-t border-border p-4 flex justify-end">
              <button
                onClick={() => {
                  setShowCommandModal(false);
                  setCommandClient(null);
                }}
                className="px-5 py-2.5 bg-primary text-primary-foreground font-medium rounded-xl hover:bg-primary/90 shadow-sm transition-all"
              >
                关闭
              </button>
            </div>
          </div>
        </div>
      )}

      {/* 确认对话框 */}
      <ConfirmDialog
        open={confirmDialog.open}
        title={confirmDialog.title}
        message={confirmDialog.message}
        variant="danger"
        confirmText="确定"
        onConfirm={() => {
          confirmDialog.onConfirm();
          setConfirmDialog(prev => ({ ...prev, open: false }));
        }}
        onCancel={() => setConfirmDialog(prev => ({ ...prev, open: false }))}
      />
    </div>
  );
}
