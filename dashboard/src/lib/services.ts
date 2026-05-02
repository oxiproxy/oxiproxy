import api from './api';
import type {
  ApiResponse,
  UserWithNodeCount,
  Client,
  ClientTrafficInfo,
  Proxy,
  TrafficOverview,
  DashboardStats,
  LoginRequest,
  LoginResponse,
  LogEntry,
  Node,
  Subscription,
  UserSubscription,
  LatestVersionInfo,
  BatchUpdateResult,
} from './types';

// ============ 认证服务 ============
export const authService = {
  async login(data: LoginRequest): Promise<ApiResponse<LoginResponse>> {
    const response = await api.post<ApiResponse<LoginResponse>>('/auth/login', data);
    return response.data;
  },

  async register(data: { username: string; password: string }): Promise<ApiResponse<LoginResponse>> {
    const response = await api.post<ApiResponse<LoginResponse>>('/auth/register', data);
    return response.data;
  },

  async getRegisterStatus(): Promise<ApiResponse<{ enabled: boolean }>> {
    const response = await api.get<ApiResponse<{ enabled: boolean }>>('/auth/register-status');
    return response.data;
  },
};

// ============ 用户服务 ============
export const userService = {
  async getUsers(): Promise<ApiResponse<UserWithNodeCount[]>> {
    const response = await api.get<ApiResponse<UserWithNodeCount[]>>('/users');
    return response.data;
  },

  async createUser(data: {
    username: string;
    password?: string;
    is_admin?: boolean;
    traffic_quota_gb?: number | null;
  }): Promise<ApiResponse<any>> {
    const response = await api.post<ApiResponse<any>>('/users', data);
    return response.data;
  },

  async updateUser(
    id: number,
    data: {
      username?: string;
      password?: string;
      is_admin?: boolean;
      upload_limit_gb?: number | null;
      download_limit_gb?: number | null;
      traffic_quota_gb?: number | null;
      traffic_reset_cycle?: string;
      is_traffic_exceeded?: boolean;
      max_port_count?: number | null;
      allowed_port_range?: string | null;
      max_node_count?: number | null;
      max_client_count?: number | null;
    }
  ): Promise<ApiResponse<any>> {
    const response = await api.put<ApiResponse<any>>(`/users/${id}`, data);
    return response.data;
  },

  async deleteUser(id: number): Promise<ApiResponse<string>> {
    const response = await api.delete<ApiResponse<string>>(`/users/${id}`);
    return response.data;
  },

  async getUserNodes(id: number): Promise<ApiResponse<Node[]>> {
    const response = await api.get<ApiResponse<Node[]>>(`/users/${id}/nodes`);
    return response.data;
  },

  async assignNode(userId: number, nodeId: number): Promise<ApiResponse<string>> {
    const response = await api.post<ApiResponse<string>>(`/users/${userId}/nodes/${nodeId}`);
    return response.data;
  },

  async removeNode(userId: number, nodeId: number): Promise<ApiResponse<string>> {
    const response = await api.delete<ApiResponse<string>>(`/users/${userId}/nodes/${nodeId}`);
    return response.data;
  },

  async adjustQuota(userId: number, quotaChangeGb: number): Promise<ApiResponse<string>> {
    const response = await api.post<ApiResponse<string>>(`/users/${userId}/adjust-quota`, {
      quota_change_gb: quotaChangeGb,
    });
    return response.data;
  },

  async getQuotaInfo(userId: number): Promise<ApiResponse<any>> {
    const response = await api.get<ApiResponse<any>>(`/users/${userId}/quota-info`);
    return response.data;
  },
};

