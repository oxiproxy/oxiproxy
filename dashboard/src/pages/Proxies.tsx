import { useEffect, useState, Fragment } from 'react';
import { proxyService, clientService, nodeService, userService } from '../lib/services';
import type { Proxy, Client, Node, ProxyGroup, ProxyDisplayRow, User } from '../lib/types';
import { formatBytes } from '../lib/utils';
import { useToast } from '../contexts/ToastContext';
import { getStoredJson } from '../lib/api';
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

export default function Proxies() {
  const { showToast } = useToast();
  const [proxies, setProxies] = useState<Proxy[]>([]);
  const [clients, setClients] = useState<Client[]>([]);
  const [nodes, setNodes] = useState<Node[]>([]);
  const [availableNodes, setAvailableNodes] = useState<Node[]>([]);
  const [loading, setLoading] = useState(true);
  const [showCreateModal, setShowCreateModal] = useState(false);
  const [editingProxy, setEditingProxy] = useState<Proxy | null>(null);
  const [confirmDialog, setConfirmDialog] = useState<{ open: boolean; title: string; message: string; onConfirm: () => void }>({ open: false, title: '', message: '', onConfirm: () => {} });
  const [nodeSearchQuery, setNodeSearchQuery] = useState('');
  const [nodeTypeFilter, setNodeTypeFilter] = useState<'all' | 'shared' | 'dedicated'>('all');
  const [nodeStatusFilter, setNodeStatusFilter] = useState<'all' | 'online' | 'offline'>('all');
  const [clientSearchQuery, setClientSearchQuery] = useState('');
  const [clientStatusFilter, setClientStatusFilter] = useState<'all' | 'online' | 'offline'>('all');
  const [formData, setFormData] = useState({
    client_id: '',
    node_id: '',
    name: '',
    type: 'tcp',
    localIP: '127.0.0.1',
    localPort: '',
    remotePort: '',
    enabled: true,
  });
  const [userPortInfo, setUserPortInfo] = useState<{
    maxPortCount: number | null;
    currentPortCount: number;
    allowedPortRange: string | null;
  } | null>(null);
  const [parsedPorts, setParsedPorts] = useState<number[]>([]);
  const [portParseError, setPortParseError] = useState<string>('');
  const [expandedGroups, setExpandedGroups] = useState<Set<string>>(new Set());
  const [editingGroupId, setEditingGroupId] = useState<string | null>(null);
  const [editingGroupPorts, setEditingGroupPorts] = useState<{ id: number; remotePort: string; localPort: string }[]>([]);
  useEffect(() => {
    loadData();
  }, []);

  // 控制模态框打开时禁用背景滚动
  useEffect(() => {
    if (showCreateModal) {
      document.body.style.overflow = 'hidden';
    } else {
      document.body.style.overflow = 'unset';
    }

    // 清理函数
    return () => {
      document.body.style.overflow = 'unset';
    };
  }, [showCreateModal]);

  const loadData = async () => {
    try {
      setLoading(true);
      const [proxiesRes, clientsRes, nodesRes] = await Promise.all([
        proxyService.getProxies(),
        clientService.getClients(),
        nodeService.getNodes(),
      ]);
      if (proxiesRes.success && proxiesRes.data) setProxies(proxiesRes.data);
      if (clientsRes.success && clientsRes.data) setClients(clientsRes.data);
      if (nodesRes.success && nodesRes.data) {
        setNodes(nodesRes.data);

        // 获取当前用户信息
        const authUser = getStoredJson<Partial<User>>('user', {});

        if (authUser.is_admin) {
          // 管理员可以看到所有节点
          setAvailableNodes(nodesRes.data);
        } else if (authUser.id) {
          // 普通用户获取自己的节点列表
          const userNodesRes = await userService.getUserNodes(authUser.id);
          if (userNodesRes.success && userNodesRes.data) {
            // 过滤出共享节点 + 用户的独享节点
            const available = nodesRes.data.filter((node: Node) =>
              node.nodeType === 'shared' || userNodesRes.data!.some((un: Node) => un.id === node.id)
            );
            setAvailableNodes(available);
          } else {
            // 如果获取失败，只显示共享节点
            setAvailableNodes(nodesRes.data.filter((node: Node) => node.nodeType === 'shared'));
          }
        } else {
          // 未登录，只显示共享节点
          setAvailableNodes(nodesRes.data.filter((node: Node) => node.nodeType === 'shared'));
        }
      }

      // 加载用户端口配额信息
      await loadUserPortInfo();
    } catch (error) {
      console.error('加载数据失败:', error);
      showToast('加载失败', 'error');
    } finally {
      setLoading(false);
    }
  };

  const resetForm = () => {
    setFormData({
      client_id: '',
      node_id: '',
      name: '',
      type: 'tcp',
      localIP: '127.0.0.1',
      localPort: '',
      remotePort: '',
      enabled: true,
    });
    setEditingProxy(null);
    setEditingGroupId(null);
  };

  const handleCreateProxy = async () => {
    if (!formData.name || !formData.client_id || !formData.node_id || !formData.localPort || !formData.remotePort) {
      showToast('请填写所有必填字段', 'error');
      return;
    }

    // 解析远程端口
    const { ports, error } = parsePortString(formData.remotePort);
    if (error) {
      showToast(error, 'error');
      return;
    }

    if (ports.length === 0) {
      showToast('请输入有效的端口', 'error');
      return;
    }

    // 解析本地端口
    const { ports: localPorts, error: localError } = parsePortString(formData.localPort);
    if (localError) {
      showToast(`本地端口: ${localError}`, 'error');
      return;
    }

    if (localPorts.length === 0) {
      showToast('请输入有效的本地端口', 'error');
      return;
    }

    // 验证本地端口数量：要么是1个（所有代理共用），要么与远程端口数量一致（一一对应）
    if (localPorts.length !== 1 && localPorts.length !== ports.length) {
      showToast(
        `本地端口数量（${localPorts.length}）与节点端口数量（${ports.length}）不匹配，请填写单个端口或与节点端口数量一致的范围`,
        'error'
      );
      return;
    }

    // 验证端口配额（仅对非管理员）
    const authUser = getStoredJson<Partial<User>>('user', {});
    if (!authUser.is_admin && userPortInfo) {
      if (userPortInfo.maxPortCount !== null) {
        const availableCount = userPortInfo.maxPortCount - userPortInfo.currentPortCount;
        if (ports.length > availableCount) {
          showToast(
            `端口配额不足：需要 ${ports.length} 个端口，可用 ${availableCount} 个（总配额 ${userPortInfo.maxPortCount}）`,
            'error'
          );
          return;
        }
      }
    }

    try {
      const response = await proxyService.batchCreateProxies({
        client_id: formData.client_id,
        name: formData.name,
        type: formData.type,
        localIP: formData.localIP,
        localPorts: localPorts,
        remotePorts: ports,
        nodeId: parseInt(formData.node_id),
      });

      if (response.success) {
        showToast(`成功创建 ${ports.length} 个代理`, 'success');
        resetForm();
        setShowCreateModal(false);
        loadData();
      } else {
        showToast(response.message || '创建失败', 'error');
      }
    } catch (error) {
      console.error('创建代理失败:', error);
      showToast('创建失败', 'error');
    }
  };

  const handleUpdateProxy = async () => {
    if (!editingProxy) return;

    try {
      const response = await proxyService.updateProxy(editingProxy.id, {
        name: formData.name || undefined,
        type: formData.type || undefined,
        localIP: formData.localIP || undefined,
        localPort: formData.localPort ? parseInt(formData.localPort) : undefined,
        remotePort: formData.remotePort ? parseInt(formData.remotePort) : undefined,
        enabled: formData.enabled,
      });
      if (response.success) {
        showToast('代理更新成功', 'success');
        resetForm();
        setEditingProxy(null);
        setShowCreateModal(false);
        loadData();
      } else {
        showToast(response.message || '更新失败', 'error');
      }
    } catch (error) {
      console.error('更新代理失败:', error);
      showToast('更新失败', 'error');
    }
  };

  const handleEdit = (proxy: Proxy) => {
    setEditingProxy(proxy);
    setFormData({
      client_id: proxy.client_id,
      node_id: proxy.nodeId ? proxy.nodeId.toString() : '',
      name: proxy.name,
      type: proxy.type,
      localIP: proxy.localIP,
      localPort: proxy.localPort.toString(),
      remotePort: proxy.remotePort.toString(),
      enabled: proxy.enabled,
    });
    setShowCreateModal(true);
  };

  const handleDelete = (id: number) => {
    setConfirmDialog({
      open: true,
      title: '删除代理',
      message: '确定要删除这个代理吗？',
      onConfirm: async () => {
        try {
          const response = await proxyService.deleteProxy(id);
          if (response.success) {
            showToast('代理删除成功', 'success');
            loadData();
          } else {
            showToast(response.message || '删除失败', 'error');
          }
        } catch (error) {
          console.error('删除代理失败:', error);
          showToast('删除失败', 'error');
        }
      },
    });
  };

  const handleToggleEnabled = async (proxy: Proxy) => {
    try {
      const response = await proxyService.updateProxy(proxy.id, {
        enabled: !proxy.enabled,
      });
      if (response.success) {
        showToast(`代理已${proxy.enabled ? '禁用' : '启用'}`, 'success');
        loadData();
      }
    } catch (error) {
      console.error('切换状态失败:', error);
      showToast('操作失败', 'error');
    }
  };

  const getClientName = (clientId: string) => {
    const client = clients.find((c) => c.id.toString() === clientId);
    return client?.name || clientId;
  };

  const getNodeName = (nodeId: number | null) => {
    if (!nodeId) return '-';
    const node = nodes.find((n) => n.id === nodeId);
    return node?.name || String(nodeId);
  };

  const getNodeIp = (nodeId: number | null) => {
    if (!nodeId) return null;
    const node = nodes.find((n) => n.id === nodeId);
    return node?.tunnelAddr || node?.publicIp || null;
  };

  // 解析端口字符串，支持范围和逗号分隔
  // 格式: "8000-8010" 或 "8000,8001,8002" 或 "8000-8002,8005,8010-8012"
  const parsePortString = (portStr: string): { ports: number[]; error: string } => {
    if (!portStr.trim()) {
      return { ports: [], error: '' };
    }

    const ports: number[] = [];
    const parts = portStr.split(',');

    try {
      for (const part of parts) {
        const trimmed = part.trim();
        if (!trimmed) continue;

        if (trimmed.includes('-')) {
          // 范围格式: "8000-8010"
          const rangeParts = trimmed.split('-');
          if (rangeParts.length !== 2) {
            return { ports: [], error: `无效的端口范围格式: ${trimmed}` };
          }

          const start = parseInt(rangeParts[0].trim());
          const end = parseInt(rangeParts[1].trim());

          if (isNaN(start) || isNaN(end)) {
            return { ports: [], error: `无效的端口号: ${trimmed}` };
          }

          if (start < 1 || start > 65535 || end < 1 || end > 65535) {
            return { ports: [], error: `端口号必须在 1-65535 之间: ${trimmed}` };
          }

          if (start > end) {
            return { ports: [], error: `起始端口不能大于结束端口: ${trimmed}` };
          }

          // 限制范围大小，避免生成过多端口
          if (end - start > 1000) {
            return { ports: [], error: `端口范围过大（最多1000个）: ${trimmed}` };
          }

          for (let port = start; port <= end; port++) {
            ports.push(port);
          }
        } else {
          // 单个端口: "8000"
          const port = parseInt(trimmed);
          if (isNaN(port)) {
            return { ports: [], error: `无效的端口号: ${trimmed}` };
          }

          if (port < 1 || port > 65535) {
            return { ports: [], error: `端口号必须在 1-65535 之间: ${trimmed}` };
          }

          ports.push(port);
        }
      }

      // 去重
      const uniquePorts = Array.from(new Set(ports)).sort((a, b) => a - b);
      return { ports: uniquePorts, error: '' };
    } catch (e) {
      return { ports: [], error: '端口解析失败' };
    }
  };

  // 获取用户端口配额信息
  const loadUserPortInfo = async () => {
    try {
      const authUser = getStoredJson<Partial<User>>('user', {});
      if (authUser.is_admin) {
        // 管理员没有端口限制
        setUserPortInfo(null);
        return;
      }

      if (authUser.id) {
        const response = await userService.getUsers();
        if (response.success && response.data) {
          const currentUser = response.data.find((u: any) => u.id === authUser.id);
          if (currentUser) {
            setUserPortInfo({
              maxPortCount: currentUser.maxPortCount,
              currentPortCount: currentUser.currentPortCount || 0,
              allowedPortRange: currentUser.allowedPortRange,
            });
          }
        }
      }
    } catch (error) {
      console.error('加载端口配额信息失败:', error);
    }
  };

  // 监听远程端口输入变化，实时解析
  useEffect(() => {
    if (!formData.remotePort) {
      setParsedPorts([]);
      setPortParseError('');
      return;
    }

    const { ports, error } = parsePortString(formData.remotePort);
    setParsedPorts(ports);
    setPortParseError(error);
  }, [formData.remotePort]);

  // 筛选节点
  const getFilteredNodes = () => {
    return availableNodes.filter((node) => {
      // 搜索过滤
      if (nodeSearchQuery) {
        const query = nodeSearchQuery.toLowerCase();
        const matchName = node.name.toLowerCase().includes(query);
        const matchRegion = node.region?.toLowerCase().includes(query);
        if (!matchName && !matchRegion) return false;
      }

      // 类型过滤
      if (nodeTypeFilter !== 'all') {
        if (nodeTypeFilter === 'shared' && node.nodeType !== 'shared') return false;
        if (nodeTypeFilter === 'dedicated' && node.nodeType !== 'dedicated') return false;
      }

      // 状态过滤
      if (nodeStatusFilter !== 'all') {
        if (nodeStatusFilter === 'online' && !node.isOnline) return false;
        if (nodeStatusFilter === 'offline' && node.isOnline) return false;
      }

      return true;
    }).sort((a, b) => {
      // 在线节点排在前面
      if (a.isOnline && !b.isOnline) return -1;
      if (!a.isOnline && b.isOnline) return 1;
      return 0;
    });
  };

  // 筛选客户端
  const getFilteredClients = () => {
    return clients.filter((client) => {
      // 搜索过滤
      if (clientSearchQuery) {
        const query = clientSearchQuery.toLowerCase();
        const matchName = client.name.toLowerCase().includes(query);
        const matchId = client.id.toString().includes(query);
        if (!matchName && !matchId) return false;
      }

      // 状态过滤
      if (clientStatusFilter !== 'all') {
        if (clientStatusFilter === 'online' && !client.is_online) return false;
        if (clientStatusFilter === 'offline' && client.is_online) return false;
      }

      return true;
    });
  };

  // 将代理列表按 group_id 分组为显示行
  const getDisplayRows = (): ProxyDisplayRow[] => {
    const groups = new Map<string, Proxy[]>();
    const standalone: Proxy[] = [];

    for (const proxy of proxies) {
      if (proxy.groupId) {
        if (!groups.has(proxy.groupId)) {
          groups.set(proxy.groupId, []);
        }
        groups.get(proxy.groupId)!.push(proxy);
      } else {
        standalone.push(proxy);
      }
    }

    const rows: ProxyDisplayRow[] = [];

    for (const [groupId, groupProxies] of groups) {
      if (groupProxies.length === 1) {
        rows.push({ kind: 'standalone', proxy: groupProxies[0] });
      } else {
        const sorted = groupProxies.sort((a, b) => a.remotePort - b.remotePort);
        const first = sorted[0];
        const baseName = first.name.replace(/-\d+$/, '');
        rows.push({
          kind: 'group',
          group: {
            groupId,
            name: baseName,
            proxies: sorted,
            client_id: first.client_id,
            nodeId: first.nodeId,
            type: first.type,
            localIP: first.localIP,
            enabled: sorted.every(p => p.enabled),
            totalBytesSent: sorted.reduce((sum, p) => sum + p.totalBytesSent, 0),
            totalBytesReceived: sorted.reduce((sum, p) => sum + p.totalBytesReceived, 0),
          },
        });
      }
    }

    for (const proxy of standalone) {
      rows.push({ kind: 'standalone', proxy });
    }

    return rows;
  };

  const toggleGroupExpand = (groupId: string) => {
    setExpandedGroups(prev => {
      const next = new Set(prev);
      if (next.has(groupId)) next.delete(groupId);
      else next.add(groupId);
      return next;
    });
  };

  const handleDeleteGroup = (groupId: string, proxyCount: number) => {
    setConfirmDialog({
      open: true,
      title: '删除代理组',
      message: `确定要删除这个代理组（共 ${proxyCount} 个代理）吗？`,
      onConfirm: async () => {
        try {
          const response = await proxyService.deleteProxyGroup(groupId);
          if (response.success) {
            showToast('代理组删除成功', 'success');
            loadData();
          } else {
            showToast(response.message || '删除失败', 'error');
          }
        } catch (error) {
          showToast('删除失败', 'error');
        }
      },
    });
  };

  const handleToggleGroupEnabled = async (groupId: string, currentlyEnabled: boolean) => {
    try {
      const response = await proxyService.toggleProxyGroup(groupId, !currentlyEnabled);
      if (response.success) {
        showToast(`代理组已${currentlyEnabled ? '禁用' : '启用'}`, 'success');
        loadData();
      }
    } catch (error) {
      showToast('操作失败', 'error');
    }
  };

  const handleEditGroup = (group: ProxyGroup) => {
    setEditingGroupId(group.groupId);
    setEditingProxy(null);
    setFormData({
      client_id: group.client_id,
      node_id: group.nodeId ? group.nodeId.toString() : '',
      name: group.name,
      type: group.type,
      localIP: group.localIP,
      localPort: '',
      remotePort: '',
      enabled: group.enabled,
    });
    setEditingGroupPorts(
      group.proxies
        .sort((a, b) => a.remotePort - b.remotePort)
        .map(p => ({ id: p.id, remotePort: p.remotePort.toString(), localPort: p.localPort.toString() }))
    );
    setShowCreateModal(true);
  };

  const handleUpdateGroup = async () => {
    if (!editingGroupId) return;
    try {
      // 更新组共享属性
      const response = await proxyService.updateProxyGroup(editingGroupId, {
        name: formData.name || undefined,
        type: formData.type || undefined,
        localIP: formData.localIP || undefined,
      });
      if (!response.success) {
        showToast(response.message || '更新失败', 'error');
        return;
      }

      // 逐个更新每个子代理的端口
      for (const item of editingGroupPorts) {
        const remotePort = parseInt(item.remotePort);
        const localPort = parseInt(item.localPort);
        if (isNaN(remotePort) || isNaN(localPort)) continue;
        await proxyService.updateProxy(item.id, { remotePort, localPort });
      }

      showToast('代理组更新成功', 'success');
      resetForm();
      setEditingGroupId(null);
      setEditingGroupPorts([]);
      setShowCreateModal(false);
      loadData();
    } catch (error) {
      showToast('更新失败', 'error');
    }
  };

  // 格式化端口范围显示
  const formatPortRange = (proxies: Proxy[]): string => {
    const ports = proxies.map(p => p.remotePort).sort((a, b) => a - b);
    if (ports.length <= 3) {
      return ports.map(p => `:${p}`).join(', ');
    }
    return `:${ports[0]}-${ports[ports.length - 1]}`;
  };

  const formatLocalPortRange = (proxies: Proxy[]): string => {
    const localPorts = [...new Set(proxies.map(p => p.localPort))].sort((a, b) => a - b);
    if (localPorts.length === 1) {
      return `${proxies[0].localIP}:${localPorts[0]}`;
    }
    if (localPorts.length <= 3) {
      return localPorts.map(p => `${proxies[0].localIP}:${p}`).join(', ');
    }
    return `${proxies[0].localIP}:${localPorts[0]}-${localPorts[localPorts.length - 1]}`;
  };

  return (
    <div className="space-y-6">
      {/* 页面标题 */}
      <div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
        <div>
          <h2 className="text-2xl font-bold text-foreground">代理管理</h2>
          <p className="mt-1 text-sm text-muted-foreground">管理所有代理映射规则</p>
        </div>
        <button
          onClick={() => {
            resetForm();
            setShowCreateModal(true);
          }}
          className="inline-flex items-center gap-2 px-5 py-2.5 text-primary-foreground text-sm font-medium rounded-xl focus:outline-none focus:ring-2 focus:ring-primary/40 shadow-sm transition-all duration-200 hover:opacity-90"
          style={{ background: 'linear-gradient(135deg, hsl(210 100% 45%), hsl(189 94% 43%))' }}
        >
          <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" strokeWidth={2} stroke="currentColor" className="w-4 h-4">
            <path strokeLinecap="round" strokeLinejoin="round" d="M12 4.5v15m7.5-7.5h-15" />
          </svg>
          新建代理
        </button>
      </div>

      {loading ? (
        <TableSkeleton rows={5} cols={7} />
      ) : (
        <TableContainer>
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>名称</TableHead>
                <TableHead>客户端</TableHead>
                <TableHead>节点</TableHead>
                <TableHead>类型</TableHead>
                <TableHead>端口映射</TableHead>
                <TableHead>状态</TableHead>
                <TableHead>流量</TableHead>
                <TableHead className="text-right">操作</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {proxies.length === 0 ? (
                <TableRow>
                  <TableCell colSpan={8} className="px-6 py-16 text-center">
                    <div className="flex flex-col items-center gap-3">
                      <div className="w-16 h-16 bg-muted rounded-full flex items-center justify-center">
                        <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" strokeWidth={1.5} stroke="currentColor" className="w-8 h-8 text-muted-foreground">
                          <path strokeLinecap="round" strokeLinejoin="round" d="M7.5 21L3 16.5m0 0L7.5 12M3 16.5h13.5m0-13.5L21 7.5m0 0L16.5 12M21 7.5H7.5" />
                        </svg>
                      </div>
                      <p className="text-muted-foreground">暂无代理数据</p>
                      <button
                        onClick={() => {
                          resetForm();
                          setShowCreateModal(true);
                        }}
                        className="text-sm text-primary hover:text-primary/80 font-medium"
                      >
                        创建第一个代理
                      </button>
                    </div>
                  </TableCell>
                </TableRow>
              ) : (
                getDisplayRows().map((row) => {
                  if (row.kind === 'standalone') {
                    const proxy = row.proxy;
                    return (
                      <TableRow key={proxy.id}>
                        <TableCell className="whitespace-nowrap">
                          <div className="flex items-center gap-3">
                            <div className="w-9 h-9 bg-gradient-to-br from-purple-500 to-pink-600 rounded-lg flex items-center justify-center text-primary-foreground text-sm font-semibold shadow-sm">
                              {proxy.name.charAt(0).toUpperCase()}
                            </div>
                            <span className="text-sm font-semibold text-foreground">{proxy.name}</span>
                          </div>
                        </TableCell>
                        <TableCell className="whitespace-nowrap">
                          <span className="text-sm text-muted-foreground">{getClientName(proxy.client_id)}</span>
                        </TableCell>
                        <TableCell className="whitespace-nowrap">
                          <div className="flex flex-col">
                            <span className="text-sm text-muted-foreground">{getNodeName(proxy.nodeId)}</span>
                            {getNodeIp(proxy.nodeId) && (
                              <span className="text-xs text-muted-foreground/70 font-mono">{getNodeIp(proxy.nodeId)}</span>
                            )}
                          </div>
                        </TableCell>
                        <TableCell className="whitespace-nowrap">
                          <span className="inline-flex items-center px-2.5 py-1 text-xs font-semibold rounded-lg" style={{
                            background: (proxy.type || 'tcp').toLowerCase() === 'tcp' ? 'hsl(217 91% 60% / 0.15)' : 'hsl(263 70% 58% / 0.15)',
                            color: (proxy.type || 'tcp').toLowerCase() === 'tcp' ? 'hsl(217 91% 60%)' : 'hsl(263 70% 58%)'
                          }}>
                            {(proxy.type || 'tcp').toUpperCase()}
                          </span>
                        </TableCell>
                        <TableCell className="whitespace-nowrap">
                          <div className="flex items-center gap-2 text-sm">
                            <span className="px-2 py-1 bg-muted text-primary rounded-lg font-mono text-xs">
                              {getNodeIp(proxy.nodeId) ? `${getNodeIp(proxy.nodeId)}:${proxy.remotePort}` : `:${proxy.remotePort}`}
                            </span>
                            <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" strokeWidth={2} stroke="currentColor" className="w-4 h-4 text-muted-foreground">
                              <path strokeLinecap="round" strokeLinejoin="round" d="M13.5 4.5L21 12m0 0l-7.5 7.5M21 12H3" />
                            </svg>
                            <span className="text-muted-foreground font-mono text-xs">
                              {proxy.localIP}:{proxy.localPort}
                            </span>
                          </div>
                        </TableCell>
                        <TableCell className="whitespace-nowrap">
                          <span className={`inline-flex items-center gap-1.5 px-2.5 py-1 text-xs font-semibold rounded-lg`}
                            style={proxy.enabled
                              ? { background: 'hsl(142 71% 45% / 0.15)', color: 'hsl(142 71% 45%)' }
                              : { background: 'hsl(0 0% 50% / 0.1)', color: 'hsl(0 0% 45%)' }
                            }
                          >
                            <span className="w-1.5 h-1.5 rounded-full" style={{ background: proxy.enabled ? 'hsl(142 71% 45%)' : 'hsl(0 0% 60%)' }}></span>
                            {proxy.enabled ? '启用' : '禁用'}
                          </span>
                        </TableCell>
                        <TableCell className="whitespace-nowrap">
                          <div className="flex flex-col gap-1">
                            <div className="flex items-center gap-1.5 text-xs">
                              <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" strokeWidth={2} stroke="currentColor" className="w-3.5 h-3.5" style={{ color: 'hsl(217 91% 60%)' }}>
                                <path strokeLinecap="round" strokeLinejoin="round" d="M4.5 10.5L12 3m0 0l7.5 7.5M12 3v18" />
                              </svg>
                              <span className="text-muted-foreground">{formatBytes(proxy.totalBytesSent)}</span>
                            </div>
                            <div className="flex items-center gap-1.5 text-xs">
                              <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" strokeWidth={2} stroke="currentColor" className="w-3.5 h-3.5" style={{ color: 'hsl(142 71% 45%)' }}>
                                <path strokeLinecap="round" strokeLinejoin="round" d="M19.5 13.5L12 21m0 0l-7.5-7.5M12 21V3" />
                              </svg>
                              <span className="text-muted-foreground">{formatBytes(proxy.totalBytesReceived)}</span>
                            </div>
                          </div>
                        </TableCell>
                        <TableCell className="whitespace-nowrap text-right">
                          <div className="flex flex-wrap items-center justify-end gap-1.5">
                            <button
                              onClick={() => handleToggleEnabled(proxy)}
                              className={`inline-flex items-center gap-1.5 px-3 py-1.5 text-xs font-medium rounded-lg transition-colors ${
                                proxy.enabled
                                  ? 'text-amber-600 hover:bg-amber-50'
                                  : 'text-green-600 hover:bg-green-50'
                              }`}
                            >
                              {proxy.enabled ? '禁用' : '启用'}
                            </button>
                            <button
                              onClick={() => handleEdit(proxy)}
                              className="inline-flex items-center gap-1.5 px-3 py-1.5 text-xs font-medium text-primary hover:bg-accent rounded-lg transition-colors"
                            >
                              编辑
                            </button>
                            <button
                              onClick={() => handleDelete(proxy.id)}
                              className="inline-flex items-center gap-1.5 px-3 py-1.5 text-xs font-medium text-red-600 hover:bg-red-50 rounded-lg transition-colors"
                            >
                              删除
                            </button>
                          </div>
                        </TableCell>
                      </TableRow>
                    );
                  }

                  // Group row
                  const group = row.group;
                  const isExpanded = expandedGroups.has(group.groupId);
                  return (
                    <Fragment key={`group-${group.groupId}`}>
                      <TableRow className="cursor-pointer hover:bg-accent/50" onClick={() => toggleGroupExpand(group.groupId)}>
                        <TableCell className="whitespace-nowrap">
                          <div className="flex items-center gap-3">
                            <button
                              onClick={(e) => { e.stopPropagation(); toggleGroupExpand(group.groupId); }}
                              className="text-muted-foreground hover:text-foreground transition-colors"
                            >
                              <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" strokeWidth={2} stroke="currentColor"
                                className={`w-4 h-4 transition-transform duration-200 ${isExpanded ? 'rotate-90' : ''}`}
                              >
                                <path strokeLinecap="round" strokeLinejoin="round" d="M8.25 4.5l7.5 7.5-7.5 7.5" />
                              </svg>
                            </button>
                            <div className="w-9 h-9 bg-gradient-to-br from-indigo-500 to-blue-600 rounded-lg flex items-center justify-center text-primary-foreground text-sm font-semibold shadow-sm">
                              {group.name.charAt(0).toUpperCase()}
                            </div>
                            <div>
                              <span className="text-sm font-semibold text-foreground">{group.name}</span>
                              <span className="ml-2 text-xs text-muted-foreground">({group.proxies.length} 个端口)</span>
                            </div>
                          </div>
                        </TableCell>
                        <TableCell className="whitespace-nowrap">
                          <span className="text-sm text-muted-foreground">{getClientName(group.client_id)}</span>
                        </TableCell>
                        <TableCell className="whitespace-nowrap">
                          <div className="flex flex-col">
                            <span className="text-sm text-muted-foreground">{getNodeName(group.nodeId)}</span>
                            {getNodeIp(group.nodeId) && (
                              <span className="text-xs text-muted-foreground/70 font-mono">{getNodeIp(group.nodeId)}</span>
                            )}
                          </div>
                        </TableCell>
                        <TableCell className="whitespace-nowrap">
                          <span className="inline-flex items-center px-2.5 py-1 text-xs font-semibold rounded-lg" style={{
                            background: (group.type || 'tcp').toLowerCase() === 'tcp' ? 'hsl(217 91% 60% / 0.15)' : 'hsl(263 70% 58% / 0.15)',
                            color: (group.type || 'tcp').toLowerCase() === 'tcp' ? 'hsl(217 91% 60%)' : 'hsl(263 70% 58%)'
                          }}>
                            {(group.type || 'tcp').toUpperCase()}
                          </span>
                        </TableCell>
                        <TableCell className="whitespace-nowrap">
                          <div className="flex items-center gap-2 text-sm">
                            <span className="px-2 py-1 bg-muted text-primary rounded-lg font-mono text-xs">
                              {getNodeIp(group.nodeId) ? `${getNodeIp(group.nodeId)}:[${formatPortRange(group.proxies)}]` : formatPortRange(group.proxies)}
                            </span>
                            <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" strokeWidth={2} stroke="currentColor" className="w-4 h-4 text-muted-foreground">
                              <path strokeLinecap="round" strokeLinejoin="round" d="M13.5 4.5L21 12m0 0l-7.5 7.5M21 12H3" />
                            </svg>
                            <span className="text-muted-foreground font-mono text-xs">
                              {formatLocalPortRange(group.proxies)}
                            </span>
                          </div>
                        </TableCell>
                        <TableCell className="whitespace-nowrap">
                          <span className={`inline-flex items-center gap-1.5 px-2.5 py-1 text-xs font-semibold rounded-lg`}
                            style={group.enabled
                              ? { background: 'hsl(142 71% 45% / 0.15)', color: 'hsl(142 71% 45%)' }
                              : { background: 'hsl(0 0% 50% / 0.1)', color: 'hsl(0 0% 45%)' }
                            }
                          >
                            <span className="w-1.5 h-1.5 rounded-full" style={{ background: group.enabled ? 'hsl(142 71% 45%)' : 'hsl(0 0% 60%)' }}></span>
                            {group.enabled ? '启用' : '禁用'}
                          </span>
                        </TableCell>
                        <TableCell className="whitespace-nowrap">
                          <div className="flex flex-col gap-1">
                            <div className="flex items-center gap-1.5 text-xs">
                              <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" strokeWidth={2} stroke="currentColor" className="w-3.5 h-3.5" style={{ color: 'hsl(217 91% 60%)' }}>
                                <path strokeLinecap="round" strokeLinejoin="round" d="M4.5 10.5L12 3m0 0l7.5 7.5M12 3v18" />
                              </svg>
                              <span className="text-muted-foreground">{formatBytes(group.totalBytesSent)}</span>
                            </div>
                            <div className="flex items-center gap-1.5 text-xs">
                              <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" strokeWidth={2} stroke="currentColor" className="w-3.5 h-3.5" style={{ color: 'hsl(142 71% 45%)' }}>
                                <path strokeLinecap="round" strokeLinejoin="round" d="M19.5 13.5L12 21m0 0l-7.5-7.5M12 21V3" />
                              </svg>
                              <span className="text-muted-foreground">{formatBytes(group.totalBytesReceived)}</span>
                            </div>
                          </div>
                        </TableCell>
                        <TableCell className="whitespace-nowrap text-right">
                          <div className="flex flex-wrap items-center justify-end gap-1.5" onClick={(e) => e.stopPropagation()}>
                            <button
                              onClick={() => handleToggleGroupEnabled(group.groupId, group.enabled)}
                              className={`inline-flex items-center gap-1.5 px-3 py-1.5 text-xs font-medium rounded-lg transition-colors ${
                                group.enabled
                                  ? 'text-amber-600 hover:bg-amber-50'
                                  : 'text-green-600 hover:bg-green-50'
                              }`}
                            >
                              {group.enabled ? '禁用' : '启用'}
                            </button>
                            <button
                              onClick={() => handleEditGroup(group)}
                              className="inline-flex items-center gap-1.5 px-3 py-1.5 text-xs font-medium text-primary hover:bg-accent rounded-lg transition-colors"
                            >
                              编辑
                            </button>
                            <button
                              onClick={() => handleDeleteGroup(group.groupId, group.proxies.length)}
                              className="inline-flex items-center gap-1.5 px-3 py-1.5 text-xs font-medium text-red-600 hover:bg-red-50 rounded-lg transition-colors"
                            >
                              删除
                            </button>
                          </div>
                        </TableCell>
                      </TableRow>
                      {/* Expanded sub-rows */}
                      {isExpanded && group.proxies.map((proxy) => (
                        <TableRow key={proxy.id} className="bg-muted/30">
                          <TableCell className="whitespace-nowrap pl-16">
                            <span className="text-xs text-muted-foreground">{proxy.name}</span>
                          </TableCell>
                          <TableCell></TableCell>
                          <TableCell></TableCell>
                          <TableCell></TableCell>
                          <TableCell className="whitespace-nowrap">
                            <div className="flex items-center gap-2 text-sm">
                              <span className="px-2 py-1 bg-muted text-primary rounded-lg font-mono text-xs">
                                :{proxy.remotePort}
                              </span>
                              <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" strokeWidth={2} stroke="currentColor" className="w-4 h-4 text-muted-foreground">
                                <path strokeLinecap="round" strokeLinejoin="round" d="M13.5 4.5L21 12m0 0l-7.5 7.5M21 12H3" />
                              </svg>
                              <span className="text-muted-foreground font-mono text-xs">
                                {proxy.localIP}:{proxy.localPort}
                              </span>
                            </div>
                          </TableCell>
                          <TableCell className="whitespace-nowrap">
                            <span className={`inline-flex items-center gap-1.5 px-2 py-0.5 text-xs rounded-lg`}
                              style={proxy.enabled
                                ? { background: 'hsl(142 71% 45% / 0.1)', color: 'hsl(142 71% 45%)' }
                                : { background: 'hsl(0 0% 50% / 0.08)', color: 'hsl(0 0% 45%)' }
                              }
                            >
                              <span className="w-1 h-1 rounded-full" style={{ background: proxy.enabled ? 'hsl(142 71% 45%)' : 'hsl(0 0% 60%)' }}></span>
                              {proxy.enabled ? '启用' : '禁用'}
                            </span>
                          </TableCell>
                          <TableCell className="whitespace-nowrap">
                            <div className="flex flex-col gap-1">
                              <div className="flex items-center gap-1.5 text-xs">
                                <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" strokeWidth={2} stroke="currentColor" className="w-3 h-3" style={{ color: 'hsl(217 91% 60%)' }}>
                                  <path strokeLinecap="round" strokeLinejoin="round" d="M4.5 10.5L12 3m0 0l7.5 7.5M12 3v18" />
                                </svg>
                                <span className="text-muted-foreground">{formatBytes(proxy.totalBytesSent)}</span>
                              </div>
                              <div className="flex items-center gap-1.5 text-xs">
                                <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" strokeWidth={2} stroke="currentColor" className="w-3 h-3" style={{ color: 'hsl(142 71% 45%)' }}>
                                  <path strokeLinecap="round" strokeLinejoin="round" d="M19.5 13.5L12 21m0 0l-7.5-7.5M12 21V3" />
                                </svg>
                                <span className="text-muted-foreground">{formatBytes(proxy.totalBytesReceived)}</span>
                              </div>
                            </div>
                          </TableCell>
                          <TableCell className="whitespace-nowrap text-right">
                            <button
                              onClick={() => handleEdit(proxy)}
                              className="inline-flex items-center gap-1.5 px-2.5 py-1 text-xs font-medium text-primary hover:bg-accent rounded-lg transition-colors"
                            >
                              编辑
                            </button>
                            <button
                              onClick={() => handleDelete(proxy.id)}
                              className="inline-flex items-center gap-1.5 px-2.5 py-1 text-xs font-medium text-red-600 hover:bg-red-50 rounded-lg transition-colors"
                            >
                              删除
                            </button>
                          </TableCell>
                        </TableRow>
                      ))}
                    </Fragment>
                  );
                })
              )}
            </TableBody>
          </Table>
        </TableContainer>
      )}

      {/* 创建/编辑代理模态框 */}
      {showCreateModal && (
        <div className="fixed inset-0 bg-black/50 backdrop-blur-sm overflow-y-auto h-full w-full flex items-start justify-center z-50 p-4">
          <div className="relative bg-card rounded-2xl shadow-2xl w-full max-w-2xl my-8 transform transition-all flex flex-col max-h-[calc(100vh-4rem)]">
            {/* 固定头部 */}
            <div className="flex-shrink-0 px-6 pt-5 pb-4 border-b border-border">
              <div className="flex items-center justify-between">
                <div className="flex items-center gap-3">
                  <div className="w-10 h-10 rounded-xl flex items-center justify-center" style={{ background: 'linear-gradient(135deg, hsl(210 100% 45%), hsl(189 94% 43%))' }}>
                    <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" strokeWidth={1.5} stroke="currentColor" className="w-5 h-5 text-primary-foreground">
                      <path strokeLinecap="round" strokeLinejoin="round" d="M7.5 21L3 16.5m0 0L7.5 12M3 16.5h13.5m0-13.5L21 7.5m0 0L16.5 12M21 7.5H7.5" />
                    </svg>
                  </div>
                  <div>
                    <h3 className="text-lg font-bold text-foreground">
                      {editingGroupId ? '编辑代理组' : editingProxy ? '编辑代理' : '创建新代理'}
                    </h3>
                    <p className="text-xs text-muted-foreground">
                      {editingGroupId ? '修改代理组共享配置' : editingProxy ? '修改代理配置信息' : '添加一个新的端口映射规则'}
                    </p>
                  </div>
                </div>
                <button
                  onClick={() => {
                    setShowCreateModal(false);
                    setEditingProxy(null);
                    resetForm();
                  }}
                  className="text-muted-foreground hover:text-muted-foreground transition-colors"
                >
                  <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" strokeWidth={2} stroke="currentColor" className="w-5 h-5">
                    <path strokeLinecap="round" strokeLinejoin="round" d="M6 18L18 6M6 6l12 12" />
                  </svg>
                </button>
              </div>
            </div>

            {/* 可滚动内容区域 */}
            <div className="flex-1 overflow-y-auto px-6 py-4">
              <div className="space-y-4">
                {/* 客户端选择 */}
                <div>
                  <label className="block text-sm font-semibold text-foreground mb-3">选择客户端 *</label>
                  {(editingProxy || editingGroupId) ? (
                    <div className="px-4 py-3 bg-muted rounded-xl text-muted-foreground text-sm">
                      {getClientName(formData.client_id)} (编辑时不可更改)
                    </div>
                  ) : (
                    <>
                      {/* 搜索和筛选 */}
                      {clients.length > 3 && (
                        <div className="mb-3 space-y-2">
                          {/* 搜索框 */}
                          <div className="relative">
                            <input
                              type="text"
                              value={clientSearchQuery}
                              onChange={(e) => setClientSearchQuery(e.target.value)}
                              placeholder="搜索客户端名称或ID..."
                              className="w-full pl-10 pr-4 py-2 border border-border rounded-lg focus:ring-2 focus:ring-primary focus:border-transparent text-sm"
                            />
                            <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" strokeWidth={2} stroke="currentColor" className="w-4 h-4 absolute left-3 top-2.5 text-muted-foreground">
                              <path strokeLinecap="round" strokeLinejoin="round" d="M21 21l-5.197-5.197m0 0A7.5 7.5 0 105.196 5.196a7.5 7.5 0 0010.607 10.607z" />
                            </svg>
                          </div>

                          {/* 状态筛选器 */}
                          <select
                            value={clientStatusFilter}
                            onChange={(e) => setClientStatusFilter(e.target.value as 'all' | 'online' | 'offline')}
                            className="w-full px-3 py-1.5 border border-border rounded-lg text-xs focus:ring-2 focus:ring-primary focus:border-transparent"
                          >
                            <option value="all">全部状态</option>
                            <option value="online">仅在线</option>
                            <option value="offline">仅离线</option>
                          </select>
                        </div>
                      )}

                      <div className="grid grid-cols-1 gap-2 max-h-48 overflow-y-auto pr-1">
                        {clients.length === 0 ? (
                          <div className="text-center py-8 text-muted-foreground text-sm">
                            暂无可用客户端，请先创建客户端
                          </div>
                        ) : getFilteredClients().length === 0 ? (
                          <div className="text-center py-8 text-muted-foreground text-sm">
                            没有符合条件的客户端
                          </div>
                        ) : (
                          getFilteredClients().map((client) => (
                          <button
                            key={client.id}
                            type="button"
                            onClick={() => setFormData({ ...formData, client_id: client.id.toString() })}
                            className={`relative flex items-center gap-3 p-3 rounded-xl border-2 transition-all text-left ${
                              formData.client_id === client.id.toString()
                                ? 'border-primary bg-muted shadow-sm'
                                : 'border-border hover:border-primary/50 hover:bg-accent'
                            }`}
                          >
                            <div className={`w-10 h-10 rounded-lg flex items-center justify-center text-primary-foreground font-semibold text-sm shadow-sm ${
                              formData.client_id === client.id.toString()
                                ? 'bg-primary text-primary-foreground'
                                : 'bg-muted-foreground'
                            }`}>
                              {client.name.charAt(0).toUpperCase()}
                            </div>
                            <div className="flex-1 min-w-0">
                              <div className="flex items-center gap-2">
                                <span className="font-semibold text-foreground text-sm truncate">{client.name}</span>
                                {client.is_online && (
                                  <span className="flex items-center gap-1 px-1.5 py-0.5 bg-green-100 text-green-700 text-xs font-medium rounded">
                                    <span className="w-1.5 h-1.5 bg-green-500 rounded-full"></span>
                                    在线
                                  </span>
                                )}
                              </div>
                              <p className="text-xs text-muted-foreground mt-0.5">ID: {client.id}</p>
                            </div>
                            {formData.client_id === client.id.toString() && (
                              <div className="flex-shrink-0">
                                <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" strokeWidth={2.5} stroke="currentColor" className="w-5 h-5 text-primary">
                                  <path strokeLinecap="round" strokeLinejoin="round" d="M9 12.75L11.25 15 15 9.75M21 12a9 9 0 11-18 0 9 9 0 0118 0z" />
                                </svg>
                              </div>
                            )}
                          </button>
                        ))
                      )}
                    </div>
                    </>
                  )}
                </div>

                {/* 节点选择 */}
                <div>
                  <label className="block text-sm font-semibold text-foreground mb-3">选择节点 *</label>
                  {editingProxy ? (
                    <div className="px-4 py-3 bg-muted rounded-xl text-muted-foreground text-sm">
                      {getNodeName(formData.node_id ? parseInt(formData.node_id) : null)} (编辑时不可更改)
                    </div>
                  ) : (
                    <>
                      {/* 搜索和筛选 */}
                      {availableNodes.length > 3 && (
                        <div className="mb-3 space-y-2">
                          {/* 搜索框 */}
                          <div className="relative">
                            <input
                              type="text"
                              value={nodeSearchQuery}
                              onChange={(e) => setNodeSearchQuery(e.target.value)}
                              placeholder="搜索节点名称或地区..."
                              className="w-full pl-10 pr-4 py-2 border border-border rounded-lg focus:ring-2 focus:ring-primary focus:border-transparent text-sm"
                            />
                            <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" strokeWidth={2} stroke="currentColor" className="w-4 h-4 absolute left-3 top-2.5 text-muted-foreground">
                              <path strokeLinecap="round" strokeLinejoin="round" d="M21 21l-5.197-5.197m0 0A7.5 7.5 0 105.196 5.196a7.5 7.5 0 0010.607 10.607z" />
                            </svg>
                          </div>

                          {/* 筛选器 */}
                          <div className="flex gap-2">
                            <select
                              value={nodeTypeFilter}
                              onChange={(e) => setNodeTypeFilter(e.target.value as 'all' | 'shared' | 'dedicated')}
                              className="flex-1 px-3 py-1.5 border border-border rounded-lg text-xs focus:ring-2 focus:ring-primary focus:border-transparent"
                            >
                              <option value="all">全部类型</option>
                              <option value="shared">共享节点</option>
                              <option value="dedicated">独享节点</option>
                            </select>
                            <select
                              value={nodeStatusFilter}
                              onChange={(e) => setNodeStatusFilter(e.target.value as 'all' | 'online' | 'offline')}
                              className="flex-1 px-3 py-1.5 border border-border rounded-lg text-xs focus:ring-2 focus:ring-primary focus:border-transparent"
                            >
                              <option value="all">全部状态</option>
                              <option value="online">仅在线</option>
                              <option value="offline">仅离线</option>
                            </select>
                          </div>
                        </div>
                      )}

                      <div className="grid grid-cols-1 gap-2 max-h-48 overflow-y-auto pr-1">
                        {availableNodes.length === 0 ? (
                          <div className="text-center py-8 text-muted-foreground text-sm">
                            暂无可用节点
                          </div>
                        ) : getFilteredNodes().length === 0 ? (
                          <div className="text-center py-8 text-muted-foreground text-sm">
                            没有符合条件的节点
                          </div>
                        ) : (
                          getFilteredNodes().map((node) => (
                            <button
                              key={node.id}
                              type="button"
                              disabled={!node.isOnline}
                              onClick={() => node.isOnline && setFormData({ ...formData, node_id: node.id.toString() })}
                              className={`relative flex items-center gap-3 p-3 rounded-xl border-2 transition-all text-left ${
                                !node.isOnline
                                  ? 'border-border bg-muted/50 opacity-50 cursor-not-allowed'
                                  : formData.node_id === node.id.toString()
                                    ? 'border-primary bg-muted shadow-sm'
                                    : 'border-border hover:border-primary/50 hover:bg-accent'
                              }`}
                            >
                              <div className={`w-10 h-10 rounded-lg flex items-center justify-center text-primary-foreground font-semibold text-sm shadow-sm ${
                                !node.isOnline
                                  ? 'bg-muted-foreground/50'
                                  : formData.node_id === node.id.toString()
                                    ? 'bg-primary text-primary-foreground'
                                    : 'bg-muted-foreground'
                              }`}>
                                <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" strokeWidth={2} stroke="currentColor" className="w-5 h-5">
                                  <path strokeLinecap="round" strokeLinejoin="round" d="M5.25 14.25h13.5m-13.5 0a3 3 0 01-3-3m3 3a3 3 0 100 6h13.5a3 3 0 100-6m-16.5-3a3 3 0 013-3h13.5a3 3 0 013 3m-19.5 0a4.5 4.5 0 01.9-2.7L5.737 5.1a3.375 3.375 0 012.7-1.35h7.126c1.062 0 2.062.5 2.7 1.35l2.587 3.45a4.5 4.5 0 01.9 2.7m0 0a3 3 0 01-3 3m0 3h.008v.008h-.008v-.008zm0-6h.008v.008h-.008v-.008zm-3 6h.008v.008h-.008v-.008zm0-6h.008v.008h-.008v-.008z" />
                                </svg>
                              </div>
                              <div className="flex-1 min-w-0">
                                <div className="flex items-center gap-2 flex-wrap">
                                  <span className="font-semibold text-foreground text-sm truncate">{node.name}</span>
                                  {node.isOnline ? (
                                    <span className="flex items-center gap-1 px-1.5 py-0.5 bg-green-100 text-green-700 text-xs font-medium rounded">
                                      <span className="w-1.5 h-1.5 bg-green-500 rounded-full"></span>
                                      在线
                                    </span>
                                  ) : (
                                    <span className="flex items-center gap-1 px-1.5 py-0.5 bg-red-100 text-red-600 text-xs font-medium rounded">
                                      <span className="w-1.5 h-1.5 bg-red-400 rounded-full"></span>
                                      离线
                                    </span>
                                  )}
                                  <span className={`px-1.5 py-0.5 text-xs font-medium rounded ${
                                    node.nodeType === 'shared'
                                      ? 'bg-muted text-primary'
                                      : 'bg-purple-100 text-purple-700'
                                  }`}>
                                    {node.nodeType === 'shared' ? '共享' : '独享'}
                                  </span>
                                </div>
                                <div className="flex items-center gap-2 mt-0.5">
                                  {node.region && (
                                    <span className="text-xs text-muted-foreground flex items-center gap-1">
                                      <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" strokeWidth={2} stroke="currentColor" className="w-3 h-3">
                                        <path strokeLinecap="round" strokeLinejoin="round" d="M15 10.5a3 3 0 11-6 0 3 3 0 016 0z" />
                                        <path strokeLinecap="round" strokeLinejoin="round" d="M19.5 10.5c0 7.142-7.5 11.25-7.5 11.25S4.5 17.642 4.5 10.5a7.5 7.5 0 1115 0z" />
                                      </svg>
                                      {node.region}
                                    </span>
                                  )}
                                </div>
                              </div>
                              {formData.node_id === node.id.toString() && (
                                <div className="flex-shrink-0">
                                  <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" strokeWidth={2.5} stroke="currentColor" className="w-5 h-5 text-primary">
                                    <path strokeLinecap="round" strokeLinejoin="round" d="M9 12.75L11.25 15 15 9.75M21 12a9 9 0 11-18 0 9 9 0 0118 0z" />
                                  </svg>
                                </div>
                              )}
                            </button>
                          ))
                        )}
                      </div>
                      <p className="text-xs text-muted-foreground mt-2 flex items-start gap-1.5">
                        <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" strokeWidth={2} stroke="currentColor" className="w-3.5 h-3.5 mt-0.5 flex-shrink-0">
                          <path strokeLinecap="round" strokeLinejoin="round" d="M11.25 11.25l.041-.02a.75.75 0 011.063.852l-.708 2.836a.75.75 0 001.063.853l.041-.021M21 12a9 9 0 11-18 0 9 9 0 0118 0zm-9-3.75h.008v.008H12V8.25z" />
                        </svg>
                        <span>共享节点对所有用户可用，独享节点仅限分配的用户使用。离线节点无法创建代理。</span>
                      </p>
                    </>
                  )}
                </div>
                <div>
                  <label className="block text-sm font-medium text-foreground mb-1.5">代理名称 *</label>
                  <input
                    type="text"
                    value={formData.name}
                    onChange={(e) => setFormData({ ...formData, name: e.target.value })}
                    placeholder="请输入代理名称"
                    className="w-full px-4 py-3 border border-border rounded-xl text-foreground placeholder-muted-foreground focus:outline-none focus:ring-2 focus:ring-primary/20 focus:border-primary transition-all bg-muted/50 hover:bg-card"
                  />
                </div>
                <div className="grid grid-cols-2 gap-4">
                  <div>
                    <label className="block text-sm font-medium text-foreground mb-1.5">代理类型 *</label>
                    <select
                      value={formData.type}
                      onChange={(e) => setFormData({ ...formData, type: e.target.value })}
                      className="w-full px-4 py-3 border border-border rounded-xl text-foreground focus:outline-none focus:ring-2 focus:ring-primary/20 focus:border-primary transition-all bg-muted/50 hover:bg-card"
                    >
                      <option value="tcp">TCP</option>
                      <option value="udp">UDP</option>
                    </select>
                  </div>
                  <div>
                    <label className="block text-sm font-medium text-foreground mb-1.5">客户端本地 IP *</label>
                    <input
                      type="text"
                      value={formData.localIP}
                      onChange={(e) => setFormData({ ...formData, localIP: e.target.value })}
                      className="w-full px-4 py-3 border border-border rounded-xl text-foreground focus:outline-none focus:ring-2 focus:ring-primary/20 focus:border-primary transition-all bg-muted/50 hover:bg-card"
                    />
                  </div>
                </div>
                {editingGroupId ? (
                <div>
                  <label className="block text-sm font-medium text-foreground mb-1.5">端口映射 ({editingGroupPorts.length} 个)</label>
                  <div className="border border-border rounded-xl overflow-hidden">
                    <div className="grid grid-cols-[1fr_auto_1fr] gap-0 px-3 py-2 bg-muted/50 text-xs font-medium text-muted-foreground border-b border-border">
                      <span>节点端口</span>
                      <span className="px-3"></span>
                      <span>本地端口</span>
                    </div>
                    <div className="max-h-48 overflow-y-auto divide-y divide-border">
                      {editingGroupPorts.map((item, idx) => (
                        <div key={item.id} className="grid grid-cols-[1fr_auto_1fr] gap-0 items-center px-3 py-1.5">
                          <input
                            type="number"
                            value={item.remotePort}
                            onChange={(e) => {
                              const next = [...editingGroupPorts];
                              next[idx] = { ...next[idx], remotePort: e.target.value };
                              setEditingGroupPorts(next);
                            }}
                            className="w-full px-2 py-1.5 border border-border rounded-lg text-sm font-mono focus:outline-none focus:ring-2 focus:ring-primary/20 focus:border-primary bg-muted/50"
                          />
                          <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" strokeWidth={2} stroke="currentColor" className="w-4 h-4 mx-2 text-muted-foreground flex-shrink-0">
                            <path strokeLinecap="round" strokeLinejoin="round" d="M13.5 4.5L21 12m0 0l-7.5 7.5M21 12H3" />
                          </svg>
                          <input
                            type="number"
                            value={item.localPort}
                            onChange={(e) => {
                              const next = [...editingGroupPorts];
                              next[idx] = { ...next[idx], localPort: e.target.value };
                              setEditingGroupPorts(next);
                            }}
                            className="w-full px-2 py-1.5 border border-border rounded-lg text-sm font-mono focus:outline-none focus:ring-2 focus:ring-primary/20 focus:border-primary bg-muted/50"
                          />
                        </div>
                      ))}
                    </div>
                  </div>
                </div>
                ) : (
                <div className="grid grid-cols-2 gap-4">
                  <div>
                    <label className="block text-sm font-medium text-foreground mb-1.5">客户端本地端口 *</label>
                    <input
                      type="text"
                      value={formData.localPort}
                      onChange={(e) => setFormData({ ...formData, localPort: e.target.value })}
                      placeholder="如: 80 或 8080-9000"
                      className="w-full px-4 py-3 border border-border rounded-xl text-foreground placeholder-muted-foreground focus:outline-none focus:ring-2 focus:ring-primary/20 focus:border-primary transition-all bg-muted/50 hover:bg-card"
                    />
                    <p className="mt-1.5 text-xs text-muted-foreground">
                      填单个端口则所有代理共用，填范围则与节点端口一一对应
                    </p>
                  </div>
                  <div>
                    <label className="block text-sm font-medium text-foreground mb-1.5">节点端口 *</label>
                    <input
                      type="text"
                      value={formData.remotePort}
                      onChange={(e) => setFormData({ ...formData, remotePort: e.target.value })}
                      placeholder="如: 8080 或 8000-8010 或 8000,8001,8002"
                      className="w-full px-4 py-3 border border-border rounded-xl text-foreground placeholder-muted-foreground focus:outline-none focus:ring-2 focus:ring-primary/20 focus:border-primary transition-all bg-muted/50 hover:bg-card"
                    />

                    {/* 端口解析结果显示 */}
                    {formData.remotePort && (
                      <div className="mt-2 space-y-2">
                        {portParseError ? (
                          <div className="p-3 bg-red-50 border border-red-200 rounded-lg">
                            <div className="flex items-start gap-2">
                              <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" strokeWidth={2} stroke="currentColor" className="w-4 h-4 text-red-600 mt-0.5 flex-shrink-0">
                                <path strokeLinecap="round" strokeLinejoin="round" d="M12 9v3.75m-9.303 3.376c-.866 1.5.217 3.374 1.948 3.374h14.71c1.73 0 2.813-1.874 1.948-3.374L13.949 3.378c-.866-1.5-3.032-1.5-3.898 0L2.697 16.126zM12 15.75h.007v.008H12v-.008z" />
                              </svg>
                              <div className="text-xs text-red-700">
                                <p className="font-medium">{portParseError}</p>
                              </div>
                            </div>
                          </div>
                        ) : parsedPorts.length > 0 && (
                          <div className="space-y-2">
                            {/* 端口数量显示 */}
                            <div className="p-3 bg-muted border border-border rounded-lg">
                              <div className="flex items-start gap-2">
                                <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" strokeWidth={2} stroke="currentColor" className="w-4 h-4 text-primary mt-0.5 flex-shrink-0">
                                  <path strokeLinecap="round" strokeLinejoin="round" d="M9 12.75L11.25 15 15 9.75M21 12a9 9 0 11-18 0 9 9 0 0118 0z" />
                                </svg>
                                <div className="text-xs text-primary flex-1">
                                  <p className="font-medium mb-1">
                                    将创建 {parsedPorts.length} 个代理
                                  </p>
                                  {(() => {
                                    const { ports: lPorts } = parsePortString(formData.localPort);
                                    const isMapping = lPorts.length > 1 && lPorts.length === parsedPorts.length;
                                    if (isMapping) {
                                      // 显示本地端口→节点端口映射
                                      const items = parsedPorts.slice(0, 5).map((rp, i) => `${lPorts[i]}→${rp}`);
                                      return (
                                        <p className="text-primary">
                                          映射: {items.join(', ')}{parsedPorts.length > 5 ? ` ... (共 ${parsedPorts.length} 个)` : ''}
                                        </p>
                                      );
                                    } else {
                                      // 单个本地端口，只显示节点端口
                                      return parsedPorts.length <= 10 ? (
                                        <p className="text-primary">
                                          节点端口: {parsedPorts.join(', ')}
                                        </p>
                                      ) : (
                                        <p className="text-primary">
                                          节点端口: {parsedPorts.slice(0, 10).join(', ')} ... (共 {parsedPorts.length} 个)
                                        </p>
                                      );
                                    }
                                  })()}
                                </div>
                              </div>
                            </div>

                            {/* 端口配额验证 */}
                            {userPortInfo && userPortInfo.maxPortCount !== null && (
                              <div className={`p-3 border rounded-lg ${
                                parsedPorts.length + userPortInfo.currentPortCount > userPortInfo.maxPortCount
                                  ? 'bg-red-50 border-red-200'
                                  : parsedPorts.length + userPortInfo.currentPortCount > userPortInfo.maxPortCount * 0.8
                                  ? 'bg-amber-50 border-amber-200'
                                  : 'bg-green-50 border-green-200'
                              }`}>
                                <div className="flex items-start gap-2">
                                  <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" strokeWidth={2} stroke="currentColor" className={`w-4 h-4 mt-0.5 flex-shrink-0 ${
                                    parsedPorts.length + userPortInfo.currentPortCount > userPortInfo.maxPortCount
                                      ? 'text-red-600'
                                      : parsedPorts.length + userPortInfo.currentPortCount > userPortInfo.maxPortCount * 0.8
                                      ? 'text-amber-600'
                                      : 'text-green-600'
                                  }`}>
                                    <path strokeLinecap="round" strokeLinejoin="round" d="M11.25 11.25l.041-.02a.75.75 0 011.063.852l-.708 2.836a.75.75 0 001.063.853l.041-.021M21 12a9 9 0 11-18 0 9 9 0 0118 0zm-9-3.75h.008v.008H12V8.25z" />
                                  </svg>
                                  <div className={`text-xs flex-1 ${
                                    parsedPorts.length + userPortInfo.currentPortCount > userPortInfo.maxPortCount
                                      ? 'text-red-700'
                                      : parsedPorts.length + userPortInfo.currentPortCount > userPortInfo.maxPortCount * 0.8
                                      ? 'text-amber-700'
                                      : 'text-green-700'
                                  }`}>
                                    <p className="font-medium mb-1">端口配额</p>
                                    <div className="space-y-1">
                                      <p>当前已用: {userPortInfo.currentPortCount} / {userPortInfo.maxPortCount}</p>
                                      <p>本次创建: {parsedPorts.length} 个</p>
                                      <p className="font-semibold">
                                        创建后: {userPortInfo.currentPortCount + parsedPorts.length} / {userPortInfo.maxPortCount}
                                        {parsedPorts.length + userPortInfo.currentPortCount > userPortInfo.maxPortCount && (
                                          <span className="text-red-600"> (超出配额 {parsedPorts.length + userPortInfo.currentPortCount - userPortInfo.maxPortCount} 个)</span>
                                        )}
                                      </p>
                                    </div>
                                  </div>
                                </div>
                              </div>
                            )}
                          </div>
                        )}
                      </div>
                    )}

                    {/* 端口格式说明 */}
                    <p className="mt-2 text-xs text-muted-foreground flex items-start gap-1.5">
                      <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" strokeWidth={2} stroke="currentColor" className="w-3.5 h-3.5 mt-0.5 flex-shrink-0">
                        <path strokeLinecap="round" strokeLinejoin="round" d="M11.25 11.25l.041-.02a.75.75 0 011.063.852l-.708 2.836a.75.75 0 001.063.853l.041-.021M21 12a9 9 0 11-18 0 9 9 0 0118 0zm-9-3.75h.008v.008H12V8.25z" />
                      </svg>
                      <span>支持单个端口（8080）、范围端口（8000-8010）或逗号分隔（8000,8001,8002）</span>
                    </p>
                  </div>
                </div>
                )}
                {editingProxy && (
                  <div className="flex items-center gap-3 p-3 bg-muted rounded-xl">
                    <input
                      type="checkbox"
                      id="enabled"
                      checked={formData.enabled}
                      onChange={(e) => setFormData({ ...formData, enabled: e.target.checked })}
                      className="h-4 w-4 text-primary focus:ring-primary border-border rounded"
                    />
                    <label htmlFor="enabled" className="text-sm text-foreground font-medium">
                      启用代理
                    </label>
                  </div>
                )}
              </div>
            </div>

            {/* 固定底部按钮 */}
            <div className="flex-shrink-0 px-6 py-4 border-t border-border bg-muted/50">
              <div className="flex gap-3">
                <button
                  onClick={() => {
                    setShowCreateModal(false);
                    setEditingProxy(null);
                    resetForm();
                  }}
                  className="flex-1 px-4 py-2.5 bg-card border border-border text-foreground font-medium rounded-xl hover:bg-accent transition-colors"
                >
                  取消
                </button>
                <button
                  onClick={editingGroupId ? handleUpdateGroup : (editingProxy ? handleUpdateProxy : handleCreateProxy)}
                  className="flex-1 px-4 py-2.5 bg-primary text-primary-foreground font-medium rounded-xl hover:bg-primary/90 shadow-sm transition-all"
                >
                  {(editingProxy || editingGroupId) ? '更新' : '创建'}
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
        variant="danger"
        confirmText="删除"
        onConfirm={() => {
          confirmDialog.onConfirm();
          setConfirmDialog(prev => ({ ...prev, open: false }));
        }}
        onCancel={() => setConfirmDialog(prev => ({ ...prev, open: false }))}
      />
    </div>
  );
}
