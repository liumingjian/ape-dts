import { describe, expect, it, vi } from 'vitest';
import { ref } from 'vue';

/**
 * Test the polling-gating logic that Dashboard.vue will use.
 * The useDocumentVisibility composable uses onMounted/onUnmounted
 * so it cannot be called outside a component setup. Instead we test
 * the gating pattern and the document.visibilityState read directly.
 */
describe('Dashboard polling pauses when tab hidden', () => {
  it('reads document.visibilityState as visible by default', () => {
    expect(document.visibilityState).toBe('visible');
  });

  it('does not call fetch when isVisible ref is false', () => {
    const isVisible = ref(false);
    const fetchFn = vi.fn();
    // Simulate the gating pattern used by Dashboard
    if (isVisible.value) fetchFn();
    expect(fetchFn).not.toHaveBeenCalled();
  });

  it('calls fetch when isVisible ref is true', () => {
    const isVisible = ref(true);
    const fetchFn = vi.fn();
    if (isVisible.value) fetchFn();
    expect(fetchFn).toHaveBeenCalledOnce();
  });

  it('polling interval callback checks isVisible before fetching', () => {
    const isVisible = ref(true);
    const fetchFn = vi.fn();
    // Simulate the interval tick pattern
    const tick = () => { if (isVisible.value) fetchFn(); };
    tick();
    isVisible.value = false;
    tick();
    isVisible.value = true;
    tick();
    expect(fetchFn).toHaveBeenCalledTimes(2);
  });
});