// ============ 客户端服务 ============
export const clientService = {
  async getClients(): Promise<ApiResponse<Client[]>> {
    const response = await api.get<ApiResponse<Client[]>>('/clients');
    return response.data;
  },

  async getClient(id: number): Promise<ApiResponse<Client>> {
    const response = await api.get<ApiResponse<Client>>(`/clients/${id}`);
    return response.data;
  },

  async createClient(data: { name: string; token?: string; region?: string }): Promise<ApiResponse<Client>> {
    const response = await api.post<ApiResponse<Client>>('/clients', data);
    return response.data;
  },

  async deleteClient(id: number): Promise<ApiResponse<string>> {
    const response = await api.delete<ApiResponse<string>>(`/clients/${id}`);
    return response.data;
  },

  async getClientLogs(id: number): Promise<ApiResponse<LogEntry[]>> {
    const response = await api.get<ApiResponse<LogEntry[]>>(`/clients/${id}/logs`);
    return response.data;
  },

  async updateClient(
    id: number,
    data: {
      name?: string;
      upload_limit_gb?: number | null;
      download_limit_gb?: number | null;
      traffic_quota_gb?: number | null;
      traffic_reset_cycle?: string;
      is_traffic_exceeded?: boolean;
    }
  ): Promise<ApiResponse<Client>> {
    const response = await api.put<ApiResponse<Client>>(`/clients/${id}`, data);
    return response.data;
  },

  async allocateQuota(id: number, quotaGb: number): Promise<ApiResponse<string>> {
    const response = await api.post<ApiResponse<string>>(`/clients/${id}/allocate-quota`, {
      quota_gb: quotaGb,
    });
    return response.data;
  },

  async getClientTraffic(id: number): Promise<ApiResponse<ClientTrafficInfo>> {
    const response = await api.get<ApiResponse<ClientTrafficInfo>>(`/clients/${id}/traffic`);
    return response.data;
  },

  async triggerUpdate(id: number): Promise<ApiResponse<{ success: boolean; error?: string; newVersion?: string }>> {
    const response = await api.post<ApiResponse<any>>(`/clients/${id}/update`);
    return response.data;
  },

  async triggerRestart(id: number): Promise<ApiResponse<{ message: string }>> {
    const response = await api.post<ApiResponse<any>>(`/clients/${id}/restart`);
    return response.data;
  },

  async batchUpdate(): Promise<ApiResponse<{ results: BatchUpdateResult[] }>> {
    const response = await api.post<ApiResponse<any>>('/clients/batch-update');
    return response.data;
  },
};

// ============ 代理服务 ============
export const proxyService = {
  async getProxies(): Promise<ApiResponse<Proxy[]>> {
    const response = await api.get<ApiResponse<Proxy[]>>('/proxies');
    return response.data;
  },

  async getProxiesByClient(clientId: number): Promise<ApiResponse<Proxy[]>> {
    const response = await api.get<ApiResponse<Proxy[]>>(`/clients/${clientId}/proxies`);
    return response.data;
  },

  async createProxy(data: {
    client_id: string;
    name: string;
    type: string;
    localIP: string;
    localPort: number;
    remotePort: number;
    nodeId?: number;
  }): Promise<ApiResponse<Proxy>> {
    const response = await api.post<ApiResponse<Proxy>>('/proxies', data);
    return response.data;
  },

  async updateProxy(
    id: number,
    data: {
      name?: string;
      type?: string;
      localIP?: string;
      localPort?: number;
      remotePort?: number;
      enabled?: boolean;
    }
  ): Promise<ApiResponse<Proxy>> {
    const response = await api.put<ApiResponse<Proxy>>(`/proxies/${id}`, data);
    return response.data;
  },

  async deleteProxy(id: number): Promise<ApiResponse<string>> {
    const response = await api.delete<ApiResponse<string>>(`/proxies/${id}`);
    return response.data;
  },

  async batchCreateProxies(data: {
    client_id: string;
    name: string;
    type: string;
    localIP: string;
    localPorts: number[];
    remotePorts: number[];
    nodeId?: number;
  }): Promise<ApiResponse<Proxy[]>> {
    const response = await api.post<ApiResponse<Proxy[]>>('/proxies/batch', data);
    return response.data;
  },

  async deleteProxyGroup(groupId: string): Promise<ApiResponse<string>> {
    const response = await api.delete<ApiResponse<string>>(`/proxies/group/${groupId}`);
    return response.data;
  },

  async toggleProxyGroup(groupId: string, enabled: boolean): Promise<ApiResponse<string>> {
    const response = await api.post<ApiResponse<string>>(`/proxies/group/${groupId}/toggle`, { enabled });
    return response.data;
  },

  async updateProxyGroup(groupId: string, data: {
    name?: string;
    type?: string;
    localIP?: string;
    localPort?: number;
  }): Promise<ApiResponse<string>> {
    const response = await api.put<ApiResponse<string>>(`/proxies/group/${groupId}`, data);
    return response.data;
  },

  async addPortsToProxyGroup(groupId: string, data: {
    localPorts: number[];
    remotePorts: number[];
  }): Promise<ApiResponse<Proxy[]>> {
    const response = await api.post<ApiResponse<Proxy[]>>(`/proxies/group/${groupId}/proxies`, data);
    return response.data;
  },
};

