import { useEffect, useState } from 'react';
import { userService, nodeService } from '../lib/services';
import type { UserWithNodeCount, Node } from '../lib/types';
import { formatDate, formatBytes } from '../lib/utils';
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

export default function Users() {
  const { showToast } = useToast();
  const [users, setUsers] = useState<UserWithNodeCount[]>([]);
  const [nodes, setNodes] = useState<Node[]>([]);
  const [loading, setLoading] = useState(true);
  const [showCreateModal, setShowCreateModal] = useState(false);
  const [showBindModal, setShowBindModal] = useState(false);
  const [showQuotaModal, setShowQuotaModal] = useState(false);
  const [selectedUser, setSelectedUser] = useState<UserWithNodeCount | null>(null);
  const [userNodes, setUserNodes] = useState<Node[]>([]);
  const [quotaSaving, setQuotaSaving] = useState(false);
  const [quotaChangeGb, setQuotaChangeGb] = useState('');
  const [portLimitSaving, setPortLimitSaving] = useState(false);
  const [portLimitData, setPortLimitData] = useState({ maxPortCount: '', allowedPortRange: '', maxNodeCount: '', maxClientCount: '' });
  const [confirmDialog, setConfirmDialog] = useState<{ open: boolean; title: string; message: string; variant: 'danger' | 'warning' | 'info'; confirmText: string; onConfirm: () => void }>({ open: false, title: '', message: '', variant: 'danger', confirmText: '确定', onConfirm: () => {} });
  const [formData, setFormData] = useState({
    username: '',
    password: '',
    is_admin: false,
  });

  useEffect(() => {
    loadUsers();
    loadNodes();
  }, []);

  const loadUsers = async () => {
    try {
      setLoading(true);
      const response = await userService.getUsers();
      if (response.success && response.data) {
        setUsers(response.data);
      }
    } catch (error) {
      console.error('加载用户失败:', error);
      showToast('加载失败', 'error');
    } finally {
      setLoading(false);
    }
  };

  const loadNodes = async () => {
    try {
      const response = await nodeService.getNodes();
      if (response.success && response.data) {
        setNodes(response.data);
      }
    } catch (error) {
      console.error('加载节点失败:', error);
    }
  };

  const resetForm = () => {
    setFormData({ username: '', password: '', is_admin: false });
  };

  const handleCreateUser = async () => {
    if (!formData.username) {
      showToast('请输入用户名', 'error');
      return;
    }

    try {
      const response = await userService.createUser({
        username: formData.username,
        password: formData.password || undefined,
        is_admin: formData.is_admin,
      });
      if (response.success) {
        showToast('用户创建成功', 'success');
        resetForm();
        setShowCreateModal(false);
        loadUsers();
      } else {
        showToast(response.message || '创建失败', 'error');
      }
    } catch (error) {
      console.error('创建用户失败:', error);
      showToast('创建失败', 'error');
    }
  };

  const handleDeleteUser = (id: number) => {
    setConfirmDialog({
      open: true,
      title: '删除用户',
      message: '确定要删除这个用户吗？',
      variant: 'danger',
      confirmText: '删除',
      onConfirm: async () => {
        try {
          const response = await userService.deleteUser(id);
          if (response.success) {
            showToast('用户删除成功', 'success');
            loadUsers();
          } else {
            showToast(response.message || '删除失败', 'error');
          }
        } catch (error) {
          console.error('删除用户失败:', error);
          showToast('删除失败', 'error');
        }
      },
    });
  };

  const handleToggleAdmin = async (user: UserWithNodeCount) => {
    try {
      const response = await userService.updateUser(user.id, {
        is_admin: !user.is_admin,
      });
      if (response.success) {
        showToast(`用户已${user.is_admin ? '取消管理员权限' : '设为管理员'}`, 'success');
        loadUsers();
      } else {
        showToast(response.message || '更新失败', 'error');
      }
    } catch (error) {
      console.error('更新用户失败:', error);
      showToast('更新失败', 'error');
    }
  };

  const handleManageNodes = async (user: UserWithNodeCount) => {
    setSelectedUser(user);
    try {
      const response = await userService.getUserNodes(user.id);
      if (response.success && response.data) {
        setUserNodes(response.data);
      }
    } catch (error) {
      console.error('加载用户节点失败:', error);
    }
    setShowBindModal(true);
  };

  const handleAssignNode = async (nodeId: number) => {
    if (!selectedUser) return;

    try {
      const response = await userService.assignNode(selectedUser.id, nodeId);
      if (response.success) {
        showToast('节点绑定成功', 'success');
        // 重新加载用户的节点列表
        const res = await userService.getUserNodes(selectedUser.id);
        if (res.success && res.data) {
          setUserNodes(res.data);
        }
        loadUsers();
      } else {
        showToast(response.message || '绑定失败', 'error');
      }
    } catch (error) {
      console.error('绑定节点失败:', error);
      showToast('绑定失败', 'error');
    }
  };

  const handleRemoveNode = async (nodeId: number) => {
    if (!selectedUser) return;

    try {
      const response = await userService.removeNode(selectedUser.id, nodeId);
      if (response.success) {
        showToast('节点解绑成功', 'success');
        const res = await userService.getUserNodes(selectedUser.id);
        if (res.success && res.data) {
          setUserNodes(res.data);
        }
        loadUsers();
      } else {
        showToast(response.message || '解绑失败', 'error');
      }
    } catch (error) {
      console.error('解绑节点失败:', error);
      showToast('解绑失败', 'error');
    }
  };

  const handleResetTrafficExceeded = (user: UserWithNodeCount) => {
    setConfirmDialog({
      open: true,
      title: '重置超限状态',
      message: '确定要重置该用户的超限状态吗？',
      variant: 'warning',
      confirmText: '重置',
      onConfirm: async () => {
        try {
          const response = await userService.updateUser(user.id, {
            is_traffic_exceeded: false,
          });
          if (response.success) {
            showToast('超限状态已重置', 'success');
            loadUsers();
          } else {
            showToast(response.message || '重置失败', 'error');
          }
        } catch (error) {
          console.error('重置超限状态失败:', error);
          showToast('重置失败', 'error');
        }
      },
    });
  };

  const handleManageQuota = (user: UserWithNodeCount) => {
    setSelectedUser(user);
    setQuotaChangeGb('');
    setPortLimitData({
      maxPortCount: user.maxPortCount?.toString() || '',
      allowedPortRange: user.allowedPortRange || '',
      maxNodeCount: user.maxNodeCount?.toString() || '',
      maxClientCount: user.maxClientCount?.toString() || '',
    });
    setShowQuotaModal(true);
  };

  const handleAdjustQuota = async (isIncrease: boolean) => {
    if (!selectedUser || !quotaChangeGb) {
      showToast('请输入配额变更数量', 'error');
      return;
    }

    const changeValue = parseFloat(quotaChangeGb);
    if (isNaN(changeValue) || changeValue <= 0) {
      showToast('请输入有效的配额数量', 'error');
      return;
    }

    try {
      setQuotaSaving(true);
      const quotaChange = isIncrease ? changeValue : -changeValue;
      const response = await userService.adjustQuota(selectedUser.id, quotaChange);

      if (response.success) {
        showToast(response.data || '配额调整成功', 'success');
        setShowQuotaModal(false);
        setSelectedUser(null);
        setQuotaChangeGb('');
        loadUsers();
      } else {
        showToast(response.message || '配额调整失败', 'error');
      }
    } catch (error) {
      console.error('配额调整失败:', error);
      showToast('配额调整失败', 'error');
    } finally {
      setQuotaSaving(false);
    }
  };

  const handleSavePortLimit = async () => {
    if (!selectedUser) return;

    try {
      setPortLimitSaving(true);
      const response = await userService.updateUser(selectedUser.id, {
        max_port_count: portLimitData.maxPortCount ? parseInt(portLimitData.maxPortCount) : null,
        allowed_port_range: portLimitData.allowedPortRange || null,
        max_node_count: portLimitData.maxNodeCount ? parseInt(portLimitData.maxNodeCount) : null,
        max_client_count: portLimitData.maxClientCount ? parseInt(portLimitData.maxClientCount) : null,
      });

      if (response.success) {
        showToast('配额设置更新成功', 'success');
        setShowQuotaModal(false);
        setSelectedUser(null);
        setPortLimitData({ maxPortCount: '', allowedPortRange: '', maxNodeCount: '', maxClientCount: '' });
        loadUsers();
      } else {
        showToast(response.message || '更新失败', 'error');
      }
    } catch (error) {
      console.error('更新配额设置失败:', error);
      showToast('更新失败', 'error');
    } finally {
      setPortLimitSaving(false);
    }
  };

  return (
    <div className="space-y-6">
      {/* 页面标题 */}
      <div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
        <div>
          <h2 className="text-2xl font-bold text-foreground">用户管理</h2>
          <p className="mt-1 text-sm text-muted-foreground">管理系统用户和权限分配</p>
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
          新建用户
        </button>
      </div>

      {loading ? (
        <TableSkeleton rows={5} cols={7} />
      ) : (
        <TableContainer>
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>用户名</TableHead>
                <TableHead>角色</TableHead>
                <TableHead>绑定节点</TableHead>
                <TableHead>流量统计</TableHead>
                <TableHead>流量限制</TableHead>
                <TableHead>创建时间</TableHead>
                <TableHead className="text-right">操作</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {users.length === 0 ? (
                <TableRow>
                  <TableCell colSpan={7} className="px-6 py-16 text-center">
                    <div className="flex flex-col items-center gap-3">
                      <div className="w-16 h-16 bg-muted rounded-full flex items-center justify-center">
                        <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" strokeWidth={1.5} stroke="currentColor" className="w-8 h-8 text-muted-foreground">
                          <path strokeLinecap="round" strokeLinejoin="round" d="M15 19.128a9.38 9.38 0 002.625.372 9.337 9.337 0 004.121-.952 4.125 4.125 0 00-7.533-2.493M15 19.128v-.003c0-1.113-.285-2.16-.786-3.07M15 19.128v.106A12.318 12.318 0 018.624 21c-2.331 0-4.512-.645-6.374-1.766l-.001-.109a6.375 6.375 0 0111.964-3.07M12 6.375a3.375 3.375 0 11-6.75 0 3.375 3.375 0 016.75 0zm8.25 2.25a2.625 2.625 0 11-5.25 0 2.625 2.625 0 015.25 0z" />
                        </svg>
                      </div>
                      <p className="text-muted-foreground">暂无用户数据</p>
                    </div>
                  </TableCell>
                </TableRow>
              ) : (
                users.map((user) => (
                  <TableRow key={user.id}>
                    <TableCell className="whitespace-nowrap">
                      <div className="flex items-center gap-3">
                        <div className={`w-10 h-10 rounded-full flex items-center justify-center text-primary-foreground text-sm font-semibold shadow-sm ${
                          user.is_admin
                            ? 'bg-gradient-to-br from-amber-500 to-orange-600'
                            : ''
                        }`}
                        style={!user.is_admin ? { background: 'linear-gradient(135deg, hsl(210 100% 45%), hsl(189 94% 43%))' } : undefined}
                        >
                          {user.username.charAt(0).toUpperCase()}
                        </div>
                        <span className="text-sm font-semibold text-foreground">{user.username}</span>
                      </div>
                    </TableCell>
                    <TableCell className="whitespace-nowrap">
                      {user.is_admin ? (
                        <span className="inline-flex items-center gap-1.5 px-2.5 py-1 text-xs font-semibold rounded-lg bg-gradient-to-r from-amber-100 to-orange-100 text-amber-700">
                          <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" strokeWidth={2} stroke="currentColor" className="w-3.5 h-3.5">
                            <path strokeLinecap="round" strokeLinejoin="round" d="M9 12.75L11.25 15 15 9.75m-3-7.036A11.959 11.959 0 013.598 6 11.99 11.99 0 003 9.749c0 5.592 3.824 10.29 9 11.623 5.176-1.332 9-6.03 9-11.622 0-1.31-.21-2.571-.598-3.751h-.152c-3.196 0-6.1-1.248-8.25-3.285z" />
                          </svg>
                          管理员
                        </span>
                      ) : (
                        <span className="inline-flex items-center gap-1.5 px-2.5 py-1 text-xs font-semibold rounded-lg bg-muted text-muted-foreground">
                          <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" strokeWidth={2} stroke="currentColor" className="w-3.5 h-3.5">
                            <path strokeLinecap="round" strokeLinejoin="round" d="M15.75 6a3.75 3.75 0 11-7.5 0 3.75 3.75 0 017.5 0zM4.501 20.118a7.5 7.5 0 0114.998 0A17.933 17.933 0 0112 21.75c-2.676 0-5.216-.584-7.499-1.632z" />
                          </svg>
                          普通用户
                        </span>
                      )}
                    </TableCell>
                    <TableCell className="whitespace-nowrap">
                      <span className="inline-flex items-center gap-1.5 px-2.5 py-1 text-xs font-medium rounded-lg bg-muted text-primary">
                        <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" strokeWidth={2} stroke="currentColor" className="w-3.5 h-3.5">
                          <path strokeLinecap="round" strokeLinejoin="round" d="M21 7.5l-9-5.25L3 7.5m18 0l-9 5.25m9-5.25v9l-9 5.25M3 7.5l9 5.25M3 7.5v9l9 5.25m0-9v9" />
                        </svg>
                        {user.node_count} 个节点
                      </span>
                    </TableCell>
                    <TableCell className="whitespace-nowrap">
                      <div className="flex flex-col gap-1">
                        <div className="flex items-center gap-1.5 text-xs">
                          <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" strokeWidth={2} stroke="currentColor" className="w-3.5 h-3.5" style={{ color: 'hsl(217 91% 60%)' }}>
                            <path strokeLinecap="round" strokeLinejoin="round" d="M4.5 10.5L12 3m0 0l7.5 7.5M12 3v18" />
                          </svg>
                          <span className="text-muted-foreground">{formatBytes(user.totalBytesSent)}</span>
                        </div>
                        <div className="flex items-center gap-1.5 text-xs">
                          <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" strokeWidth={2} stroke="currentColor" className="w-3.5 h-3.5" style={{ color: 'hsl(142 71% 45%)' }}>
                            <path strokeLinecap="round" strokeLinejoin="round" d="M19.5 13.5L12 21m0 0l-7.5-7.5M12 21V3" />
                          </svg>
                          <span className="text-muted-foreground">{formatBytes(user.totalBytesReceived)}</span>
                        </div>
                      </div>
                    </TableCell>
                    <TableCell className="whitespace-nowrap">
                      <div className="flex flex-col gap-1">
                        {user.trafficQuotaGb ? (
                          <>
                            <div className="text-xs font-medium text-foreground">
                              配额: {user.trafficQuotaGb} GB
                            </div>
                            <div className="text-xs text-green-600">
                              剩余: {user.remainingQuotaGb?.toFixed(2) || '0.00'} GB
                            </div>
                            <div className="w-full bg-muted rounded-full h-1.5 mt-1">
                              <div
                                className={`h-1.5 rounded-full ${
                                  (user.remainingQuotaGb || 0) / user.trafficQuotaGb < 0.2
                                    ? 'bg-red-500'
                                    : (user.remainingQuotaGb || 0) / user.trafficQuotaGb < 0.5
                                    ? 'bg-yellow-500'
                                    : 'bg-green-500'
                                }`}
                                style={{
                                  width: `${Math.max(0, Math.min(100, ((user.remainingQuotaGb || 0) / user.trafficQuotaGb) * 100))}%`,
                                }}
                              ></div>
                            </div>
                            {user.isTrafficExceeded && (
                              <span className="inline-flex items-center gap-1 px-2 py-0.5 text-xs font-semibold rounded-md bg-red-100 text-red-700">
                                <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" strokeWidth={2} stroke="currentColor" className="w-3 h-3">
                                  <path strokeLinecap="round" strokeLinejoin="round" d="M12 9v3.75m-9.303 3.376c-.866 1.5.217 3.374 1.948 3.374h14.71c1.73 0 2.813-1.874 1.948-3.374L13.949 3.378c-.866-1.5-3.032-1.5-3.898 0L2.697 16.126zM12 15.75h.007v.008H12v-.008z" />
                                </svg>
                                配额已用尽
                              </span>
                            )}
                          </>
                        ) : (
                          <span className="text-xs text-muted-foreground">未设置</span>
                        )}
                      </div>
                    </TableCell>
                    <TableCell className="whitespace-nowrap text-sm text-muted-foreground">
                      {formatDate(user.created_at)}
                    </TableCell>
                    <TableCell className="whitespace-nowrap text-right">
                      <div className="flex flex-wrap items-center justify-end gap-1.5">
                        <button
                          onClick={() => handleManageQuota(user)}
                          className="inline-flex items-center gap-1.5 px-3 py-1.5 text-xs font-medium text-emerald-600 hover:bg-emerald-50 rounded-lg transition-colors"
                        >
                          <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" strokeWidth={2} stroke="currentColor" className="w-3.5 h-3.5">
                            <path strokeLinecap="round" strokeLinejoin="round" d="M12 6v12m-3-2.818l.879.659c1.171.879 3.07.879 4.242 0 1.172-.879 1.172-2.303 0-3.182C13.536 12.219 12.768 12 12 12c-.725 0-1.45-.22-2.003-.659-1.106-.879-1.106-2.303 0-3.182s2.9-.879 4.006 0l.415.33M21 12a9 9 0 11-18 0 9 9 0 0118 0z" />
                          </svg>
                          配额设置
                        </button>
                        <button
                          onClick={() => handleManageNodes(user)}
                          className="inline-flex items-center gap-1.5 px-3 py-1.5 text-xs font-medium text-primary hover:bg-accent rounded-lg transition-colors"
                        >
                          <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" strokeWidth={2} stroke="currentColor" className="w-3.5 h-3.5">
                            <path strokeLinecap="round" strokeLinejoin="round" d="M13.19 8.688a4.5 4.5 0 011.242 7.244l-4.5 4.5a4.5 4.5 0 01-6.364-6.364l1.757-1.757m13.35-.622l1.757-1.757a4.5 4.5 0 00-6.364-6.364l-4.5 4.5a4.5 4.5 0 001.242 7.244" />
                          </svg>
                          管理节点
                        </button>
                        {user.isTrafficExceeded && (
                          <button
                            onClick={() => handleResetTrafficExceeded(user)}
                            className="inline-flex items-center gap-1.5 px-3 py-1.5 text-xs font-medium text-orange-600 hover:bg-orange-50 rounded-lg transition-colors"
                          >
                            <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" strokeWidth={2} stroke="currentColor" className="w-3.5 h-3.5">
                              <path strokeLinecap="round" strokeLinejoin="round" d="M16.023 9.348h4.992v-.001M2.985 19.644v-4.992m0 0h4.992m-4.993 0l3.181 3.183a8.25 8.25 0 0013.803-3.7M4.031 9.865a8.25 8.25 0 0113.803-3.7l3.181 3.182M2.985 19.644l3.181-3.182" />
                            </svg>
                            重置超限
                          </button>
                        )}
                        <button
                          onClick={() => handleToggleAdmin(user)}
                          className="inline-flex items-center gap-1.5 px-3 py-1.5 text-xs font-medium text-purple-600 hover:bg-purple-50 rounded-lg transition-colors"
                        >
                          <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" strokeWidth={2} stroke="currentColor" className="w-3.5 h-3.5">
                            <path strokeLinecap="round" strokeLinejoin="round" d="M9 12.75L11.25 15 15 9.75m-3-7.036A11.959 11.959 0 013.598 6 11.99 11.99 0 003 9.749c0 5.592 3.824 10.29 9 11.623 5.176-1.332 9-6.03 9-11.622 0-1.31-.21-2.571-.598-3.751h-.152c-3.196 0-6.1-1.248-8.25-3.285z" />
                          </svg>
                          {user.is_admin ? '取消管理员' : '设为管理员'}
                        </button>
                        <button
                          onClick={() => handleDeleteUser(user.id)}
                          className="inline-flex items-center gap-1.5 px-3 py-1.5 text-xs font-medium text-red-600 hover:bg-red-50 rounded-lg transition-colors"
                        >
                          <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" strokeWidth={2} stroke="currentColor" className="w-3.5 h-3.5">
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

      {/* 创建用户模态框 */}
      {showCreateModal && (
        <div className="fixed inset-0 bg-black/50 backdrop-blur-sm overflow-y-auto h-full w-full flex items-center justify-center z-50">
          <div className="relative bg-card rounded-2xl shadow-2xl w-full max-w-md mx-4 transform transition-all">
            <div className="p-6">
              <div className="flex items-center gap-3 mb-6">
                <div className="w-10 h-10 rounded-xl flex items-center justify-center" style={{ background: 'linear-gradient(135deg, hsl(210 100% 45%), hsl(189 94% 43%))' }}>
                  <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" strokeWidth={1.5} stroke="currentColor" className="w-5 h-5 text-primary-foreground">
                    <path strokeLinecap="round" strokeLinejoin="round" d="M19 7.5v3m0 0v3m0-3h3m-3 0h-3m-2.25-4.125a3.375 3.375 0 11-6.75 0 3.375 3.375 0 016.75 0zM4 19.235v-.11a6.375 6.375 0 0112.75 0v.109A12.318 12.318 0 0110.374 21c-2.331 0-4.512-.645-6.374-1.766z" />
                  </svg>
                </div>
                <div>
                  <h3 className="text-lg font-bold text-foreground">创建新用户</h3>
                  <p className="text-sm text-muted-foreground">添加一个新的系统用户</p>
                </div>
              </div>
              <div className="space-y-4">
                <div>
                  <label className="block text-sm font-medium text-foreground mb-1.5">用户名 *</label>
                  <input
                    type="text"
                    value={formData.username}
                    onChange={(e) => setFormData({ ...formData, username: e.target.value })}
                    placeholder="请输入用户名"
                    className="w-full px-4 py-3 border border-border rounded-xl text-foreground placeholder-muted-foreground focus:outline-none focus:ring-2 focus:ring-primary/20 focus:border-primary transition-all bg-muted/50 hover:bg-card"
                  />
                </div>
                <div>
                  <label className="block text-sm font-medium text-foreground mb-1.5">密码</label>
                  <input
                    type="password"
                    value={formData.password}
                    onChange={(e) => setFormData({ ...formData, password: e.target.value })}
                    placeholder="留空自动生成"
                    className="w-full px-4 py-3 border border-border rounded-xl text-foreground placeholder-muted-foreground focus:outline-none focus:ring-2 focus:ring-primary/20 focus:border-primary transition-all bg-muted/50 hover:bg-card"
                  />
                </div>
                <div className="flex items-center gap-3 p-3 bg-muted rounded-xl">
                  <input
                    type="checkbox"
                    id="is_admin"
                    checked={formData.is_admin}
                    onChange={(e) => setFormData({ ...formData, is_admin: e.target.checked })}
                    className="h-4 w-4 text-primary focus:ring-primary border-border rounded"
                  />
                  <label htmlFor="is_admin" className="text-sm text-foreground font-medium">
                    设为管理员
                  </label>
                </div>
              </div>
              <div className="mt-6 flex gap-3">
                <button
                  onClick={() => {
                    setShowCreateModal(false);
                    resetForm();
                  }}
                  className="flex-1 px-4 py-2.5 bg-muted text-foreground font-medium rounded-xl hover:bg-accent transition-colors"
                >
                  取消
                </button>
                <button
                  onClick={handleCreateUser}
                  className="flex-1 px-4 py-2.5 bg-primary text-primary-foreground font-medium rounded-xl hover:bg-primary/90 shadow-sm transition-all"
                >
                  创建
                </button>
              </div>
            </div>
          </div>
        </div>
      )}

      {/* 管理节点绑定模态框 */}
      {showBindModal && selectedUser && (
        <div className="fixed inset-0 bg-black/50 backdrop-blur-sm overflow-y-auto h-full w-full flex items-center justify-center z-50">
          <div className="relative bg-card rounded-2xl shadow-2xl w-full max-w-xl mx-4 max-h-[85vh] flex flex-col transform transition-all">
            <div className="flex items-center justify-between p-6 border-b border-border">
              <div className="flex items-center gap-3">
                <div className="w-10 h-10 rounded-xl flex items-center justify-center" style={{ background: 'linear-gradient(135deg, hsl(210 100% 45%), hsl(189 94% 43%))' }}>
                  <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" strokeWidth={1.5} stroke="currentColor" className="w-5 h-5 text-primary-foreground">
                    <path strokeLinecap="round" strokeLinejoin="round" d="M13.19 8.688a4.5 4.5 0 011.242 7.244l-4.5 4.5a4.5 4.5 0 01-6.364-6.364l1.757-1.757m13.35-.622l1.757-1.757a4.5 4.5 0 00-6.364-6.364l-4.5 4.5a4.5 4.5 0 001.242 7.244" />
                  </svg>
                </div>
                <div>
                  <h3 className="text-lg font-bold text-foreground">管理用户节点</h3>
                  <p className="text-sm text-muted-foreground">{selectedUser.username}</p>
                </div>
              </div>
              <button
                onClick={() => {
                  setShowBindModal(false);
                  setSelectedUser(null);
                  setUserNodes([]);
                }}
                className="p-2 text-muted-foreground hover:text-muted-foreground hover:bg-accent rounded-lg transition-colors"
              >
                <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" strokeWidth={2} stroke="currentColor" className="w-5 h-5">
                  <path strokeLinecap="round" strokeLinejoin="round" d="M6 18L18 6M6 6l12 12" />
                </svg>
              </button>
            </div>

            <div className="flex-1 overflow-y-auto p-6 space-y-6">
              <div>
                <h4 className="text-sm font-semibold text-foreground mb-3 flex items-center gap-2">
                  <span className="w-1.5 h-1.5 bg-green-500 rounded-full"></span>
                  已绑定的节点
                </h4>
                {userNodes.length === 0 ? (
                  <div className="text-center py-8 bg-muted rounded-xl">
                    <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" strokeWidth={1.5} stroke="currentColor" className="w-10 h-10 text-muted-foreground mx-auto mb-2">
                      <path strokeLinecap="round" strokeLinejoin="round" d="M21 7.5l-9-5.25L3 7.5m18 0l-9 5.25m9-5.25v9l-9 5.25M3 7.5l9 5.25M3 7.5v9l9 5.25m0-9v9" />
                    </svg>
                    <p className="text-sm text-muted-foreground">暂无绑定节点</p>
                  </div>
                ) : (
                  <div className="space-y-2">
                    {userNodes.map((node) => (
                      <div
                        key={node.id}
                        className="flex items-center justify-between p-4 bg-gradient-to-r from-muted to-card rounded-xl border border-border"
                      >
                        <div className="flex items-center gap-3">
                          <div className="w-8 h-8 rounded-lg flex items-center justify-center text-primary-foreground text-xs font-semibold" style={{ background: 'linear-gradient(135deg, hsl(210 100% 45%), hsl(189 94% 43%))' }}>
                            {node.name.charAt(0).toUpperCase()}
                          </div>
                          <div>
                            <span className="text-sm font-medium text-foreground">{node.name}</span>
                            <div className="flex items-center gap-1.5 mt-0.5">
                              <span className={`w-1.5 h-1.5 rounded-full ${node.isOnline ? 'bg-green-500' : 'bg-muted'}`}></span>
                              <span className="text-xs text-muted-foreground">{node.isOnline ? '在线' : '离线'}</span>
                              {node.region && <span className="text-xs text-muted-foreground ml-1">({node.region})</span>}
                            </div>
                          </div>
                        </div>
                        <button
                          onClick={() => handleRemoveNode(node.id)}
                          className="px-3 py-1.5 text-xs font-medium text-red-600 hover:bg-red-50 rounded-lg transition-colors"
                        >
                          解绑
                        </button>
                      </div>
                    ))}
                  </div>
                )}
              </div>

              <div>
                <h4 className="text-sm font-semibold text-foreground mb-3 flex items-center gap-2">
                  <span className="w-1.5 h-1.5 rounded-full" style={{ background: 'hsl(217 91% 60%)' }}></span>
                  可绑定的独享节点
                </h4>
                <div className="space-y-2 max-h-48 overflow-y-auto">
                  {nodes
                    .filter((n) => !userNodes.find((un) => un.id === n.id) && n.nodeType === 'dedicated')
                    .map((node) => (
                      <div
                        key={node.id}
                        className="flex items-center justify-between p-4 bg-muted rounded-xl hover:bg-accent transition-colors"
                      >
                        <div className="flex items-center gap-3">
                          <div className="w-8 h-8 bg-muted rounded-lg flex items-center justify-center text-muted-foreground text-xs font-semibold">
                            {node.name.charAt(0).toUpperCase()}
                          </div>
                          <div>
                            <span className="text-sm font-medium text-foreground">{node.name}</span>
                            <div className="flex items-center gap-1.5 mt-0.5">
                              <span className={`w-1.5 h-1.5 rounded-full ${node.isOnline ? 'bg-green-500' : 'bg-muted'}`}></span>
                              <span className="text-xs text-muted-foreground">{node.isOnline ? '在线' : '离线'}</span>
                              {node.region && <span className="text-xs text-muted-foreground ml-1">({node.region})</span>}
                            </div>
                          </div>
                        </div>
                        <button
                          onClick={() => handleAssignNode(node.id)}
                          className="px-3 py-1.5 text-xs font-medium text-primary hover:bg-accent rounded-lg transition-colors"
                        >
                          绑定
                        </button>
                      </div>
                    ))}
                </div>
              </div>
            </div>

            <div className="p-4 border-t border-border flex justify-end">
              <button
                onClick={() => {
                  setShowBindModal(false);
                  setSelectedUser(null);
                  setUserNodes([]);
                }}
                className="px-5 py-2.5 bg-muted text-foreground font-medium rounded-xl hover:bg-accent transition-colors"
              >
                关闭
              </button>
            </div>
          </div>
        </div>
      )}

      {/* 配额设置模态框（合并流量配额 + 端口/数量限制） */}
      {showQuotaModal && selectedUser && (
        <div className="fixed inset-0 bg-black/50 backdrop-blur-sm overflow-y-auto h-full w-full flex items-center justify-center z-50">
          <div className="relative bg-card rounded-2xl shadow-2xl w-full max-w-md mx-4 transform transition-all max-h-[90vh] overflow-y-auto">
            <div className="p-6">
              <div className="flex items-center gap-3 mb-6">
                <div className="w-10 h-10 bg-gradient-to-br from-emerald-500 to-teal-600 rounded-xl flex items-center justify-center">
                  <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" strokeWidth={1.5} stroke="currentColor" className="w-5 h-5 text-primary-foreground">
                    <path strokeLinecap="round" strokeLinejoin="round" d="M12 6v12m-3-2.818l.879.659c1.171.879 3.07.879 4.242 0 1.172-.879 1.172-2.303 0-3.182C13.536 12.219 12.768 12 12 12c-.725 0-1.45-.22-2.003-.659-1.106-.879-1.106-2.303 0-3.182s2.9-.879 4.006 0l.415.33M21 12a9 9 0 11-18 0 9 9 0 0118 0z" />
                  </svg>
                </div>
                <div>
                  <h3 className="text-lg font-bold text-foreground">配额设置</h3>
                  <p className="text-sm text-muted-foreground">{selectedUser.username}</p>
                </div>
              </div>

              {/* 流量配额区域 */}
              <div className="mb-5">
                <h4 className="text-sm font-semibold text-foreground mb-3">流量配额</h4>
                <div className="p-4 bg-muted rounded-xl border border-border mb-3">
                  <div className="space-y-2">
                    <div className="flex justify-between items-center">
                      <span className="text-sm text-muted-foreground">当前配额</span>
                      <span className="text-sm font-semibold text-foreground">
                        {selectedUser.trafficQuotaGb ? `${selectedUser.trafficQuotaGb} GB` : '未设置'}
                      </span>
                    </div>
                    {selectedUser.trafficQuotaGb && (
                      <>
                        <div className="flex justify-between items-center">
                          <span className="text-sm text-muted-foreground">剩余配额</span>
                          <span className="text-sm font-semibold text-green-600">
                            {selectedUser.remainingQuotaGb?.toFixed(2) || '0.00'} GB
                          </span>
                        </div>
                        <div className="w-full bg-muted rounded-full h-2 mt-2">
                          <div
                            className={`h-2 rounded-full transition-all ${
                              (selectedUser.remainingQuotaGb || 0) / selectedUser.trafficQuotaGb < 0.2
                                ? 'bg-red-500'
                                : (selectedUser.remainingQuotaGb || 0) / selectedUser.trafficQuotaGb < 0.5
                                ? 'bg-yellow-500'
                                : 'bg-green-500'
                            }`}
                            style={{
                              width: `${Math.max(0, Math.min(100, ((selectedUser.remainingQuotaGb || 0) / selectedUser.trafficQuotaGb) * 100))}%`,
                            }}
                          ></div>
                        </div>
                      </>
                    )}
                  </div>
                </div>
                <div>
                  <label className="block text-sm font-medium text-foreground mb-1.5">调整配额 (GB)</label>
                  <div className="flex gap-2">
                    <input
                      type="number"
                      step="0.1"
                      min="0"
                      value={quotaChangeGb}
                      onChange={(e) => setQuotaChangeGb(e.target.value)}
                      placeholder="输入配额数量"
                      className="flex-1 px-4 py-2.5 border border-border rounded-xl text-foreground placeholder-muted-foreground focus:outline-none focus:ring-2 focus:ring-emerald-500/20 focus:border-emerald-500 transition-all bg-muted/50 hover:bg-card"
                    />
                    <button
                      onClick={() => handleAdjustQuota(false)}
                      disabled={quotaSaving || !quotaChangeGb}
                      className="px-3 py-2.5 bg-gradient-to-r from-red-500 to-rose-600 text-primary-foreground text-xs font-medium rounded-xl hover:from-red-600 hover:to-rose-700 shadow-sm transition-all disabled:opacity-50 disabled:cursor-not-allowed"
                    >
                      {quotaSaving ? '...' : '减少'}
                    </button>
                    <button
                      onClick={() => handleAdjustQuota(true)}
                      disabled={quotaSaving || !quotaChangeGb}
                      className="px-3 py-2.5 bg-gradient-to-r from-emerald-500 to-teal-600 text-primary-foreground text-xs font-medium rounded-xl hover:from-emerald-600 hover:to-teal-700 shadow-sm transition-all disabled:opacity-50 disabled:cursor-not-allowed"
                    >
                      {quotaSaving ? '...' : '增加'}
                    </button>
                  </div>
                </div>
              </div>

              <hr className="border-border mb-5" />

              {/* 数量限制区域 */}
              <div className="mb-5">
                <h4 className="text-sm font-semibold text-foreground mb-3">数量限制</h4>
                <div className="p-4 bg-muted rounded-xl border border-border mb-3">
                  <div className="space-y-2">
                    <div className="flex justify-between items-center">
                      <span className="text-sm text-muted-foreground">端口数量</span>
                      <span className="text-sm font-semibold text-foreground">
                        {selectedUser.currentPortCount || 0}{selectedUser.maxPortCount ? ` / ${selectedUser.maxPortCount}` : ''} 个
                      </span>
                    </div>
                    <div className="flex justify-between items-center">
                      <span className="text-sm text-muted-foreground">节点数量</span>
                      <span className="text-sm font-semibold text-foreground">
                        {selectedUser.node_count || 0}{selectedUser.maxNodeCount ? ` / ${selectedUser.maxNodeCount}` : ''} 个
                      </span>
                    </div>
                    <div className="flex justify-between items-center">
                      <span className="text-sm text-muted-foreground">客户端数量</span>
                      <span className="text-sm font-semibold text-foreground">
                        {selectedUser.currentClientCount || 0}{selectedUser.maxClientCount ? ` / ${selectedUser.maxClientCount}` : ''} 个
                      </span>
                    </div>
                  </div>
                </div>
                <div className="space-y-3">
                  <div>
                    <label className="block text-sm font-medium text-foreground mb-1.5">最大端口数量</label>
                    <input
                      type="number"
                      min="0"
                      value={portLimitData.maxPortCount}
                      onChange={(e) => setPortLimitData({ ...portLimitData, maxPortCount: e.target.value })}
                      placeholder="留空表示无限制"
                      className="w-full px-4 py-2.5 border border-border rounded-xl text-foreground placeholder-muted-foreground focus:outline-none focus:ring-2 focus:ring-primary/20 focus:border-primary transition-all bg-muted/50 hover:bg-card"
                    />
                  </div>
                  <div>
                    <label className="block text-sm font-medium text-foreground mb-1.5">最大节点数量</label>
                    <input
                      type="number"
                      min="0"
                      value={portLimitData.maxNodeCount}
                      onChange={(e) => setPortLimitData({ ...portLimitData, maxNodeCount: e.target.value })}
                      placeholder="留空表示无限制"
                      className="w-full px-4 py-2.5 border border-border rounded-xl text-foreground placeholder-muted-foreground focus:outline-none focus:ring-2 focus:ring-primary/20 focus:border-primary transition-all bg-muted/50 hover:bg-card"
                    />
                  </div>
                  <div>
                    <label className="block text-sm font-medium text-foreground mb-1.5">最大客户端数量</label>
                    <input
                      type="number"
                      min="0"
                      value={portLimitData.maxClientCount}
                      onChange={(e) => setPortLimitData({ ...portLimitData, maxClientCount: e.target.value })}
                      placeholder="留空表示无限制"
                      className="w-full px-4 py-2.5 border border-border rounded-xl text-foreground placeholder-muted-foreground focus:outline-none focus:ring-2 focus:ring-primary/20 focus:border-primary transition-all bg-muted/50 hover:bg-card"
                    />
                  </div>
                  <div>
                    <label className="block text-sm font-medium text-foreground mb-1.5">允许的端口范围</label>
                    <input
                      type="text"
                      value={portLimitData.allowedPortRange}
                      onChange={(e) => setPortLimitData({ ...portLimitData, allowedPortRange: e.target.value })}
                      placeholder="例如: 1000-9999,20000-30000"
                      className="w-full px-4 py-2.5 border border-border rounded-xl text-foreground placeholder-muted-foreground focus:outline-none focus:ring-2 focus:ring-primary/20 focus:border-primary transition-all bg-muted/50 hover:bg-card font-mono text-sm"
                    />
                    <p className="mt-1 text-xs text-muted-foreground">
                      格式: 单个端口 (8080) 或范围 (1000-9999)，多个用逗号分隔
                    </p>
                  </div>
                </div>
              </div>

              {/* 操作按钮 */}
              <div className="flex gap-3">
                <button
                  onClick={() => {
                    setShowQuotaModal(false);
                    setSelectedUser(null);
                    setQuotaChangeGb('');
                    setPortLimitData({ maxPortCount: '', allowedPortRange: '', maxNodeCount: '', maxClientCount: '' });
                  }}
                  className="flex-1 px-4 py-2.5 bg-muted text-foreground font-medium rounded-xl hover:bg-accent transition-colors"
                  disabled={portLimitSaving}
                >
                  取消
                </button>
                <button
                  onClick={handleSavePortLimit}
                  disabled={portLimitSaving}
                  className="flex-1 px-4 py-2.5 bg-primary text-primary-foreground font-medium rounded-xl hover:bg-primary/90 shadow-sm transition-all disabled:opacity-50 disabled:cursor-not-allowed"
                >
                  {portLimitSaving ? '保存中...' : '保存数量限制'}
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
        variant={confirmDialog.variant}
        confirmText={confirmDialog.confirmText}
        onConfirm={() => {
          confirmDialog.onConfirm();
          setConfirmDialog(prev => ({ ...prev, open: false }));
        }}
        onCancel={() => setConfirmDialog(prev => ({ ...prev, open: false }))}
      />
    </div>
  );
}
