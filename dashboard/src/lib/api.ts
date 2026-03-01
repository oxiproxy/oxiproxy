import axios from 'axios';

const API_BASE_URL = import.meta.env.VITE_API_URL || '/api';

export const api = axios.create({
  baseURL: API_BASE_URL,
  headers: {
    'Content-Type': 'application/json',
  },
});

// 请求拦截器 - 添加 token
api.interceptors.request.use(
  (config) => {
    const token = localStorage.getItem('token');
    if (token) {
      config.headers.Authorization = `Bearer ${token}`;
    }
    return config;
  },
  (error) => {
    return Promise.reject(error);
  }
);

// 响应拦截器 - 处理错误
api.interceptors.response.use(
  (response) => response,
  (error) => {
    if (error.response?.status === 401) {
      const url = error.config?.url || '';
      const errorMessage = error.response?.data?.message || '';

      // 登录/注册接口的401错误应该由组件自己处理，不在这里跳转
      const isLoginRequest = url.includes('/auth/login') || url.includes('/auth/register');
      if (isLoginRequest) {
        // 返回 ApiResponse 格式让登录页展示后端的详细错误信息
        if (error.response?.data && typeof error.response.data.success === 'boolean') {
          return error.response;
        }
        return Promise.reject(error);
      }

      // 只在token真正无效时才登出，避免过于激进的登出行为
      // 检查是否是认证相关的401错误
      const isAuthError =
        url.includes('/auth/') ||
        errorMessage.toLowerCase().includes('token') ||
        errorMessage.toLowerCase().includes('unauthorized') ||
        errorMessage.toLowerCase().includes('not authenticated');

      if (isAuthError) {
        console.warn('认证失败，正在跳转到登录页...');
        localStorage.removeItem('token');
        localStorage.removeItem('user');
        window.location.href = '/login';
        return Promise.reject(error);
      } else {
        // 其他401错误只记录日志，不登出用户
        console.error('权限不足:', url, errorMessage);
      }
    }

    // 如果响应体是 ApiResponse 格式（含 success 字段），将其作为正常响应返回，
    // 让业务代码通过 response.success 判断并展示后端返回的详细错误信息
    if (error.response?.data && typeof error.response.data.success === 'boolean') {
      return error.response;
    }

    return Promise.reject(error);
  }
);

export default api;