// ============ 流量服务 ============
export const trafficService = {
  async getTrafficOverview(days?: number): Promise<ApiResponse<TrafficOverview>> {
    const response = await api.get<ApiResponse<TrafficOverview>>('/traffic/overview', {
      params: { days },
    });
    return response.data;
  },

  async getUserTraffic(userId: number, days?: number): Promise<ApiResponse<TrafficOverview>> {
    const response = await api.get<ApiResponse<TrafficOverview>>(`/traffic/users/${userId}`, {
      params: { days },
    });
    return response.data;
  },
};

// ============ Dashboard 服务 ============
export const dashboardService = {
  async getDashboardStats(userId: number): Promise<ApiResponse<DashboardStats>> {
    const response = await api.get<ApiResponse<DashboardStats>>(`/dashboard/stats/${userId}`);
    return response.data;
  },
};

// ============ 系统配置服务 ============
export const systemService = {
  async getConfigs(): Promise<ApiResponse<{ configs: Array<{ id: number; key: string; value: number | string | boolean; description: string; valueType: 'number' | 'string' | 'boolean' }> }>> {
    const response = await api.get('/system/configs');
    return response.data;
  },

  async batchUpdateConfigs(configs: Array<{ key: string; value: any }>): Promise<ApiResponse<any>> {
    const response = await api.post('/system/configs/batch', { configs });
    return response.data;
  },

  async restart(): Promise<ApiResponse<{ message: string }>> {
    const response = await api.post('/system/restart');
    return response.data;
  },

  async getLatestVersion(): Promise<ApiResponse<LatestVersionInfo>> {
    const response = await api.get<ApiResponse<LatestVersionInfo>>('/system/latest-version');
    return response.data;
  },

  async getGrpcTlsStatus(): Promise<{ enabled: boolean; domain: string }> {
    const response = await api.get('/system/configs');
    const data = response.data as ApiResponse<{ configs: Array<{ key: string; value: any }> }>;
    let enabled = false;
    let domain = '';
    if (data.success && data.data?.configs) {
      for (const c of data.data.configs) {
        if (c.key === 'grpc_tls_enabled') enabled = c.value === true || c.value === 'true';
        if (c.key === 'grpc_domain') domain = String(c.value || '');
      }
    }
    return { enabled, domain };
  },
};

// ============ 节点服务 ============
export const nodeService = {
  async getNodes(): Promise<ApiResponse<Node[]>> {
    const response = await api.get<ApiResponse<Node[]>>('/nodes');
    return response.data;
  },

  async getNode(id: number): Promise<ApiResponse<Node>> {
    const response = await api.get<ApiResponse<Node>>(`/nodes/${id}`);
    return response.data;
  },

  async createNode(data: {
    name: string;
    url: string;
    secret?: string;
    region?: string;
    description?: string;
    tunnelAddr?: string;
    tunnelPort?: number;
    tunnelProtocol?: string;
    kcpConfig?: string;
    nodeType?: string;
    maxProxyCount?: number | null;
    allowedPortRange?: string | null;
    trafficQuotaGb?: number | null;
    trafficResetCycle?: string;
    speedLimit?: number | null;
  }): Promise<ApiResponse<Node>> {
    const response = await api.post<ApiResponse<Node>>('/nodes', data);
    return response.data;
  },

  async updateNode(
    id: number,
    data: {
      name?: string;
      url?: string;
      secret?: string;
      region?: string;
      description?: string;
      tunnelAddr?: string;
      tunnelPort?: number;
      tunnelProtocol?: string;
      kcpConfig?: string;
      nodeType?: string;
      maxProxyCount?: number | null;
      allowedPortRange?: string | null;
      trafficQuotaGb?: number | null;
      trafficResetCycle?: string;
      speedLimit?: number | null;
    }
  ): Promise<ApiResponse<Node>> {
    const response = await api.put<ApiResponse<Node>>(`/nodes/${id}`, data);
    return response.data;
  },

  async deleteNode(id: number): Promise<ApiResponse<string>> {
    const response = await api.delete<ApiResponse<string>>(`/nodes/${id}`);
    return response.data;
  },

  async testConnection(id: number): Promise<ApiResponse<any>> {
    const response = await api.post<ApiResponse<any>>(`/nodes/${id}/test`);
    return response.data;
  },

  async getNodeStatus(id: number): Promise<ApiResponse<any>> {
    const response = await api.get<ApiResponse<any>>(`/nodes/${id}/status`);
    return response.data;
  },

  async getNodeLogs(id: number, lines: number = 100): Promise<ApiResponse<{ node_id: number; node_name: string; logs: LogEntry[] }>> {
    const response = await api.get<ApiResponse<{ node_id: number; node_name: string; logs: LogEntry[] }>>(`/nodes/${id}/logs?lines=${lines}`);
    return response.data;
  },

  async triggerUpdate(id: number): Promise<ApiResponse<{ success: boolean; error?: string; newVersion?: string }>> {
    const response = await api.post<ApiResponse<any>>(`/nodes/${id}/update`);
    return response.data;
  },

  async triggerRestart(id: number): Promise<ApiResponse<{ message: string }>> {
    const response = await api.post<ApiResponse<any>>(`/nodes/${id}/restart`);
    return response.data;
  },

  async batchUpdate(): Promise<ApiResponse<{ results: BatchUpdateResult[] }>> {
    const response = await api.post<ApiResponse<any>>('/nodes/batch-update');
    return response.data;
  },
};

