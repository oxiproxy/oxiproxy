import { useState, useEffect } from 'react';
import { useAuth } from '../contexts/AuthContext';
import { systemService } from '../lib/services';
import { useToast } from '../contexts/ToastContext';
import ConfirmDialog from '../components/ConfirmDialog';
import SkeletonBlock from '../components/Skeleton';
import { Save, RotateCcw, Power, Info, AlertCircle, Server, Shield, Globe, Upload, X } from 'lucide-react';
import { Button } from '../components/ui/button';
import { Input } from '../components/ui/input';
import { Label } from '../components/ui/label';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '../components/ui/card';
import { Alert, AlertDescription } from '../components/ui/alert';

interface ConfigItem {
  id: number;
  key: string;
  value: number | string | boolean;
  description: string;
  valueType: 'number' | 'string' | 'boolean';
}

const configHints: Record<string, string> = {
  web_port: 'Web 管理界面的访问端口',
  internal_port: 'Node 和 Client 连接到 Controller 的 gRPC 端口',
  enable_registration: '开启后，任何人可以通过登录页面注册新账号',
  default_traffic_quota_gb: '创建新用户时自动分配的流量配额，0 表示不限制',
  default_max_port_count: '创建新用户时自动分配的最大端口数，0 表示不限制',
  default_max_node_count: '创建新用户时自动分配的最大节点数，0 表示不限制',
  default_max_client_count: '创建新用户时自动分配的最大客户端数，0 表示不限制',
  jwt_expiration_hours: 'JWT 令牌的有效期，过期后需要重新登录',
  db_path: '数据库文件的存储路径',
  grpc_tls_enabled: '启用后 gRPC 连接将使用 TLS 加密，可避免 GFW 干扰',
  grpc_tls_cert_path: 'TLS 证书文件的绝对路径（PEM 格式）',
  grpc_tls_key_path: 'TLS 私钥文件的绝对路径（PEM 格式）',
  grpc_tls_cert_content: 'TLS 证书内容（PEM 格式，可直接上传证书文件）',
  grpc_tls_key_content: 'TLS 私钥内容（PEM 格式，可直接上传私钥文件）',
  grpc_domain: 'gRPC 服务器域名（可选，用于 SNI）',
  web_tls_enabled: '启用后 Web 管理界面将使用 HTTPS 加密访问',
  web_tls_cert_path: 'Web TLS 证书文件的绝对路径（PEM 格式）',
  web_tls_key_path: 'Web TLS 私钥文件的绝对路径（PEM 格式）',
  web_tls_cert_content: 'Web TLS 证书内容（PEM 格式，可直接上传证书文件）',
  web_tls_key_content: 'Web TLS 私钥内容（PEM 格式，可直接上传私钥文件）',
};

// gRPC TLS 证书相关的 key
const GRPC_TLS_CERT_PATH_KEYS = ['grpc_tls_cert_path', 'grpc_tls_key_path'];
const GRPC_TLS_CERT_CONTENT_KEYS = ['grpc_tls_cert_content', 'grpc_tls_key_content'];
const GRPC_TLS_CERT_ALL_KEYS = [...GRPC_TLS_CERT_PATH_KEYS, ...GRPC_TLS_CERT_CONTENT_KEYS];

// Web TLS 证书相关的 key
const WEB_TLS_CERT_PATH_KEYS = ['web_tls_cert_path', 'web_tls_key_path'];
const WEB_TLS_CERT_CONTENT_KEYS = ['web_tls_cert_content', 'web_tls_key_content'];
const WEB_TLS_CERT_ALL_KEYS = [...WEB_TLS_CERT_PATH_KEYS, ...WEB_TLS_CERT_CONTENT_KEYS];

