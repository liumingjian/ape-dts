import { ref, onMounted, onUnmounted } from 'vue';

/**
 * Reactive wrapper around document.visibilityState.
 * Polling composables should gate their interval on `isVisible.value`.
 */
export function useDocumentVisibility() {
  const isVisible = ref(!import.meta.env.SSR && document.visibilityState !== 'hidden');

  function onChange() {
    isVisible.value = document.visibilityState !== 'hidden';
  }

  onMounted(() => document.addEventListener('visibilitychange', onChange));
  onUnmounted(() => document.removeEventListener('visibilitychange', onChange));

  return { isVisible };
}