// ============ 订阅服务 ============
export const subscriptionService = {
  async getSubscriptions(): Promise<ApiResponse<Subscription[]>> {
    const response = await api.get<ApiResponse<Subscription[]>>('/subscriptions');
    return response.data;
  },

  async getActiveSubscriptions(): Promise<ApiResponse<Subscription[]>> {
    const response = await api.get<ApiResponse<Subscription[]>>('/subscriptions/active');
    return response.data;
  },

  async getSubscription(id: number): Promise<ApiResponse<Subscription>> {
    const response = await api.get<ApiResponse<Subscription>>(`/subscriptions/${id}`);
    return response.data;
  },

  async createSubscription(data: {
    name: string;
    duration_type: string;
    duration_value?: number;
    traffic_quota_gb: number;
    max_port_count?: number;
    max_node_count?: number;
    max_client_count?: number;
    price?: number;
    description?: string;
    is_active?: boolean;
  }): Promise<ApiResponse<Subscription>> {
    const response = await api.post<ApiResponse<Subscription>>('/subscriptions', data);
    return response.data;
  },

  async updateSubscription(
    id: number,
    data: {
      name?: string;
      duration_type?: string;
      duration_value?: number;
      traffic_quota_gb?: number;
      max_port_count?: number;
      max_node_count?: number;
      max_client_count?: number;
      price?: number;
      description?: string;
      is_active?: boolean;
    }
  ): Promise<ApiResponse<Subscription>> {
    const response = await api.put<ApiResponse<Subscription>>(`/subscriptions/${id}`, data);
    return response.data;
  },

  async deleteSubscription(id: number): Promise<ApiResponse<string>> {
    const response = await api.delete<ApiResponse<string>>(`/subscriptions/${id}`);
    return response.data;
  },
};

// ============ 用户订阅服务 ============
export const userSubscriptionService = {
  async getAllUserSubscriptions(): Promise<ApiResponse<UserSubscription[]>> {
    const response = await api.get<ApiResponse<UserSubscription[]>>('/user-subscriptions');
    return response.data;
  },

  async getUserSubscriptions(userId: number): Promise<ApiResponse<UserSubscription[]>> {
    const response = await api.get<ApiResponse<UserSubscription[]>>(`/users/${userId}/subscriptions`);
    return response.data;
  },

  async getUserActiveSubscription(userId: number): Promise<ApiResponse<UserSubscription | null>> {
    const response = await api.get<ApiResponse<UserSubscription | null>>(`/users/${userId}/subscriptions/active`);
    return response.data;
  },

  async createUserSubscription(data: {
    user_id: number;
    subscription_id: number;
    start_date?: string;
  }): Promise<ApiResponse<UserSubscription>> {
    const response = await api.post<ApiResponse<UserSubscription>>('/user-subscriptions', data);
    return response.data;
  },

  async updateUserSubscription(
    id: number,
    data: {
      is_active?: boolean;
      traffic_used_gb?: number;
    }
  ): Promise<ApiResponse<UserSubscription>> {
    const response = await api.put<ApiResponse<UserSubscription>>(`/user-subscriptions/${id}`, data);
    return response.data;
  },

  async deleteUserSubscription(id: number): Promise<ApiResponse<string>> {
    const response = await api.delete<ApiResponse<string>>(`/user-subscriptions/${id}`);
    return response.data;
  },
};