export default function Settings() {
  const [configs, setConfigs] = useState<ConfigItem[]>([]);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [restarting, setRestarting] = useState(false);
  const [editedValues, setEditedValues] = useState<Record<string, any>>({});
  const [grpcCertMode, setGrpcCertMode] = useState<'upload' | 'path'>('upload');
  const [webCertMode, setWebCertMode] = useState<'upload' | 'path'>('upload');
  const { showToast } = useToast();
  const { isAdmin } = useAuth();
  const [confirmDialog, setConfirmDialog] = useState<{ open: boolean; title: string; message: string; variant: 'danger' | 'warning' | 'info'; confirmText: string; onConfirm: () => void }>({ open: false, title: '', message: '', variant: 'warning', confirmText: '确定', onConfirm: () => {} });

  useEffect(() => {
    loadConfigs();
  }, []);

  const restartSystem = () => {
    setConfirmDialog({
      open: true,
      title: '重启系统',
      message: '确定要重启系统吗？重启期间服务将暂时不可用。',
      variant: 'warning',
      confirmText: '重启',
      onConfirm: async () => {
        setRestarting(true);
        try {
          const response = await systemService.restart();
          if (response.success) {
            showToast('系统正在重启，请稍候...', 'success');
            setTimeout(() => {
              window.location.reload();
            }, 5000);
          } else {
            showToast(response.message || '重启失败', 'error');
            setRestarting(false);
          }
        } catch (error) {
          showToast('网络错误，请稍后重试', 'error');
          setRestarting(false);
        }
      },
    });
  };

  const loadConfigs = async () => {
    try {
      const response = await systemService.getConfigs();
      if (response.success && response.data) {
        setConfigs(response.data.configs);
        const initialValues: Record<string, any> = {};
        response.data.configs.forEach(config => {
          initialValues[config.key] = config.value;
        });
        setEditedValues(initialValues);
        // 根据已有数据自动判断 gRPC 证书配置模式
        const hasGrpcPath = initialValues['grpc_tls_cert_path'] && initialValues['grpc_tls_cert_path'] !== '';
        const hasGrpcContent = initialValues['grpc_tls_cert_content'] && initialValues['grpc_tls_cert_content'] !== '';
        setGrpcCertMode(hasGrpcPath && !hasGrpcContent ? 'path' : 'upload');
        // 根据已有数据自动判断 Web 证书配置模式
        const hasWebPath = initialValues['web_tls_cert_path'] && initialValues['web_tls_cert_path'] !== '';
        const hasWebContent = initialValues['web_tls_cert_content'] && initialValues['web_tls_cert_content'] !== '';
        setWebCertMode(hasWebPath && !hasWebContent ? 'path' : 'upload');
      } else {
        showToast(response.message || '无法加载系统配置', 'error');
      }
    } catch (error) {
      showToast('网络错误，请稍后重试', 'error');
    } finally {
      setLoading(false);
    }
  };

  const handleValueChange = (key: string, value: any, valueType: string) => {
    let parsedValue = value;

    if (valueType === 'number') {
      parsedValue = value === '' ? 0 : Number(value);
    } else if (valueType === 'boolean') {
      parsedValue = value === 'true' || value === true;
    }

    setEditedValues(prev => ({
      ...prev,
      [key]: parsedValue,
    }));
  };

  const valuesEqual = (a: any, b: any, valueType: string): boolean => {
    if (valueType === 'boolean') {
      const aBool = a === true || a === 'true';
      const bBool = b === true || b === 'true';
      return aBool === bBool;
    }
    if (valueType === 'number') {
      return Number(a) === Number(b);
    }
    return a === b;
  };

  const handleSave = async () => {
    setSaving(true);
    try {
      const updates = configs
        .filter(config => !valuesEqual(editedValues[config.key], config.value, config.valueType))
        .map(config => ({
          key: config.key,
          value: editedValues[config.key],
        }));

      if (updates.length === 0) {
        showToast('没有需要保存的更改', 'success');
        setSaving(false);
        return;
      }

      const response = await systemService.batchUpdateConfigs(updates);

      if (response.success) {
        showToast(`已成功更新 ${updates.length} 个配置项`, 'success');
        loadConfigs();
      } else {
        showToast(response.message || '无法保存配置', 'error');
      }
    } catch (error) {
      showToast('网络错误，请稍后重试', 'error');
    } finally {
      setSaving(false);
    }
  };

  const handleReset = () => {
    const initialValues: Record<string, any> = {};
    configs.forEach(config => {
      initialValues[config.key] = config.value;
    });
    setEditedValues(initialValues);
    showToast('所有更改已撤销', 'success');
  };

  const hasChanges = configs.some(config => !valuesEqual(editedValues[config.key], config.value, config.valueType));

  const renderConfigInput = (config: ConfigItem) => {
    const value = editedValues[config.key];

    // 文件上传组件（用于证书内容）
    if (config.key === 'grpc_tls_cert_content' || config.key === 'grpc_tls_key_content' ||
        config.key === 'web_tls_cert_content' || config.key === 'web_tls_key_content') {
      const handleFileUpload = async (e: React.ChangeEvent<HTMLInputElement>) => {
        const file = e.target.files?.[0];
        if (!file) return;

        try {
          const text = await file.text();
          // 将文件内容转换为 base64
          const base64 = btoa(text);
          handleValueChange(config.key, base64, config.valueType);
          const certType = config.key.includes('cert_content') ? '证书' : '私钥';
          showToast(`${certType}文件已加载`, 'success');
        } catch (error) {
          showToast('文件读取失败', 'error');
        }
      };

      const hasContent = value && value !== '';

      return (
        <div className="space-y-2">
          <div className="flex items-center gap-3">
            <Button variant="outline" size="sm" asChild>
              <label className="cursor-pointer">
                <input
                  type="file"
                  accept=".pem,.crt,.key"
                  onChange={handleFileUpload}
                  className="hidden"
                />
                <Upload className="w-4 h-4 mr-2" />
                上传文件
              </label>
            </Button>
            {hasContent && (
              <span className="text-sm text-primary font-medium flex items-center gap-1">
                <span className="w-1.5 h-1.5 bg-primary rounded-full"></span>
                已上传
              </span>
            )}
          </div>
          {hasContent && (
            <Button
              variant="ghost"
              size="sm"
              onClick={() => handleValueChange(config.key, '', config.valueType)}
              className="h-8 text-destructive hover:text-destructive-foreground hover:bg-destructive"
            >
              <X className="w-3 h-3 mr-1" />
              清除内容
            </Button>
          )}
        </div>
      );
    }

    switch (config.valueType) {
      case 'number':
        return (
          <Input
            type="number"
            value={value ?? 0}
            onChange={(e) => handleValueChange(config.key, e.target.value, config.valueType)}
            className="max-w-xs"
          />
        );

      case 'boolean':
        return (
          <select
            value={value === true || value === 'true' ? 'true' : 'false'}
            onChange={(e) => handleValueChange(config.key, e.target.value, config.valueType)}
            className="flex h-10 w-full max-w-xs rounded-md border border-input bg-background px-3 py-2 text-sm ring-offset-background focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2"
          >
            <option value="true">启用</option>
            <option value="false">禁用</option>
          </select>
        );

      case 'string':
      default:
        return (
          <Input
            type="text"
            value={value || ''}
            onChange={(e) => handleValueChange(config.key, e.target.value, config.valueType)}
            className="max-w-xs"
          />
        );
    }
  };

  if (loading) {
    return (
      <div className="space-y-6">
        <div className="flex items-center justify-between">
          <div className="space-y-2">
            <SkeletonBlock className="h-8 w-32" />
            <SkeletonBlock className="h-4 w-48" />
          </div>
          <div className="flex gap-3">
            <SkeletonBlock className="h-10 w-28 rounded-xl" />
            <SkeletonBlock className="h-10 w-24 rounded-xl" />
            <SkeletonBlock className="h-10 w-28 rounded-xl" />
          </div>
        </div>
        <Card>
          <CardContent className="pt-6">
            <SkeletonBlock className="h-6 w-24 mb-6" />
            <div className="space-y-6">
              {Array.from({ length: 4 }).map((_, i) => (
                <div key={i} className="space-y-2">
                  <SkeletonBlock className="h-4 w-40" />
                  <SkeletonBlock className="h-12 w-80 rounded-xl" />
                  <SkeletonBlock className="h-3 w-56" />
                </div>
              ))}
            </div>
          </CardContent>
        </Card>
      </div>
    );
  }

  return (
    <div className="space-y-6">
      {/* 页面标题和操作按钮 */}
      <div className="flex items-center justify-between">
        <div>
          <h2 className="text-2xl font-bold text-foreground">系统配置</h2>
          <p className="mt-1 text-sm text-muted-foreground">管理 OxiProxy Controller 的运行参数</p>
        </div>
        <div className="flex gap-3">
          {isAdmin && (
            <Button
              onClick={restartSystem}
              disabled={restarting || saving}
              variant="destructive"
            >
              <Power className="w-4 h-4" />
              {restarting ? '重启中...' : '重启系统'}
            </Button>
          )}
          <Button
            onClick={handleReset}
            disabled={!hasChanges || saving}
            variant="outline"
          >
            <RotateCcw className="w-4 h-4" />
            重置
          </Button>
          <Button
            onClick={handleSave}
            disabled={!hasChanges || saving}
            className="text-primary-foreground border-0"
            style={{ background: 'linear-gradient(135deg, hsl(210 100% 45%), hsl(189 94% 43%))' }}
          >
            <Save className="w-4 h-4" />
            {saving ? '保存中...' : '保存更改'}
          </Button>
        </div>
      </div>

      {/* 未保存提示 */}
      {hasChanges && (
        <Alert className="border" style={{ background: 'hsl(38 92% 50% / 0.08)', borderColor: 'hsl(38 92% 50% / 0.3)' }}>
          <AlertCircle className="w-4 h-4" style={{ color: 'hsl(38 92% 50%)' }} />
          <AlertDescription className="ml-2" style={{ color: 'hsl(38 92% 50%)' }}>
            你有未保存的更改（修改后需要重启服务端生效）
          </AlertDescription>
        </Alert>
      )}

      {/* 基础配置 */}
      <Card>
        <CardHeader>
          <div className="flex items-center gap-3">
            <div className="w-10 h-10 rounded-lg flex items-center justify-center" style={{ background: 'hsl(217 91% 60% / 0.15)' }}>
              <Server className="w-5 h-5" style={{ color: 'hsl(217 91% 60%)' }} />
            </div>
            <div>
              <CardTitle className="text-lg">基础配置</CardTitle>
              <CardDescription>端口、认证和数据库配置</CardDescription>
            </div>
          </div>
        </CardHeader>
        <CardContent>
          <div className="grid grid-cols-1 md:grid-cols-2 gap-6">
            {configs.filter(c => ['web_port', 'internal_port', 'enable_registration', 'default_traffic_quota_gb', 'default_max_port_count', 'default_max_node_count', 'default_max_client_count'].includes(c.key)).map((config) => (
              <div key={config.key} className="space-y-2">
                <Label className="text-foreground">
                  {config.description}
                </Label>
                <div className="flex items-center gap-3">
                  {renderConfigInput(config)}
                  {config.valueType === 'number' && config.key === 'default_traffic_quota_gb' && (
                    <span className="text-sm text-muted-foreground">GB</span>
                  )}
                  {config.valueType === 'number' && config.key.includes('hours') && (
                    <span className="text-sm text-muted-foreground">小时</span>
                  )}
                </div>
                {configHints[config.key] && (
                  <div className="flex items-center gap-1.5">
                    <Info className="w-3.5 h-3.5 text-muted-foreground flex-shrink-0" />
                    <p className="text-sm text-muted-foreground">{configHints[config.key]}</p>
                  </div>
                )}
              </div>
            ))}
          </div>
        </CardContent>
      </Card>

      {/* gRPC TLS 配置 */}
      {configs.some(c => c.key === 'grpc_tls_enabled' || GRPC_TLS_CERT_ALL_KEYS.includes(c.key) || c.key === 'grpc_domain') && (
        <Card>
          <CardHeader>
            <div className="flex items-center gap-3">
              <div className="w-10 h-10 rounded-lg flex items-center justify-center" style={{ background: 'hsl(142 71% 45% / 0.15)' }}>
                <Shield className="w-5 h-5" style={{ color: 'hsl(142 71% 45%)' }} />
              </div>
              <div>
                <CardTitle className="text-lg">gRPC TLS 配置</CardTitle>
                <CardDescription>Node 和 Client 连接加密配置</CardDescription>
              </div>
            </div>
          </CardHeader>
          <CardContent className="space-y-6">
            {/* TLS 开关和域名 */}
            <div className="grid grid-cols-1 md:grid-cols-2 gap-6">
              {configs.filter(c => c.key === 'grpc_tls_enabled' || c.key === 'grpc_domain').map((config) => (
                <div key={config.key} className="space-y-2">
                  <Label className="text-foreground">
                    {config.description}
                  </Label>
                  {renderConfigInput(config)}
                  {configHints[config.key] && (
                    <div className="flex items-center gap-1.5">
                      <Info className="w-3.5 h-3.5 text-muted-foreground flex-shrink-0" />
                      <p className="text-sm text-muted-foreground">{configHints[config.key]}</p>
                    </div>
                  )}
                </div>
              ))}
            </div>

            {/* 证书配置 */}
            {configs.some(c => GRPC_TLS_CERT_ALL_KEYS.includes(c.key)) && (
              <div className="border-t border-border pt-6">
                <div className="flex items-center justify-between mb-4">
                  <Label className="text-foreground">证书配置方式</Label>
                  <div className="inline-flex rounded-lg border border-border p-1 bg-muted">
                    <button
                      type="button"
                      onClick={() => {
                        setGrpcCertMode('upload');
                        GRPC_TLS_CERT_PATH_KEYS.forEach(k => handleValueChange(k, '', 'string'));
                      }}
                      className={`px-3 py-1.5 text-sm font-medium rounded-md transition-all ${grpcCertMode === 'upload' ? 'bg-card text-foreground shadow-sm' : 'text-muted-foreground hover:text-foreground'}`}
                    >
                      上传文件
                    </button>
                    <button
                      type="button"
                      onClick={() => {
                        setGrpcCertMode('path');
                        GRPC_TLS_CERT_CONTENT_KEYS.forEach(k => handleValueChange(k, '', 'string'));
                      }}
                      className={`px-3 py-1.5 text-sm font-medium rounded-md transition-all ${grpcCertMode === 'path' ? 'bg-card text-foreground shadow-sm' : 'text-muted-foreground hover:text-foreground'}`}
                    >
                      指定路径
                    </button>
                  </div>
                </div>

                <div className="grid grid-cols-1 md:grid-cols-2 gap-6">
                  {configs
                    .filter(c => grpcCertMode === 'upload' ? GRPC_TLS_CERT_CONTENT_KEYS.includes(c.key) : GRPC_TLS_CERT_PATH_KEYS.includes(c.key))
                    .map((config) => (
                      <div key={config.key} className="space-y-2">
                        <Label className="text-foreground">
                          {config.description}
                        </Label>
                        {renderConfigInput(config)}
                        {configHints[config.key] && (
                          <div className="flex items-center gap-1.5">
                            <Info className="w-3.5 h-3.5 text-muted-foreground flex-shrink-0" />
                            <p className="text-sm text-muted-foreground">{configHints[config.key]}</p>
                          </div>
                        )}
                      </div>
                    ))
                  }
                </div>
              </div>
            )}
          </CardContent>
        </Card>
      )}

      {/* Web TLS 配置 */}
      {configs.some(c => c.key === 'web_tls_enabled' || WEB_TLS_CERT_ALL_KEYS.includes(c.key)) && (
        <Card>
          <CardHeader>
            <div className="flex items-center gap-3">
              <div className="w-10 h-10 rounded-lg flex items-center justify-center" style={{ background: 'hsl(189 94% 43% / 0.15)' }}>
                <Globe className="w-5 h-5" style={{ color: 'hsl(189 94% 43%)' }} />
              </div>
              <div>
                <CardTitle className="text-lg">Web TLS 配置</CardTitle>
                <CardDescription>管理界面 HTTPS 加密配置</CardDescription>
              </div>
            </div>
          </CardHeader>
          <CardContent className="space-y-6">
            {/* TLS 开关 */}
            {configs.filter(c => c.key === 'web_tls_enabled').map((config) => (
              <div key={config.key} className="space-y-2">
                <Label className="text-foreground">
                  {config.description}
                </Label>
                {renderConfigInput(config)}
                {configHints[config.key] && (
                  <div className="flex items-center gap-1.5">
                    <Info className="w-3.5 h-3.5 text-muted-foreground flex-shrink-0" />
                    <p className="text-sm text-muted-foreground">{configHints[config.key]}</p>
                  </div>
                )}
              </div>
            ))}

            {/* 证书配置 */}
            {configs.some(c => WEB_TLS_CERT_ALL_KEYS.includes(c.key)) && (
              <div className="border-t border-border pt-6">
                <div className="flex items-center justify-between mb-4">
                  <Label className="text-foreground">证书配置方式</Label>
                  <div className="inline-flex rounded-lg border border-border p-1 bg-muted">
                    <button
                      type="button"
                      onClick={() => {
                        setWebCertMode('upload');
                        WEB_TLS_CERT_PATH_KEYS.forEach(k => handleValueChange(k, '', 'string'));
                      }}
                      className={`px-3 py-1.5 text-sm font-medium rounded-md transition-all ${webCertMode === 'upload' ? 'bg-card text-foreground shadow-sm' : 'text-muted-foreground hover:text-foreground'}`}
                    >
                      上传文件
                    </button>
                    <button
                      type="button"
                      onClick={() => {
                        setWebCertMode('path');
                        WEB_TLS_CERT_CONTENT_KEYS.forEach(k => handleValueChange(k, '', 'string'));
                      }}
                      className={`px-3 py-1.5 text-sm font-medium rounded-md transition-all ${webCertMode === 'path' ? 'bg-card text-foreground shadow-sm' : 'text-muted-foreground hover:text-foreground'}`}
                    >
                      指定路径
                    </button>
                  </div>
                </div>

                <div className="grid grid-cols-1 md:grid-cols-2 gap-6">
                  {configs
                    .filter(c => webCertMode === 'upload' ? WEB_TLS_CERT_CONTENT_KEYS.includes(c.key) : WEB_TLS_CERT_PATH_KEYS.includes(c.key))
                    .map((config) => (
                      <div key={config.key} className="space-y-2">
                        <Label className="text-foreground">
                          {config.description}
                        </Label>
                        {renderConfigInput(config)}
                        {configHints[config.key] && (
                          <div className="flex items-center gap-1.5">
                            <Info className="w-3.5 h-3.5 text-muted-foreground flex-shrink-0" />
                            <p className="text-sm text-muted-foreground">{configHints[config.key]}</p>
                          </div>
                        )}
                      </div>
                    ))
                  }
                </div>
              </div>
            )}
          </CardContent>
        </Card>
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
