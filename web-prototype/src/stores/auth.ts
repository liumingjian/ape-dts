import { defineStore } from 'pinia';
import { ref, computed } from 'vue';

export interface CurrentUser {
  username: string;
  displayName: string;
  role: 'admin' | 'operator' | 'viewer';
  token: string;
}

export const useAuthStore = defineStore(
  'auth',
  () => {
    const user = ref<CurrentUser | null>(null);
    const isAuthenticated = computed(() => !!user.value?.token);

    function login(username: string, _password: string) {
      // Prototype: any non-empty credential works; admin gets admin role.
      const role = username === 'admin' ? 'admin' : 'operator';
      user.value = {
        username,
        displayName: username === 'admin' ? '超级管理员' : username,
        role,
        token: `mock-${Date.now()}`,
      };
    }

    function logout() {
      user.value = null;
    }

    return { user, isAuthenticated, login, logout };
  },
  {
    persist: { key: 'drs.auth', pick: ['user'] },
  },
);
