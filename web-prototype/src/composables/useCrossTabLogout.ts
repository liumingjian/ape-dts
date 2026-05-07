import { watch } from 'vue';
import { useAuthStore } from '@/stores/auth';
import type { Router } from 'vue-router';

const PERSIST_KEY = 'console.auth';

/**
 * Listens for storage events from other tabs. When the auth key is cleared
 * (another tab logged out), this tab clears its in-memory auth state and
 * redirects to /login.
 */
export function useCrossTabLogout(router: Router) {
  const auth = useAuthStore();

  function onStorage(e: StorageEvent) {
    if (e.key !== PERSIST_KEY && e.key !== null) return;

    // Key was deleted or set to null in another tab
    const raw = e.newValue;
    if (!raw) {
      auth.logout();
      router.push({ path: '/login', query: { redirect: router.currentRoute.value.fullPath } });
    }
  }

  window.addEventListener('storage', onStorage);

  // Also watch for in-tab logout to avoid redundant handling
  const stop = watch(
    () => auth.isAuthenticated,
    (isAuth, wasAuth) => {
      if (wasAuth && !isAuth) {
        // In-tab logout is already handled by TopBar.vue → router.push('/login')
      }
    },
  );

  return () => {
    window.removeEventListener('storage', onStorage);
    stop();
  };
}
