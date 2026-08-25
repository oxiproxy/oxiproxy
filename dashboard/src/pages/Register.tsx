import { useEffect, useRef, useState } from 'react';
import { Link, useNavigate } from 'react-router-dom';
import { AlertCircle, ArrowRight, CheckCircle2, Eye, EyeOff, Lock, Loader2, User } from 'lucide-react';
import { useAuth } from '../contexts/AuthContext';
import { authService } from '../lib/services';
import AuthShell from '../components/AuthShell';
import { Alert, AlertDescription } from '../components/ui/alert';
import { Button } from '../components/ui/button';
import { Input } from '../components/ui/input';
import { Label } from '../components/ui/label';

export default function Register() {
  const navigate = useNavigate();
  const { login } = useAuth();
  const usernameRef = useRef<HTMLInputElement>(null);
  const [username, setUsername] = useState('');
  const [password, setPassword] = useState('');
  const [confirmPassword, setConfirmPassword] = useState('');
  const [showPassword, setShowPassword] = useState(false);
  const [error, setError] = useState('');
  const [loading, setLoading] = useState(false);
  const [registrationEnabled, setRegistrationEnabled] = useState<boolean | null>(null);

  useEffect(() => {
    authService.getRegisterStatus().then((response) => {
      setRegistrationEnabled(response.success && response.data ? response.data.enabled : false);
    }).catch(() => setRegistrationEnabled(false));
  }, []);

  useEffect(() => {
    if (registrationEnabled === true) {
      const timer = window.setTimeout(() => usernameRef.current?.focus(), 250);
      return () => window.clearTimeout(timer);
    }
  }, [registrationEnabled]);

  const handleSubmit = async (event: React.FormEvent) => {
    event.preventDefault();
    setError('');
    const trimmedUsername = username.trim();
    if (trimmedUsername.length < 3 || trimmedUsername.length > 20) {
      setError('用户名长度需要 3-20 个字符');
      return;
    }
    if (password.length < 6) {
      setError('密码长度不能少于 6 个字符');
      return;
    }
    if (password !== confirmPassword) {
      setError('两次输入的密码不一致');
      return;
    }

    setLoading(true);
    try {
      const response = await authService.register({ username: trimmedUsername, password });
      if (response.success && response.data) {
        const { token, user } = response.data;
        login(token, {
          id: user.id,
          username: user.username,
          is_admin: user.is_admin,
          created_at: '',
          updated_at: '',
          totalBytesSent: 0,
          totalBytesReceived: 0,
          trafficQuotaGb: null,
          remainingQuotaGb: null,
          trafficResetCycle: 'none',
          lastResetAt: null,
          isTrafficExceeded: false,
          maxPortCount: null,
          allowedPortRange: null,
          maxNodeCount: null,
          maxClientCount: null,
        });
        navigate('/');
      } else {
        setError(response.message || '注册失败');
      }
    } catch (err) {
      console.error('注册错误:', err);
      setError('注册失败，请稍后重试');
    } finally {
      setLoading(false);
    }
  };

  return (
    <AuthShell
      title="创建账号"
      description="注册一个新账号以开始使用"
      footer={<p className="text-sm text-muted-foreground">已有账号？ <Link to="/login" className="font-medium text-primary underline-offset-4 hover:underline">返回登录</Link></p>}
    >
      {registrationEnabled === null ? (
        <div className="flex items-center justify-center py-12 text-muted-foreground"><Loader2 className="size-5 animate-spin" /><span className="ml-2 text-sm">检查注册状态...</span></div>
      ) : registrationEnabled === false ? (
        <Alert className="mb-6 flex items-start gap-3 border-amber-500/30 bg-amber-500/10 text-amber-800 dark:text-amber-300">
          <AlertCircle className="mt-0.5 size-4 shrink-0" />
          <AlertDescription>注册功能暂未开放，请联系管理员</AlertDescription>
        </Alert>
      ) : (
        <form className="space-y-5 pb-6" onSubmit={handleSubmit}>
          {error && <Alert variant="destructive" className="flex items-start gap-3"><AlertCircle className="mt-0.5 size-4 shrink-0" /><AlertDescription>{error}</AlertDescription></Alert>}

          <div className="space-y-2">
            <Label htmlFor="username">用户名</Label>
            <div className="relative"><User className="pointer-events-none absolute left-3 top-1/2 size-4 -translate-y-1/2 text-muted-foreground" aria-hidden="true" /><Input ref={usernameRef} id="username" name="username" type="text" required value={username} onChange={(event) => setUsername(event.target.value)} className="pl-9" placeholder="3-20 个字符" disabled={loading} autoComplete="username" /></div>
          </div>

          <div className="space-y-2">
            <Label htmlFor="password">密码</Label>
            <div className="relative"><Lock className="pointer-events-none absolute left-3 top-1/2 size-4 -translate-y-1/2 text-muted-foreground" aria-hidden="true" /><Input id="password" name="password" type={showPassword ? 'text' : 'password'} required value={password} onChange={(event) => setPassword(event.target.value)} className="pl-9 pr-11" placeholder="至少 6 个字符" disabled={loading} autoComplete="new-password" /><Button type="button" variant="ghost" size="icon" className="absolute right-1 top-1/2 size-8 -translate-y-1/2 text-muted-foreground" onClick={() => setShowPassword((visible) => !visible)} disabled={loading} aria-label={showPassword ? '隐藏密码' : '显示密码'}>{showPassword ? <EyeOff className="size-4" /> : <Eye className="size-4" />}</Button></div>
          </div>

          <div className="space-y-2">
            <Label htmlFor="confirmPassword">确认密码</Label>
            <div className="relative"><CheckCircle2 className="pointer-events-none absolute left-3 top-1/2 size-4 -translate-y-1/2 text-muted-foreground" aria-hidden="true" /><Input id="confirmPassword" name="confirmPassword" type={showPassword ? 'text' : 'password'} required value={confirmPassword} onChange={(event) => setConfirmPassword(event.target.value)} className="pl-9" placeholder="再次输入密码" disabled={loading} autoComplete="new-password" /></div>
          </div>

          <Button type="submit" disabled={loading} className="w-full">{loading ? <><Loader2 className="size-4 animate-spin" />注册中...</> : <>注册<ArrowRight className="size-4" /></>}</Button>
        </form>
      )}
    </AuthShell>
  );
}
