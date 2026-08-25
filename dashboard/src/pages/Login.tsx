import { useEffect, useRef, useState } from 'react';
import { Link, useNavigate } from 'react-router-dom';
import { AlertCircle, ArrowRight, Eye, EyeOff, Lock, Loader2, User } from 'lucide-react';
import { useAuth } from '../contexts/AuthContext';
import { authService } from '../lib/services';
import AuthShell from '../components/AuthShell';
import { Alert, AlertDescription } from '../components/ui/alert';
import { Button } from '../components/ui/button';
import { Checkbox } from '../components/ui/checkbox';
import { Input } from '../components/ui/input';
import { Label } from '../components/ui/label';

const REMEMBER_KEY = 'oxiproxy_remember_username';

export default function Login() {
  const navigate = useNavigate();
  const { login } = useAuth();
  const usernameRef = useRef<HTMLInputElement>(null);
  const passwordRef = useRef<HTMLInputElement>(null);
  const savedUsername = localStorage.getItem(REMEMBER_KEY) || '';
  const [username, setUsername] = useState(savedUsername);
  const [password, setPassword] = useState('');
  const [showPassword, setShowPassword] = useState(false);
  const [rememberUsername, setRememberUsername] = useState(Boolean(savedUsername));
  const [error, setError] = useState('');
  const [loading, setLoading] = useState(false);
  const [registerEnabled, setRegisterEnabled] = useState(false);

  useEffect(() => {
    authService.getRegisterStatus().then((res) => {
      if (res.success && res.data) setRegisterEnabled(res.data.enabled);
    }).catch(() => {});
  }, []);

  useEffect(() => {
    const timer = window.setTimeout(() => {
      if (savedUsername) passwordRef.current?.focus();
      else usernameRef.current?.focus();
    }, 250);
    return () => window.clearTimeout(timer);
  }, [savedUsername]);

  const handleSubmit = async (event: React.FormEvent) => {
    event.preventDefault();
    setError('');
    setLoading(true);

    try {
      const response = await authService.login({ username, password });
      if (response.success && response.data) {
        if (rememberUsername) localStorage.setItem(REMEMBER_KEY, username);
        else localStorage.removeItem(REMEMBER_KEY);

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
        setError(response.message || '登录失败');
      }
    } catch (err) {
      console.error('登录错误:', err);
      setError('登录失败，请检查用户名和密码');
    } finally {
      setLoading(false);
    }
  };

  return (
    <AuthShell
      title="欢迎回来"
      description="请登录您的账户以继续"
      footer={
        <>
          {registerEnabled && <p className="text-sm text-muted-foreground">没有账号？ <Link to="/register" className="font-medium text-primary underline-offset-4 hover:underline">立即注册</Link></p>}
          <p className="text-xs text-muted-foreground">使用管理员分配的账户登录控制台</p>
        </>
      }
    >
      <form className="space-y-5 pb-6" onSubmit={handleSubmit}>
        {error && (
          <Alert variant="destructive" className="flex items-start gap-3">
            <AlertCircle className="mt-0.5 size-4 shrink-0" />
            <AlertDescription>{error}</AlertDescription>
          </Alert>
        )}

        <div className="space-y-2">
          <Label htmlFor="username">用户名</Label>
          <div className="relative">
            <User className="pointer-events-none absolute left-3 top-1/2 size-4 -translate-y-1/2 text-muted-foreground" aria-hidden="true" />
            <Input ref={usernameRef} id="username" name="username" type="text" required value={username} onChange={(event) => setUsername(event.target.value)} className="pl-9" placeholder="请输入用户名" disabled={loading} autoComplete="username" />
          </div>
        </div>

        <div className="space-y-2">
          <Label htmlFor="password">密码</Label>
          <div className="relative">
            <Lock className="pointer-events-none absolute left-3 top-1/2 size-4 -translate-y-1/2 text-muted-foreground" aria-hidden="true" />
            <Input ref={passwordRef} id="password" name="password" type={showPassword ? 'text' : 'password'} required value={password} onChange={(event) => setPassword(event.target.value)} className="pl-9 pr-11" placeholder="请输入密码" disabled={loading} autoComplete="current-password" />
            <Button type="button" variant="ghost" size="icon" className="absolute right-1 top-1/2 size-8 -translate-y-1/2 text-muted-foreground" onClick={() => setShowPassword((visible) => !visible)} disabled={loading} aria-label={showPassword ? '隐藏密码' : '显示密码'}>
              {showPassword ? <EyeOff className="size-4" /> : <Eye className="size-4" />}
            </Button>
          </div>
        </div>

        <div className="flex items-center gap-2">
          <Checkbox id="remember" checked={rememberUsername} onCheckedChange={(checked) => setRememberUsername(checked === true)} disabled={loading} />
          <Label htmlFor="remember" className="cursor-pointer text-sm font-normal text-muted-foreground">记住用户名</Label>
        </div>

        <Button type="submit" disabled={loading} className="w-full">
          {loading ? <><Loader2 className="size-4 animate-spin" />登录中...</> : <>登录<ArrowRight className="size-4" /></>}
        </Button>
      </form>
    </AuthShell>
  );
}
