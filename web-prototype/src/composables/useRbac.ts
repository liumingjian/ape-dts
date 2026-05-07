import { computed } from 'vue';
import { useAuthStore } from '@/stores/auth';
import {
  canPerform,
  visibleNavItems,
  type Action,
  type NavModule,
  type Role,
} from '@/auth/permissions';

export function useRbac() {
  const auth = useAuthStore();
  const role = computed<Role | null>(() => auth.user?.role ?? null);
  const visibleNav = computed<NavModule[]>(() => visibleNavItems(role.value));

  function can(action: Action): boolean {
    return canPerform(role.value, action);
  }

  return { role, visibleNav, can };
}
