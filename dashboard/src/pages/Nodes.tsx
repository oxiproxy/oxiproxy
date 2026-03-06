import { useEffect, useState } from 'react';
import { nodeService, systemService } from '../lib/services';
import type { Node } from '../lib/types';
import { formatDate } from '../lib/utils';
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

export default function Nodes() {
  const { showToast } = useToast();
  const [nodes, setNodes] = useState<Node[]>([]);
  const [loading, setLoading] = useState(true);
  const [showCreateModal, setShowCreateModal] = useState(false);
  const [showEditModal, setShowEditModal] = useState(false);
  const [showCommandModal, setShowCommandModal] = useState(false);
  const [selectedPlatform, setSelectedPlatform] = useState<'windows' | 'linux' | 'macos'>('linux');
  const [showLogsModal, setShowLogsModal] = useState(false);
  const [createdNodeInfo, setCreatedNodeInfo] = useState<{ name: string; secret: string } | null>(null);
  const [editingNode, setEditingNode] = useState<Node | null>(null);
  const [commandNode, setCommandNode] = useState<Node | null>(null);
  const [logsNode, setLogsNode] = useState<Node | null>(null);
  const [nodeLogs, setNodeLogs] = useState<any[]>([]);
  const [loadingLogs, setLoadingLogs] = useState(false);
  const [controllerUrl, setControllerUrl] = useState('');
  const [grpcTlsEnabled, setGrpcTlsEnabled] = useState(false);
  const [isAdmin, setIsAdmin] = useState(false);
  const [formData, setFormData] = useState({
    name: '',
    url: '',
    secret: '',
    region: '',
    description: '',
    tunnelAddr: '',
    tunnelPort: '7000',
    tunnelProtocol: 'quic',
    nodeType: 'shared',
    maxProxyCount: '',
    allowedPortRange: '',
    trafficQuotaGb: '',
    trafficResetCycle: 'none',
    speedLimit: '',
  });
  const [confirmDialog, setConfirmDialog] = useState<{ open: boolean; title: string; message: string; onConfirm: () => void }>({ open: false, title: '', message: '', onConfirm: () => {} });
  const [testingId, setTestingId] = useState<number | null>(null);
  const [latestVersion, setLatestVersion] = useState<string | null>(null);
  const [updatingNodeId, setUpdatingNodeId] = useState<number | null>(null);
  const [batchUpdating, setBatchUpdating] = useState(false);

  useEffect(() => {
    // 获取当前用户信息
    const authUser = JSON.parse(localStorage.getItem('user') || '{}');
    setIsAdmin(authUser.is_admin || false);
    loadNodes();

    // Fetch latest version (admin only)
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

  const loadNodes = async () => {
    try {
      setLoading(true);
      const response = await nodeService.getNodes();
      if (response.success && response.data) {
        setNodes(response.data);
      }
    } catch (error) {
      console.error('加载节点失败:', error);
      showToast('加载失败', 'error');
    } finally {
      setLoading(false);
    }
  };

  const handleCreateNode = async () => {
    if (!formData.name.trim()) {
      showToast('请填写名称', 'error');
      return;
    }

    try {
      const response = await nodeService.createNode({
        name: formData.name,
        url: formData.url || '',
        secret: formData.secret || undefined,
        region: formData.region || undefined,
        description: formData.description || undefined,
        tunnelAddr: formData.tunnelAddr || undefined,
        tunnelPort: formData.tunnelPort ? parseInt(formData.tunnelPort) : undefined,
        tunnelProtocol: formData.tunnelProtocol || undefined,
        nodeType: formData.nodeType || undefined,
        maxProxyCount: formData.maxProxyCount ? parseInt(formData.maxProxyCount) : undefined,
        allowedPortRange: formData.allowedPortRange || undefined,
        trafficQuotaGb: formData.trafficQuotaGb ? parseFloat(formData.trafficQuotaGb) : undefined,
        trafficResetCycle: formData.trafficResetCycle !== 'none' ? formData.trafficResetCycle : undefined,
        speedLimit: formData.speedLimit ? Math.round(parseFloat(formData.speedLimit) * 1024 * 1024) : undefined,
      });
      if (response.success) {
        showToast('节点创建成功', 'success');
        setCreatedNodeInfo({
          name: response.data!.name,
          secret: response.data!.secret,
        });
        setControllerUrl(`${window.location.hostname}:3100`);
        setShowCreateModal(false);
        setShowCommandModal(true);
        resetForm();
        loadNodes();
        // 获取 TLS 配置状态
        systemService.getGrpcTlsStatus().then(s => setGrpcTlsEnabled(s.enabled)).catch(() => {});
      } else {
        showToast(response.message || '创建失败', 'error');
      }
    } catch (error) {
      console.error('创建节点失败:', error);
      showToast('创建失败', 'error');
    }
  };

  const handleEditNode = async () => {
    if (!editingNode) return;

    try {
      const response = await nodeService.updateNode(editingNode.id, {
        name: formData.name || undefined,
        url: formData.url || undefined,
        secret: formData.secret || undefined,
        region: formData.region,
        description: formData.description,
        tunnelAddr: formData.tunnelAddr || undefined,
        tunnelPort: formData.tunnelPort ? parseInt(formData.tunnelPort) : undefined,
        tunnelProtocol: formData.tunnelProtocol || undefined,
        nodeType: formData.nodeType || undefined,
        maxProxyCount: formData.maxProxyCount ? parseInt(formData.maxProxyCount) : null,
        allowedPortRange: formData.allowedPortRange || null,
        trafficQuotaGb: formData.trafficQuotaGb ? parseFloat(formData.trafficQuotaGb) : null,
        trafficResetCycle: formData.trafficResetCycle || 'none',
        speedLimit: formData.speedLimit ? Math.round(parseFloat(formData.speedLimit) * 1024 * 1024) : null,
      });
      if (response.success) {
        showToast('节点更新成功', 'success');
        resetForm();
        setShowEditModal(false);
        setEditingNode(null);
        loadNodes();
      } else {
        showToast(response.message || '更新失败', 'error');
      }
    } catch (error) {
      console.error('更新节点失败:', error);
      showToast('更新失败', 'error');
    }
  };

  const handleDeleteNode = (node: Node) => {
    setConfirmDialog({
      open: true,
      title: '删除节点',
      message: `确定要删除节点 "${node.name}" 吗？`,
      onConfirm: async () => {
        try {
          const response = await nodeService.deleteNode(node.id);
          if (response.success) {
            showToast('节点已删除', 'success');
            loadNodes();
          } else {
            showToast(response.message || '删除失败', 'error');
          }
        } catch (error) {
          console.error('删除节点失败:', error);
          showToast('删除失败', 'error');
        }
        setConfirmDialog(prev => ({ ...prev, open: false }));
      },
    });
  };

  const handleTestConnection = async (node: Node) => {
    setTestingId(node.id);
    try {
      const response = await nodeService.testConnection(node.id);
      if (response.success && response.data) {
        if (response.data.online) {
          showToast(`节点 "${node.name}" 连接正常，已连接 ${response.data.connected_clients} 个客户端`, 'success');
        } else {
          showToast(`节点 "${node.name}" 连接失败: ${response.data.error || '未知错误'}`, 'error');
        }
      }
    } catch (error) {
      console.error('测试连接失败:', error);
      showToast('测试连接失败', 'error');
    } finally {
      setTestingId(null);
    }
  };

  const handleNodeUpdate = async (nodeId: number) => {
    setUpdatingNodeId(nodeId);
    try {
      const response = await nodeService.triggerUpdate(nodeId);
      if (response.success && response.data?.success) {
        showToast(`节点已更新到 v${response.data.newVersion}，正在重启...`, 'success');
        setTimeout(() => loadNodes(), 10000);
      } else {
        showToast(response.data?.error || response.message || '更新失败', 'error');
      }
    } catch (err: any) {
      const msg = err?.response?.data?.message || '请求失败';
      showToast(msg, 'error');
    } finally {
      setUpdatingNodeId(null);
    }
  };

  const handleBatchUpdate = () => {
    const updatableCount = nodes.filter(n => n.isOnline && n.version).length;
    if (updatableCount === 0) {
      showToast('没有可更新的在线节点', 'error');
      return;
    }
    setConfirmDialog({
      open: true,
      title: '批量更新节点',
      message: `确定要更新所有在线节点 (${updatableCount} 个) ${latestVersion ? `到最新版本 v${latestVersion}` : '到最新版本'}吗？更新后节点将自动重启。`,
      onConfirm: async () => {
        setConfirmDialog(prev => ({ ...prev, open: false }));
        setBatchUpdating(true);
        try {
          const response = await nodeService.batchUpdate();
          if (response.success && response.data?.results) {
            const results = response.data.results;
            const successCount = results.filter(r => r.success).length;
            const failCount = results.filter(r => !r.success).length;
            if (failCount === 0) {
              showToast(`全部 ${successCount} 个节点更新成功，正在重启...`, 'success');
            } else {
              const failNames = results.filter(r => !r.success).map(r => r.name || `#${r.id}`).join(', ');
              showToast(`${successCount} 个成功, ${failCount} 个失败 (${failNames})`, 'error');
            }
            setTimeout(() => loadNodes(), 10000);
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

  const openEditModal = (node: Node) => {
    setEditingNode(node);
    setFormData({
      name: node.name,
      url: node.url,
      secret: node.secret,
      region: node.region || '',
      description: node.description || '',
      tunnelAddr: node.tunnelAddr || '',
      tunnelPort: String(node.tunnelPort || 7000),
      tunnelProtocol: node.tunnelProtocol || 'quic',
      nodeType: node.nodeType || 'shared',
      maxProxyCount: node.maxProxyCount != null ? String(node.maxProxyCount) : '',
      allowedPortRange: node.allowedPortRange || '',
      trafficQuotaGb: node.trafficQuotaGb != null ? String(node.trafficQuotaGb) : '',
      trafficResetCycle: node.trafficResetCycle || 'none',
      speedLimit: node.speedLimit != null ? String(Math.round(node.speedLimit / 1024 / 1024)) : '',
    });
    setShowEditModal(true);
  };

  const resetForm = () => {
    setFormData({ name: '', url: '', secret: '', region: '', description: '', tunnelAddr: '', tunnelPort: '7000', tunnelProtocol: 'quic', nodeType: 'shared', maxProxyCount: '', allowedPortRange: '', trafficQuotaGb: '', trafficResetCycle: 'none', speedLimit: '' });
  };

  const tunnelFields = (
    <>
      <div className="border-t border-border pt-4 mt-4">
        <h3 className="text-sm font-semibold text-foreground mb-3">隧道连接配置</h3>
      </div>
      <div>
        <label className="block text-sm font-medium text-foreground mb-1.5">隧道地址 *</label>
        <input
          type="text"
          value={formData.tunnelAddr}
          onChange={(e) => setFormData({ ...formData, tunnelAddr: e.target.value })}
          className="w-full px-4 py-3 border border-border rounded-xl text-foreground placeholder-muted-foreground focus:outline-none focus:ring-2 focus:ring-primary/20 focus:border-primary transition-all bg-muted/50 hover:bg-card"
          placeholder="客户端连接的公网地址，例如：1.2.3.4"
        />
      </div>
      <div className="grid grid-cols-2 gap-3">
        <div>
          <label className="block text-sm font-medium text-foreground mb-1.5">隧道端口</label>
          <input
            type="number"
            value={formData.tunnelPort}
            onChange={(e) => setFormData({ ...formData, tunnelPort: e.target.value })}
            className="w-full px-4 py-3 border border-border rounded-xl text-foreground placeholder-muted-foreground focus:outline-none focus:ring-2 focus:ring-primary/20 focus:border-primary transition-all bg-muted/50 hover:bg-card"
            placeholder="7000"
          />
        </div>
        <div>
          <label className="block text-sm font-medium text-foreground mb-1.5">隧道协议</label>
          <select
            value={formData.tunnelProtocol}
            onChange={(e) => setFormData({ ...formData, tunnelProtocol: e.target.value })}
            className="w-full px-4 py-3 border border-border rounded-xl text-foreground focus:outline-none focus:ring-2 focus:ring-primary/20 focus:border-primary transition-all bg-muted/50 hover:bg-card"
          >
            <option value="quic">QUIC</option>
            <option value="kcp">KCP</option>
            <option value="tcp">TCP</option>
          </select>
        </div>
      </div>
    </>
  );

  const limitFields = (
    <>
      <div className="border-t border-border pt-4 mt-4">
        <h3 className="text-sm font-semibold text-foreground mb-3">节点限制配置</h3>
      </div>
      <div className="grid grid-cols-2 gap-3">
        <div>
          <label className="block text-sm font-medium text-foreground mb-1.5">最大隧道数</label>
          <input
            type="number"
            value={formData.maxProxyCount}
            onChange={(e) => setFormData({ ...formData, maxProxyCount: e.target.value })}
            className="w-full px-4 py-3 border border-border rounded-xl text-foreground placeholder-muted-foreground focus:outline-none focus:ring-2 focus:ring-primary/20 focus:border-primary transition-all bg-muted/50 hover:bg-card"
            placeholder="不限制"
            min="0"
          />
        </div>
        <div>
          <label className="block text-sm font-medium text-foreground mb-1.5">速度限制 (MB/s)</label>
          <input
            type="number"
            value={formData.speedLimit}
            onChange={(e) => setFormData({ ...formData, speedLimit: e.target.value })}
            className="w-full px-4 py-3 border border-border rounded-xl text-foreground placeholder-muted-foreground focus:outline-none focus:ring-2 focus:ring-primary/20 focus:border-primary transition-all bg-muted/50 hover:bg-card"
            placeholder="不限制"
            min="0"
            step="0.1"
          />
        </div>
      </div>
      <div>
        <label className="block text-sm font-medium text-foreground mb-1.5">允许端口范围</label>
        <input
          type="text"
          value={formData.allowedPortRange}
          onChange={(e) => setFormData({ ...formData, allowedPortRange: e.target.value })}
          className="w-full px-4 py-3 border border-border rounded-xl text-foreground placeholder-muted-foreground focus:outline-none focus:ring-2 focus:ring-primary/20 focus:border-primary transition-all bg-muted/50 hover:bg-card"
          placeholder="例如：1000-9999,20000-30000（留空不限制）"
        />
      </div>
      <div className="grid grid-cols-2 gap-3">
        <div>
          <label className="block text-sm font-medium text-foreground mb-1.5">流量配额 (GB)</label>
          <input
            type="number"
            value={formData.trafficQuotaGb}
            onChange={(e) => setFormData({ ...formData, trafficQuotaGb: e.target.value })}
            className="w-full px-4 py-3 border border-border rounded-xl text-foreground placeholder-muted-foreground focus:outline-none focus:ring-2 focus:ring-primary/20 focus:border-primary transition-all bg-muted/50 hover:bg-card"
            placeholder="不限制"
            min="0"
            step="0.1"
          />
        </div>
        <div>
          <label className="block text-sm font-medium text-foreground mb-1.5">流量重置周期</label>
          <select
            value={formData.trafficResetCycle}
            onChange={(e) => setFormData({ ...formData, trafficResetCycle: e.target.value })}
            className="w-full px-4 py-3 border border-border rounded-xl text-foreground focus:outline-none focus:ring-2 focus:ring-primary/20 focus:border-primary transition-all bg-muted/50 hover:bg-card"
          >
            <option value="none">不重置</option>
            <option value="daily">每天</option>
            <option value="monthly">每月</option>
          </select>
        </div>
      </div>
    </>
  );

  const copyToClipboard = (text: string, label: string) => {
    navigator.clipboard.writeText(text).then(() => {
      showToast(`${label}已复制到剪贴板`, 'success');
    }).catch(() => {
      showToast('复制失败，请手动复制', 'error');
    });
  };

  const getStartupCommand = (node?: Node | { name: string; secret: string }, platform: 'windows' | 'linux' | 'macos' = 'linux') => {
    if (!node) return '';
    const url = controllerUrl || `${window.location.hostname}:3100`;
    const protocol = grpcTlsEnabled ? 'https' : 'http';
    const token = node.secret;

    if (platform === 'windows') {
      return `node.exe start --controller-url ${protocol}://${url} --token ${token} --bind-port 7000`;
    } else {
      return `./node start --controller-url ${protocol}://${url} --token ${token} --bind-port 7000`;
    }
  };

  const getDaemonCommand = (node?: Node | { name: string; secret: string }, platform: 'windows' | 'linux' | 'macos' = 'linux') => {
    if (!node) return '';
    const url = controllerUrl || `${window.location.hostname}:3100`;
    const protocol = grpcTlsEnabled ? 'https' : 'http';
    const token = node.secret;

    if (platform === 'windows') {
      return `node.exe daemon --controller-url ${protocol}://${url} --token ${token} --bind-port 7000`;
    } else {
      return `./node daemon --controller-url ${protocol}://${url} --token ${token} --bind-port 7000`;
    }
  };

  const handleShowCommand = async (node: Node) => {
    setCommandNode(node);
    setControllerUrl(`${window.location.hostname}:3100`);
    setShowCommandModal(true);
    try {
      const tlsStatus = await systemService.getGrpcTlsStatus();
      setGrpcTlsEnabled(tlsStatus.enabled);
    } catch {
      setGrpcTlsEnabled(false);
    }
  };

  const handleShowLogs = async (node: Node) => {
    setLogsNode(node);
    setShowLogsModal(true);
    setLoadingLogs(true);
    setNodeLogs([]);

    try {
      const response = await nodeService.getNodeLogs(node.id, 100);
      if (response.success && response.data) {
        setNodeLogs(response.data.logs);
      } else {
        showToast(response.message || '获取日志失败', 'error');
      }
    } catch (error) {
      console.error('获取节点日志失败:', error);
      showToast('获取日志失败', 'error');
    } finally {
      setLoadingLogs(false);
    }
  };

  return (
    <div className="space-y-6">
      {/* 页面标题 */}
      <div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
        <div>
          <h2 className="text-2xl font-bold text-foreground">节点管理</h2>
          <p className="mt-1 text-sm text-muted-foreground">
            {isAdmin ? '管理和监控所有代理节点' : '查看可用的代理节点（共享节点 + 您的独享节点）'}
          </p>
        </div>
        {isAdmin && (
          <div className="flex items-center gap-2">
            {nodes.some(n => n.isOnline && n.version) && (
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
              onClick={() => { resetForm(); setShowCreateModal(true); }}
              className="inline-flex items-center gap-2 px-5 py-2.5 text-primary-foreground text-sm font-medium rounded-xl focus:outline-none focus:ring-2 focus:ring-primary/40 shadow-sm transition-all duration-200 hover:opacity-90"
              style={{ background: 'linear-gradient(135deg, hsl(210 100% 45%), hsl(189 94% 43%))' }}
            >
              <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" strokeWidth={2} stroke="currentColor" className="w-4 h-4">
                <path strokeLinecap="round" strokeLinejoin="round" d="M12 4.5v15m7.5-7.5h-15" />
              </svg>
              添加节点
            </button>
          </div>
        )}
      </div>

      {loading ? (
        <TableSkeleton rows={3} cols={7} />
      ) : (
        <TableContainer>
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>名称</TableHead>
                <TableHead>类型</TableHead>
                <TableHead>地区</TableHead>
                <TableHead>公网IP</TableHead>
                <TableHead>隧道地址</TableHead>
                <TableHead>协议</TableHead>
                <TableHead>状态</TableHead>
                <TableHead>版本</TableHead>
                <TableHead>创建时间</TableHead>
                <TableHead className="text-right">操作</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {nodes.length === 0 ? (
                <TableRow>
                  <TableCell colSpan={10} className="px-6 py-16 text-center">
                    <div className="flex flex-col items-center gap-3">
                      <div className="w-16 h-16 bg-muted rounded-full flex items-center justify-center">
                        <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" strokeWidth={1.5} stroke="currentColor" className="w-8 h-8 text-muted-foreground">
                          <path strokeLinecap="round" strokeLinejoin="round" d="M5.25 14.25h13.5m-13.5 0a3 3 0 01-3-3m3 3a3 3 0 100 6h13.5a3 3 0 100-6m-16.5-3a3 3 0 013-3h13.5a3 3 0 013 3m-19.5 0a4.5 4.5 0 01.9-2.7L5.737 5.1a3.375 3.375 0 012.7-1.35h7.126c1.062 0 2.062.5 2.7 1.35l2.587 3.45a4.5 4.5 0 01.9 2.7m0 0a3 3 0 01-3 3m0 3h.008v.008h-.008v-.008zm0-6h.008v.008h-.008v-.008zm-3 6h.008v.008h-.008v-.008zm0-6h.008v.008h-.008v-.008z" />
                        </svg>
                      </div>
                      <p className="text-muted-foreground">暂无节点</p>
                      <p className="text-sm text-muted-foreground">点击"添加节点"来添加第一个节点</p>
                    </div>
                  </TableCell>
                </TableRow>
              ) : (
                nodes.map((node) => (
                  <TableRow key={node.id}>
                    <TableCell className="whitespace-nowrap">
                      <div className="flex items-center gap-3">
                        <div className="w-10 h-10 rounded-xl flex items-center justify-center text-primary-foreground text-sm font-semibold shadow-sm" style={{ background: 'linear-gradient(135deg, hsl(210 100% 45%), hsl(189 94% 43%))' }}>
                          {node.name.charAt(0).toUpperCase()}
                        </div>
                        <div>
                          <div className="text-sm font-semibold text-foreground">{node.name}</div>
                          {node.description && (
                            <div className="text-xs text-muted-foreground">{node.description}</div>
                          )}
                        </div>
                      </div>
                    </TableCell>
                    <TableCell className="whitespace-nowrap">
                      <span className={`inline-flex items-center gap-1.5 px-2.5 py-1 text-xs font-medium rounded-lg ${
                        node.nodeType === 'dedicated'
                          ? 'bg-purple-50 text-purple-700'
                          : 'bg-muted text-primary'
                      }`}>
                        <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" strokeWidth={2} stroke="currentColor" className="w-3.5 h-3.5">
                          {node.nodeType === 'dedicated' ? (
                            <path strokeLinecap="round" strokeLinejoin="round" d="M16.5 10.5V6.75a4.5 4.5 0 10-9 0v3.75m-.75 11.25h10.5a2.25 2.25 0 002.25-2.25v-6.75a2.25 2.25 0 00-2.25-2.25H6.75a2.25 2.25 0 00-2.25 2.25v6.75a2.25 2.25 0 002.25 2.25z" />
                          ) : (
                            <path strokeLinecap="round" strokeLinejoin="round" d="M18 18.72a9.094 9.094 0 003.741-.479 3 3 0 00-4.682-2.72m.94 3.198l.001.031c0 .225-.012.447-.037.666A11.944 11.944 0 0112 21c-2.17 0-4.207-.576-5.963-1.584A6.062 6.062 0 016 18.719m12 0a5.971 5.971 0 00-.941-3.197m0 0A5.995 5.995 0 0012 12.75a5.995 5.995 0 00-5.058 2.772m0 0a3 3 0 00-4.681 2.72 8.986 8.986 0 003.74.477m.94-3.197a5.971 5.971 0 00-.94 3.197M15 6.75a3 3 0 11-6 0 3 3 0 016 0zm6 3a2.25 2.25 0 11-4.5 0 2.25 2.25 0 014.5 0zm-13.5 0a2.25 2.25 0 11-4.5 0 2.25 2.25 0 014.5 0z" />
                          )}
                        </svg>
                        {node.nodeType === 'dedicated' ? '独享' : '共享'}
                      </span>
                    </TableCell>
                    <TableCell className="whitespace-nowrap">
                      {node.region ? (
                        <span className="inline-flex items-center gap-1.5 px-2.5 py-1 text-xs font-medium rounded-lg bg-muted text-primary">
                          <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" strokeWidth={2} stroke="currentColor" className="w-3.5 h-3.5">
                            <path strokeLinecap="round" strokeLinejoin="round" d="M15 10.5a3 3 0 11-6 0 3 3 0 016 0z" />
                            <path strokeLinecap="round" strokeLinejoin="round" d="M19.5 10.5c0 7.142-7.5 11.25-7.5 11.25S4.5 17.642 4.5 10.5a7.5 7.5 0 1115 0z" />
                          </svg>
                          {node.region}
                        </span>
                      ) : (
                        <span className="text-xs text-muted-foreground">-</span>
                      )}
                    </TableCell>
                    <TableCell className="whitespace-nowrap">
                      {node.publicIp ? (
                        <span className="inline-flex items-center gap-1.5 px-2.5 py-1 text-xs font-medium rounded-lg bg-emerald-50 text-emerald-700 font-mono">
                          <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" strokeWidth={2} stroke="currentColor" className="w-3.5 h-3.5">
                            <path strokeLinecap="round" strokeLinejoin="round" d="M12 21a9.004 9.004 0 008.716-6.747M12 21a9.004 9.004 0 01-8.716-6.747M12 21c2.485 0 4.5-4.03 4.5-9S14.485 3 12 3m0 18c-2.485 0-4.5-4.03-4.5-9S9.515 3 12 3m0 0a8.997 8.997 0 017.843 4.582M12 3a8.997 8.997 0 00-7.843 4.582m15.686 0A11.953 11.953 0 0112 10.5c-2.998 0-5.74-1.1-7.843-2.918m15.686 0A8.959 8.959 0 0121 12c0 .778-.099 1.533-.284 2.253m0 0A17.919 17.919 0 0112 16.5c-3.162 0-6.133-.815-8.716-2.247m0 0A9.015 9.015 0 013 12c0-1.605.42-3.113 1.157-4.418" />
                          </svg>
                          {node.publicIp}
                        </span>
                      ) : (
                        <span className="text-xs text-muted-foreground">-</span>
                      )}
                    </TableCell>
                    <TableCell className="whitespace-nowrap">
                      <span className="text-sm text-muted-foreground font-mono">
                        {node.tunnelAddr ? `${node.tunnelAddr}:${node.tunnelPort}` : node.url}
                      </span>
                    </TableCell>
                    <TableCell className="whitespace-nowrap">
                      <span className="inline-flex items-center px-2.5 py-1 rounded-lg text-xs font-semibold bg-muted text-foreground uppercase">
                        {node.tunnelProtocol || 'quic'}
                      </span>
                    </TableCell>
                    <TableCell className="whitespace-nowrap">
                      <span className="inline-flex items-center gap-1.5 px-2.5 py-1 rounded-lg text-xs font-semibold"
                        style={node.isOnline
                          ? { background: 'hsl(142 71% 45% / 0.12)', color: 'hsl(142 71% 45%)' }
                          : { background: 'hsl(0 84.2% 60.2% / 0.12)', color: 'hsl(0 84.2% 60.2%)' }
                        }
                      >
                        <span className="w-1.5 h-1.5 rounded-full" style={{ background: node.isOnline ? 'hsl(142 71% 45%)' : 'hsl(0 84.2% 60.2%)' }}></span>
                        {node.isOnline ? '在线' : '离线'}
                      </span>
                    </TableCell>
                    <TableCell className="whitespace-nowrap">
                      <div className="flex items-center gap-1.5">
                        {!node.version ? (
                          <span className="text-xs text-muted-foreground">-</span>
                        ) : node.version === latestVersion ? (
                          <span className="inline-flex items-center gap-1.5 px-2.5 py-1 text-xs font-medium rounded-lg" style={{ background: 'hsl(142 71% 45% / 0.12)', color: 'hsl(142 71% 45%)' }}>
                            v{node.version}
                          </span>
                        ) : latestVersion ? (
                          <span className="inline-flex items-center px-2.5 py-1 text-xs font-medium rounded-lg bg-amber-50 text-amber-700">
                            v{node.version}
                          </span>
                        ) : (
                          <span className="inline-flex items-center px-2.5 py-1 text-xs font-medium rounded-lg bg-muted text-muted-foreground">
                            v{node.version}
                          </span>
                        )}
                        {isAdmin && node.isOnline && node.version && (
                          <button
                            onClick={() => handleNodeUpdate(node.id)}
                            disabled={updatingNodeId === node.id}
                            className={`inline-flex items-center gap-1 px-2 py-1 text-xs font-medium rounded-lg transition-colors disabled:opacity-50 disabled:cursor-not-allowed ${
                              node.version !== latestVersion && latestVersion
                                ? 'text-amber-600 hover:bg-amber-50'
                                : 'text-muted-foreground hover:bg-muted'
                            }`}
                            title={latestVersion ? `更新到 v${latestVersion}` : '重新安装当前版本'}
                          >
                            {updatingNodeId === node.id ? (
                              <>
                                <svg className="animate-spin w-3 h-3" xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24">
                                  <circle className="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="4"></circle>
                                  <path className="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"></path>
                                </svg>
                                更新中...
                              </>
                            ) : (
                              <>
                                <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" strokeWidth={2} stroke="currentColor" className="w-3 h-3">
                                  <path strokeLinecap="round" strokeLinejoin="round" d="M3 16.5v2.25A2.25 2.25 0 005.25 21h13.5A2.25 2.25 0 0021 18.75V16.5M16.5 12L12 16.5m0 0L7.5 12m4.5 4.5V3" />
                                </svg>
                                {node.version !== latestVersion && latestVersion ? '更新' : '重装'}
                              </>
                            )}
                          </button>
                        )}
                      </div>
                    </TableCell>
                    <TableCell className="whitespace-nowrap text-sm text-muted-foreground">
                      {formatDate(node.created_at)}
                    </TableCell>
                    <TableCell className="whitespace-nowrap text-right">
                      <div className="flex flex-wrap items-center justify-end gap-1.5">
                        {isAdmin && (
                          <>
                            <button
                              onClick={() => handleShowCommand(node)}
                              className="inline-flex items-center gap-1.5 px-3 py-1.5 text-xs font-medium text-primary hover:bg-accent rounded-lg transition-colors"
                            >
                              <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" strokeWidth={2} stroke="currentColor" className="w-3.5 h-3.5">
                                <path strokeLinecap="round" strokeLinejoin="round" d="M6.75 7.5l3 2.25-3 2.25m4.5 0h3m-9 8.25h13.5A2.25 2.25 0 0021 18V6a2.25 2.25 0 00-2.25-2.25H5.25A2.25 2.25 0 003 6v12a2.25 2.25 0 002.25 2.25z" />
                              </svg>
                              启动命令
                            </button>
                            <button
                              onClick={() => handleShowLogs(node)}
                              disabled={!node.isOnline}
                              className="inline-flex items-center gap-1.5 px-3 py-1.5 text-xs font-medium text-purple-600 hover:bg-purple-50 rounded-lg transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
                            >
                              <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" strokeWidth={2} stroke="currentColor" className="w-3.5 h-3.5">
                                <path strokeLinecap="round" strokeLinejoin="round" d="M19.5 14.25v-2.625a3.375 3.375 0 00-3.375-3.375h-1.5A1.125 1.125 0 0113.5 7.125v-1.5a3.375 3.375 0 00-3.375-3.375H8.25m0 12.75h7.5m-7.5 3H12M10.5 2.25H5.625c-.621 0-1.125.504-1.125 1.125v17.25c0 .621.504 1.125 1.125 1.125h12.75c.621 0 1.125-.504 1.125-1.125V11.25a9 9 0 00-9-9z" />
                              </svg>
                              查看日志
                            </button>
                            <button
                              onClick={() => handleTestConnection(node)}
                              disabled={testingId === node.id}
                              className="inline-flex items-center gap-1.5 px-3 py-1.5 text-xs font-medium text-green-600 hover:bg-green-50 rounded-lg transition-colors disabled:opacity-50"
                            >
                              <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" strokeWidth={2} stroke="currentColor" className="w-3.5 h-3.5">
                                <path strokeLinecap="round" strokeLinejoin="round" d="M9.348 14.652a3.75 3.75 0 010-5.304m5.304 0a3.75 3.75 0 010 5.304m-7.425 2.121a6.75 6.75 0 010-9.546m9.546 0a6.75 6.75 0 010 9.546M5.106 18.894c-3.808-3.807-3.808-9.98 0-13.788m13.788 0c3.808 3.807 3.808 9.98 0 13.788M12 12h.008v.008H12V12zm.375 0a.375.375 0 11-.75 0 .375.375 0 01.75 0z" />
                              </svg>
                              {testingId === node.id ? '测试中...' : '测试'}
                            </button>
                            <button
                              onClick={() => openEditModal(node)}
                              className="inline-flex items-center gap-1.5 px-3 py-1.5 text-xs font-medium text-primary hover:bg-accent rounded-lg transition-colors"
                            >
                              <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" strokeWidth={2} stroke="currentColor" className="w-3.5 h-3.5">
                                <path strokeLinecap="round" strokeLinejoin="round" d="M16.862 4.487l1.687-1.688a1.875 1.875 0 112.652 2.652L10.582 16.07a4.5 4.5 0 01-1.897 1.13L6 18l.8-2.685a4.5 4.5 0 011.13-1.897l8.932-8.931zm0 0L19.5 7.125M18 14v4.75A2.25 2.25 0 0115.75 21H5.25A2.25 2.25 0 013 18.75V8.25A2.25 2.25 0 015.25 6H10" />
                              </svg>
                              编辑
                            </button>
                            <button
                              onClick={() => handleDeleteNode(node)}
                              className="inline-flex items-center gap-1.5 px-3 py-1.5 text-xs font-medium text-red-600 hover:bg-red-50 rounded-lg transition-colors"
                            >
                              <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" strokeWidth={2} stroke="currentColor" className="w-3.5 h-3.5">
                                <path strokeLinecap="round" strokeLinejoin="round" d="M14.74 9l-.346 9m-4.788 0L9.26 9m9.968-3.21c.342.052.682.107 1.022.166m-1.022-.165L18.16 19.673a2.25 2.25 0 01-2.244 2.077H8.084a2.25 2.25 0 01-2.244-2.077L4.772 5.79m14.456 0a48.108 48.108 0 00-3.478-.397m-12 .562c.34-.059.68-.114 1.022-.165m0 0a48.11 48.11 0 013.478-.397m7.5 0v-.916c0-1.18-.91-2.164-2.09-2.201a51.964 51.964 0 00-3.32 0c-1.18.037-2.09 1.022-2.09 2.201v.916m7.5 0a48.667 48.667 0 00-7.5 0" />
                              </svg>
                              删除
                            </button>
                          </>
                        )}
                        {!isAdmin && (
                          <span className="text-xs text-muted-foreground px-3 py-1.5">
                            {node.nodeType === 'shared' ? '所有用户可用' : '已分配给您'}
                          </span>
                        )}
                      </div>
                    </TableCell>
                  </TableRow>
                ))
              )}
            </TableBody>
          </Table>
        </TableContainer>
      )}

      {/* 创建节点弹窗 */}
      {showCreateModal && (
        <div className="fixed inset-0 bg-black/50 backdrop-blur-sm overflow-y-auto h-full w-full flex items-center justify-center z-50">
          <div className="relative bg-card rounded-2xl shadow-2xl w-full max-w-md mx-4 max-h-[90vh] overflow-y-auto transform transition-all">
            <div className="p-6">
              <div className="flex items-center gap-3 mb-6">
                <div className="w-10 h-10 rounded-xl flex items-center justify-center" style={{ background: 'linear-gradient(135deg, hsl(210 100% 45%), hsl(189 94% 43%))' }}>
                  <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" strokeWidth={1.5} stroke="currentColor" className="w-5 h-5 text-primary-foreground">
                    <path strokeLinecap="round" strokeLinejoin="round" d="M12 4.5v15m7.5-7.5h-15" />
                  </svg>
                </div>
                <div>
                  <h3 className="text-lg font-bold text-foreground">添加节点</h3>
                  <p className="text-sm text-muted-foreground">配置新的代理节点</p>
                </div>
              </div>
              <div className="space-y-4">
                <div>
                  <label className="block text-sm font-medium text-foreground mb-1.5">名称 *</label>
                  <input
                    type="text"
                    value={formData.name}
                    onChange={(e) => setFormData({ ...formData, name: e.target.value })}
                    className="w-full px-4 py-3 border border-border rounded-xl text-foreground placeholder-muted-foreground focus:outline-none focus:ring-2 focus:ring-primary/20 focus:border-primary transition-all bg-muted/50 hover:bg-card"
                    placeholder="例如：US-East-1"
                  />
                </div>
                <div>
                  <label className="block text-sm font-medium text-foreground mb-1.5">节点密钥 (Token)</label>
                  <input
                    type="text"
                    value={formData.secret}
                    onChange={(e) => setFormData({ ...formData, secret: e.target.value })}
                    className="w-full px-4 py-3 border border-border rounded-xl text-foreground placeholder-muted-foreground focus:outline-none focus:ring-2 focus:ring-primary/20 focus:border-primary transition-all bg-muted/50 hover:bg-card"
                    placeholder="留空则系统自动生成"
                  />
                  <p className="text-xs text-muted-foreground mt-1.5">节点启动时通过此密钥向 Controller 注册，留空将自动生成</p>
                </div>
                <div>
                  <label className="block text-sm font-medium text-foreground mb-1.5">地区</label>
                  <input
                    type="text"
                    value={formData.region}
                    onChange={(e) => setFormData({ ...formData, region: e.target.value })}
                    className="w-full px-4 py-3 border border-border rounded-xl text-foreground placeholder-muted-foreground focus:outline-none focus:ring-2 focus:ring-primary/20 focus:border-primary transition-all bg-muted/50 hover:bg-card"
                    placeholder="例如：华东"
                  />
                </div>
                <div>
                  <label className="block text-sm font-medium text-foreground mb-1.5">描述</label>
                  <input
                    type="text"
                    value={formData.description}
                    onChange={(e) => setFormData({ ...formData, description: e.target.value })}
                    className="w-full px-4 py-3 border border-border rounded-xl text-foreground placeholder-muted-foreground focus:outline-none focus:ring-2 focus:ring-primary/20 focus:border-primary transition-all bg-muted/50 hover:bg-card"
                    placeholder="可选描述"
                  />
                </div>
                <div>
                  <label className="block text-sm font-medium text-foreground mb-1.5">节点类型</label>
                  <select
                    value={formData.nodeType}
                    onChange={(e) => setFormData({ ...formData, nodeType: e.target.value })}
                    className="w-full px-4 py-3 border border-border rounded-xl text-foreground focus:outline-none focus:ring-2 focus:ring-primary/20 focus:border-primary transition-all bg-muted/50 hover:bg-card"
                  >
                    <option value="shared">共享节点</option>
                    <option value="dedicated">独享节点</option>
                  </select>
                  <p className="text-xs text-muted-foreground mt-1.5">共享节点可被多个用户使用，独享节点仅分配给特定用户</p>
                </div>
                {tunnelFields}
                {limitFields}
              </div>
              <div className="mt-6 flex gap-3">
                <button
                  onClick={() => setShowCreateModal(false)}
                  className="flex-1 px-4 py-2.5 bg-muted text-foreground font-medium rounded-xl hover:bg-accent transition-colors"
                >
                  取消
                </button>
                <button
                  onClick={handleCreateNode}
                  className="flex-1 px-4 py-2.5 bg-primary text-primary-foreground font-medium rounded-xl hover:bg-primary/90 shadow-sm transition-all"
                >
                  创建
                </button>
              </div>
            </div>
          </div>
        </div>
      )}

      {/* 编辑节点弹窗 */}
      {showEditModal && (
        <div className="fixed inset-0 bg-black/50 backdrop-blur-sm overflow-y-auto h-full w-full flex items-center justify-center z-50">
          <div className="relative bg-card rounded-2xl shadow-2xl w-full max-w-md mx-4 max-h-[90vh] overflow-y-auto transform transition-all">
            <div className="p-6">
              <div className="flex items-center gap-3 mb-6">
                <div className="w-10 h-10 rounded-xl flex items-center justify-center" style={{ background: 'linear-gradient(135deg, hsl(210 100% 45%), hsl(189 94% 43%))' }}>
                  <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" strokeWidth={1.5} stroke="currentColor" className="w-5 h-5 text-primary-foreground">
                    <path strokeLinecap="round" strokeLinejoin="round" d="M16.862 4.487l1.687-1.688a1.875 1.875 0 112.652 2.652L10.582 16.07a4.5 4.5 0 01-1.897 1.13L6 18l.8-2.685a4.5 4.5 0 011.13-1.897l8.932-8.931zm0 0L19.5 7.125M18 14v4.75A2.25 2.25 0 0115.75 21H5.25A2.25 2.25 0 013 18.75V8.25A2.25 2.25 0 015.25 6H10" />
                  </svg>
                </div>
                <div>
                  <h3 className="text-lg font-bold text-foreground">编辑节点</h3>
                  <p className="text-sm text-muted-foreground">修改节点配置信息</p>
                </div>
              </div>
              <div className="space-y-4">
                <div>
                  <label className="block text-sm font-medium text-foreground mb-1.5">名称</label>
                  <input
                    type="text"
                    value={formData.name}
                    onChange={(e) => setFormData({ ...formData, name: e.target.value })}
                    className="w-full px-4 py-3 border border-border rounded-xl text-foreground placeholder-muted-foreground focus:outline-none focus:ring-2 focus:ring-primary/20 focus:border-primary transition-all bg-muted/50 hover:bg-card"
                  />
                </div>
                <div>
                  <label className="block text-sm font-medium text-foreground mb-1.5">内部 API 地址</label>
                  <input
                    type="text"
                    value={formData.url}
                    onChange={(e) => setFormData({ ...formData, url: e.target.value })}
                    className="w-full px-4 py-3 border border-border rounded-xl text-foreground placeholder-muted-foreground focus:outline-none focus:ring-2 focus:ring-primary/20 focus:border-primary transition-all bg-muted/50 hover:bg-card"
                  />
                </div>
                <div>
                  <label className="block text-sm font-medium text-foreground mb-1.5">共享密钥</label>
                  <input
                    type="password"
                    value={formData.secret}
                    onChange={(e) => setFormData({ ...formData, secret: e.target.value })}
                    className="w-full px-4 py-3 border border-border rounded-xl text-foreground placeholder-muted-foreground focus:outline-none focus:ring-2 focus:ring-primary/20 focus:border-primary transition-all bg-muted/50 hover:bg-card"
                    placeholder="留空则不修改"
                  />
                </div>
                <div>
                  <label className="block text-sm font-medium text-foreground mb-1.5">地区</label>
                  <input
                    type="text"
                    value={formData.region}
                    onChange={(e) => setFormData({ ...formData, region: e.target.value })}
                    className="w-full px-4 py-3 border border-border rounded-xl text-foreground placeholder-muted-foreground focus:outline-none focus:ring-2 focus:ring-primary/20 focus:border-primary transition-all bg-muted/50 hover:bg-card"
                  />
                </div>
                <div>
                  <label className="block text-sm font-medium text-foreground mb-1.5">描述</label>
                  <input
                    type="text"
                    value={formData.description}
                    onChange={(e) => setFormData({ ...formData, description: e.target.value })}
                    className="w-full px-4 py-3 border border-border rounded-xl text-foreground placeholder-muted-foreground focus:outline-none focus:ring-2 focus:ring-primary/20 focus:border-primary transition-all bg-muted/50 hover:bg-card"
                  />
                </div>
                <div>
                  <label className="block text-sm font-medium text-foreground mb-1.5">节点类型</label>
                  <select
                    value={formData.nodeType}
                    onChange={(e) => setFormData({ ...formData, nodeType: e.target.value })}
                    className="w-full px-4 py-3 border border-border rounded-xl text-foreground focus:outline-none focus:ring-2 focus:ring-primary/20 focus:border-primary transition-all bg-muted/50 hover:bg-card"
                  >
                    <option value="shared">共享节点</option>
                    <option value="dedicated">独享节点</option>
                  </select>
                  <p className="text-xs text-muted-foreground mt-1.5">共享节点可被多个用户使用，独享节点仅分配给特定用户</p>
                </div>
                {tunnelFields}
                {limitFields}
              </div>
              <div className="mt-6 flex gap-3">
                <button
                  onClick={() => { setShowEditModal(false); setEditingNode(null); }}
                  className="flex-1 px-4 py-2.5 bg-muted text-foreground font-medium rounded-xl hover:bg-accent transition-colors"
                >
                  取消
                </button>
                <button
                  onClick={handleEditNode}
                  className="flex-1 px-4 py-2.5 bg-primary text-primary-foreground font-medium rounded-xl hover:bg-primary/90 shadow-sm transition-all"
                >
                  保存
                </button>
              </div>
            </div>
          </div>
        </div>
      )}

      {/* 启动命令弹窗 */}
      {showCommandModal && (createdNodeInfo || commandNode) && (
        <div className="fixed inset-0 bg-black/50 backdrop-blur-sm overflow-y-auto h-full w-full flex items-center justify-center z-50 p-4">
          <div className="relative bg-card rounded-2xl shadow-2xl w-full max-w-3xl max-h-[90vh] overflow-y-auto transform transition-all">
            <div className="p-6">
              <div className="flex items-center gap-3 mb-6">
                <div className="w-10 h-10 bg-gradient-to-br from-green-500 to-emerald-600 rounded-xl flex items-center justify-center">
                  <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" strokeWidth={1.5} stroke="currentColor" className="w-5 h-5 text-primary-foreground">
                    <path strokeLinecap="round" strokeLinejoin="round" d="M6.75 7.5l3 2.25-3 2.25m4.5 0h3m-9 8.25h13.5A2.25 2.25 0 0021 18V6a2.25 2.25 0 00-2.25-2.25H5.25A2.25 2.25 0 003 6v12a2.25 2.25 0 002.25 2.25z" />
                  </svg>
                </div>
                <div>
                  <h3 className="text-lg font-bold text-foreground">
                    {createdNodeInfo ? '节点创建成功 - 启动教程' : '节点启动教程'}
                  </h3>
                  <p className="text-sm text-muted-foreground">
                    按照以下步骤在目标服务器上启动节点
                  </p>
                </div>
              </div>

              {/* 平台选择 */}
              <div className="mb-6">
                <label className="block text-sm font-medium text-foreground mb-3">选择操作系统</label>
                <div className="grid grid-cols-3 gap-3">
                  {[
                    { value: 'linux', label: 'Linux', icon: '🐧' },
                    { value: 'windows', label: 'Windows', icon: '🪟' },
                    { value: 'macos', label: 'macOS', icon: '🍎' }
                  ].map((platform) => (
                    <button
                      key={platform.value}
                      onClick={() => setSelectedPlatform(platform.value as any)}
                      className={`px-4 py-3 rounded-xl border-2 transition-all ${
                        selectedPlatform === platform.value
                          ? 'border-primary bg-muted text-primary'
                          : 'border-border hover:border-border text-foreground'
                      }`}
                    >
                      <div className="text-2xl mb-1">{platform.icon}</div>
                      <div className="text-sm font-medium">{platform.label}</div>
                    </button>
                  ))}
                </div>
              </div>

              {/* Controller 地址 */}
              <div className="mb-6">
                <label className="block text-sm font-medium text-foreground mb-2">Controller 地址</label>
                <input
                  type="text"
                  value={controllerUrl}
                  onChange={(e) => setControllerUrl(e.target.value)}
                  placeholder="例如: 192.168.1.100:3100"
                  className="w-full px-4 py-2.5 border border-border rounded-xl text-foreground focus:outline-none focus:ring-2 focus:ring-primary/20 focus:border-primary transition-all bg-muted/50 hover:bg-card font-mono text-sm"
                />
                <p className="mt-1.5 text-xs text-muted-foreground">
                  修改为节点服务器可以访问的 Controller 地址（IP:端口）
                </p>
                <div className={`mt-2 flex items-center gap-2 px-3 py-2 rounded-lg text-xs font-medium ${grpcTlsEnabled ? 'bg-green-50 text-green-700 border border-green-200' : 'bg-amber-50 text-amber-700 border border-amber-200'}`}>
                  <span className={`w-2 h-2 rounded-full ${grpcTlsEnabled ? 'bg-green-500' : 'bg-amber-500'}`}></span>
                  {grpcTlsEnabled ? 'gRPC TLS 已启用，将使用 https:// 协议连接' : 'gRPC TLS 未启用，将使用 http:// 协议连接'}
                </div>
              </div>

              {/* 步骤 1: 下载 */}
              <div className="mb-6">
                <div className="flex items-center gap-2 mb-3">
                  <div className="w-6 h-6 bg-primary text-primary-foreground rounded-full flex items-center justify-center text-xs font-bold">1</div>
                  <h4 className="text-sm font-semibold text-foreground">下载节点程序</h4>
                </div>
                <div className="bg-muted rounded-xl p-4 border border-border">
                  <p className="text-sm text-foreground mb-3">从 GitHub Releases 下载对应平台的节点程序：</p>
                  <a
                    href="https://github.com/oxiproxy/oxiproxy/releases/latest"
                    target="_blank"
                    rel="noopener noreferrer"
                    className="inline-flex items-center gap-2 text-sm text-primary hover:text-primary/80 font-medium"
                  >
                    <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" strokeWidth={2} stroke="currentColor" className="w-4 h-4">
                      <path strokeLinecap="round" strokeLinejoin="round" d="M13.5 6H5.25A2.25 2.25 0 003 8.25v10.5A2.25 2.25 0 005.25 21h10.5A2.25 2.25 0 0018 18.75V10.5m-10.5 6L21 3m0 0h-5.25M21 3v5.25" />
                    </svg>
                    下载最新版本
                  </a>
                  <div className="mt-3 text-xs text-muted-foreground">
                    <p className="font-medium mb-1">文件名参考：</p>
                    <ul className="space-y-1 ml-4">
                      {selectedPlatform === 'linux' && <li>• node-x86_64-unknown-linux-musl.tar.gz</li>}
                      {selectedPlatform === 'windows' && <li>• node-x86_64-pc-windows-msvc.zip</li>}
                      {selectedPlatform === 'macos' && <li>• node-x86_64-apple-darwin.tar.gz</li>}
                    </ul>
                  </div>
                </div>
              </div>

              {/* 步骤 2: 启动命令 */}
              <div className="mb-6">
                <div className="flex items-center gap-2 mb-3">
                  <div className="w-6 h-6 bg-primary text-primary-foreground rounded-full flex items-center justify-center text-xs font-bold">2</div>
                  <h4 className="text-sm font-semibold text-foreground">启动节点（前台运行）</h4>
                </div>
                <div className="relative">
                  <button
                    onClick={() => copyToClipboard(getStartupCommand(createdNodeInfo || commandNode!, selectedPlatform), '启动命令')}
                    className="absolute top-3 right-3 inline-flex items-center gap-1.5 px-3 py-1.5 text-xs font-medium text-primary hover:text-primary/80 bg-card/90 hover:bg-card rounded-lg transition-colors shadow-sm"
                  >
                    <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" strokeWidth={2} stroke="currentColor" className="w-3.5 h-3.5">
                      <path strokeLinecap="round" strokeLinejoin="round" d="M15.666 3.888A2.25 2.25 0 0013.5 2.25h-3c-1.03 0-1.9.693-2.166 1.638m7.332 0c.055.194.084.4.084.612v0a.75.75 0 01-.75.75H9.75a.75.75 0 01-.75-.75v0c0-.212.03-.418.084-.612m7.332 0c.646.049 1.288.11 1.927.184 1.1.128 1.907 1.077 1.907 2.185V19.5a2.25 2.25 0 01-2.25 2.25H6.75A2.25 2.25 0 014.5 19.5V6.257c0-1.108.806-2.057 1.907-2.185a48.208 48.208 0 011.927-.184" />
                    </svg>
                    复制
                  </button>
                  <pre className="bg-primary text-green-400 rounded-xl px-4 py-3 pr-24 text-sm font-mono overflow-x-auto select-text cursor-text">{getStartupCommand(createdNodeInfo || commandNode!, selectedPlatform)}</pre>
                </div>
              </div>

              {/* 步骤 3: 后台运行 */}
              <div className="mb-6">
                <div className="flex items-center gap-2 mb-3">
                  <div className="w-6 h-6 bg-primary text-primary-foreground rounded-full flex items-center justify-center text-xs font-bold">3</div>
                  <h4 className="text-sm font-semibold text-foreground">后台运行（可选）</h4>
                </div>
                <div className="relative">
                  <button
                    onClick={() => copyToClipboard(getDaemonCommand(createdNodeInfo || commandNode!, selectedPlatform), '后台运行命令')}
                    className="absolute top-3 right-3 inline-flex items-center gap-1.5 px-3 py-1.5 text-xs font-medium text-primary hover:text-primary/80 bg-card/90 hover:bg-card rounded-lg transition-colors shadow-sm"
                  >
                    <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" strokeWidth={2} stroke="currentColor" className="w-3.5 h-3.5">
                      <path strokeLinecap="round" strokeLinejoin="round" d="M15.666 3.888A2.25 2.25 0 0013.5 2.25h-3c-1.03 0-1.9.693-2.166 1.638m7.332 0c.055.194.084.4.084.612v0a.75.75 0 01-.75.75H9.75a.75.75 0 01-.75-.75v0c0-.212.03-.418.084-.612m7.332 0c.646.049 1.288.11 1.927.184 1.1.128 1.907 1.077 1.907 2.185V19.5a2.25 2.25 0 01-2.25 2.25H6.75A2.25 2.25 0 014.5 19.5V6.257c0-1.108.806-2.057 1.907-2.185a48.208 48.208 0 011.927-.184" />
                    </svg>
                    复制
                  </button>
                  <pre className="bg-primary text-green-400 rounded-xl px-4 py-3 pr-24 text-sm font-mono overflow-x-auto">{getDaemonCommand(createdNodeInfo || commandNode!, selectedPlatform)}</pre>
                </div>
                <p className="mt-2 text-xs text-muted-foreground">
                  {selectedPlatform === 'windows'
                    ? '使用 daemon 子命令在后台运行节点，日志输出到文件'
                    : '使用 daemon 子命令在后台运行节点，进程会持续运行'}
                </p>
              </div>

              {/* 步骤 4: 验证 */}
              <div className="mb-6">
                <div className="flex items-center gap-2 mb-3">
                  <div className="w-6 h-6 bg-primary text-primary-foreground rounded-full flex items-center justify-center text-xs font-bold">4</div>
                  <h4 className="text-sm font-semibold text-foreground">验证节点状态</h4>
                </div>
                <div className="bg-green-50 border border-green-200 rounded-xl px-4 py-3">
                  <p className="text-sm text-green-800">
                    节点启动后会自动向 Controller 注册。刷新本页面，如果节点状态显示为
                    <span className="inline-flex items-center gap-1 mx-1 px-2 py-0.5 bg-green-100 text-green-700 rounded-full text-xs font-medium">
                      <span className="w-1.5 h-1.5 bg-green-500 rounded-full"></span>
                      在线
                    </span>
                    则表示连接成功。
                  </p>
                </div>
              </div>

              {/* 常见问题 */}
              <div className="bg-amber-50 border border-amber-200 rounded-xl px-4 py-3 mb-6">
                <div className="flex items-start gap-2">
                  <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" strokeWidth={2} stroke="currentColor" className="w-4 h-4 text-amber-600 mt-0.5 flex-shrink-0">
                    <path strokeLinecap="round" strokeLinejoin="round" d="M12 9v3.75m9-.75a9 9 0 11-18 0 9 9 0 0118 0zm-9 3.75h.008v.008H12v-.008z" />
                  </svg>
                  <div className="text-sm text-amber-800">
                    <p className="font-semibold mb-2">常见问题：</p>
                    <ul className="space-y-1 text-xs">
                      <li>• 确保防火墙允许 7000 端口（UDP）的入站连接</li>
                      <li>• 确保节点服务器可以访问 Controller 地址</li>
                      <li>• 如果连接失败，检查 token 是否正确</li>
                      <li>• 查看节点日志以获取详细错误信息</li>
                    </ul>
                  </div>
                </div>
              </div>

              <div className="flex justify-end">
                <button
                  onClick={() => {
                    setShowCommandModal(false);
                    setCreatedNodeInfo(null);
                    setCommandNode(null);
                  }}
                  className="px-5 py-2.5 bg-primary text-primary-foreground font-medium rounded-xl hover:bg-primary/90 shadow-sm transition-all"
                >
                  知道了
                </button>
              </div>
            </div>
          </div>
        </div>
      )}

      {/* 日志查看弹窗 */}
      {showLogsModal && logsNode && (
        <div className="fixed inset-0 bg-black/50 backdrop-blur-sm overflow-y-auto h-full w-full flex items-center justify-center z-50">
          <div className="relative bg-card rounded-2xl shadow-2xl w-full max-w-4xl mx-4 max-h-[90vh] overflow-hidden transform transition-all">
            <div className="p-6">
              <div className="flex items-center justify-between mb-6">
                <div className="flex items-center gap-3">
                  <div className="w-10 h-10 rounded-xl flex items-center justify-center" style={{ background: 'linear-gradient(135deg, hsl(210 100% 45%), hsl(189 94% 43%))' }}>
                    <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" strokeWidth={1.5} stroke="currentColor" className="w-5 h-5 text-primary-foreground">
                      <path strokeLinecap="round" strokeLinejoin="round" d="M19.5 14.25v-2.625a3.375 3.375 0 00-3.375-3.375h-1.5A1.125 1.125 0 0113.5 7.125v-1.5a3.375 3.375 0 00-3.375-3.375H8.25m0 12.75h7.5m-7.5 3H12M10.5 2.25H5.625c-.621 0-1.125.504-1.125 1.125v17.25c0 .621.504 1.125 1.125 1.125h12.75c.621 0 1.125-.504 1.125-1.125V11.25a9 9 0 00-9-9z" />
                    </svg>
                  </div>
                  <div>
                    <h3 className="text-lg font-bold text-foreground">节点日志</h3>
                    <p className="text-sm text-muted-foreground">{logsNode.name} - 最近 100 行</p>
                  </div>
                </div>
                <button
                  onClick={() => {
                    setShowLogsModal(false);
                    setLogsNode(null);
                    setNodeLogs([]);
                  }}
                  className="text-muted-foreground hover:text-muted-foreground transition-colors"
                >
                  <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" strokeWidth={2} stroke="currentColor" className="w-6 h-6">
                    <path strokeLinecap="round" strokeLinejoin="round" d="M6 18L18 6M6 6l12 12" />
                  </svg>
                </button>
              </div>

              {loadingLogs ? (
                <div className="flex items-center justify-center py-12">
                  <div className="animate-spin rounded-full h-12 w-12 border-b-2 border-purple-600"></div>
                </div>
              ) : (
                <div className="bg-primary rounded-xl p-4 max-h-[60vh] overflow-y-auto">
                  {nodeLogs.length === 0 ? (
                    <div className="text-center py-8 text-muted-foreground">
                      暂无日志数据
                    </div>
                  ) : (
                    <div className="space-y-1 font-mono text-sm">
                      {nodeLogs.map((log, index) => (
                        <div key={index} className="flex gap-3 text-muted-foreground hover:bg-primary/50 px-2 py-1 rounded">
                          <span className="text-muted-foreground flex-shrink-0">{log.timestamp}</span>
                          <span className={`flex-shrink-0 font-semibold ${
                            log.level === 'ERROR' ? 'text-red-400' :
                            log.level === 'WARN' ? 'text-yellow-400' :
                            log.level === 'INFO' ? 'text-muted-foreground' :
                            'text-muted-foreground'
                          }`}>{log.level}</span>
                          <span className="text-muted break-all">{log.message}</span>
                        </div>
                      ))}
                    </div>
                  )}
                </div>
              )}

              <div className="mt-6 flex justify-end">
                <button
                  onClick={() => {
                    setShowLogsModal(false);
                    setLogsNode(null);
                    setNodeLogs([]);
                  }}
                  className="px-5 py-2.5 bg-primary text-primary-foreground font-medium rounded-xl hover:bg-primary/90 shadow-sm transition-all"
                >
                  关闭
                </button>
              </div>
            </div>
          </div>
        </div>
      )}

      <ConfirmDialog
        open={confirmDialog.open}
        title={confirmDialog.title}
        message={confirmDialog.message}
        onConfirm={confirmDialog.onConfirm}
        onCancel={() => setConfirmDialog(prev => ({ ...prev, open: false }))}
      />
    </div>
  );
}
