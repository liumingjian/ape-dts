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

    // pinia-plugin-persistedstate writes {"user":null} instead of deleting the
    // key, so !e.newValue never triggers. Parse the JSON and check user===null.
    const raw = e.newValue;
    let loggedOut = false;
    if (!raw) {
      // Key was genuinely deleted
      loggedOut = true;
    } else {
      try {
        const parsed = JSON.parse(raw);
        if (parsed && parsed.user === null) {
          loggedOut = true;
        }
      } catch {
        // Non-JSON value — not a logout event
      }
    }

    if (loggedOut) {
      // Don't call server logout — the other tab already did that.
      // Just clear local state.
      auth.user = null;
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
