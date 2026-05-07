import { defineStore } from 'pinia';
import { ref, computed } from 'vue';
import { api } from '@/api/client';
import { closeAllSseStreams } from '@/composables/useSseRegistry';

export interface CurrentUser {
  username: string;
  displayName: string;
  role: 'admin' | 'operator' | 'viewer';
}

/** Server returns snake_case; normalize to camelCase. */
interface LoginResponse {
  username: string;
  display_name: string;
  role: 'admin' | 'operator' | 'viewer';
}

function normalizeUser(res: LoginResponse): CurrentUser {
  return { username: res.username, displayName: res.display_name, role: res.role };
}

export const useAuthStore = defineStore(
  'auth',
  () => {
    const user = ref<CurrentUser | null>(null);
    const isAuthenticated = computed(() => !!user.value);

    /** Call POST /api/auth/login — returns the user on success, throws ApiError on failure. */
    async function login(username: string, password: string): Promise<CurrentUser> {
      const res = await api.post<LoginResponse>('/auth/login', { username, password });
      const normalized = normalizeUser(res);
      user.value = normalized;
      return normalized;
    }

    /** Call POST /api/auth/logout, then clear local state. */
    async function logout(): Promise<void> {
      try {
        await api.post('/auth/logout');
      } catch {
        // Even if the server call fails we still clear local state
      }
      // Close all SSE streams to prevent stale connections
      // that bypass session invalidation
      closeAllSseStreams();
      user.value = null;
    }

    /** Hydrate current user from GET /api/auth/me (e.g. on app boot). */
    async function fetchMe(): Promise<CurrentUser | null> {
      try {
        const res = await api.get<LoginResponse>('/auth/me');
        const normalized = normalizeUser(res);
        user.value = normalized;
        return normalized;
      } catch {
        user.value = null;
        return null;
      }
    }

    return { user, isAuthenticated, login, logout, fetchMe };
  },
  {
    persist: { key: 'console.auth', pick: ['user'] },
  },
);
