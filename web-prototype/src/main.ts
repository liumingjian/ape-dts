import { createApp } from 'vue';
import { createPinia } from 'pinia';
import piniaPluginPersistedstate from 'pinia-plugin-persistedstate';
import ElementPlus from 'element-plus';
import 'element-plus/theme-chalk/src/index.scss';

import App from './App.vue';
import router from './router';
import { i18n } from './locales';
import { useCrossTabLogout } from './composables/useCrossTabLogout';

import './styles/main.css';

async function bootstrap() {
  // MSW is opt-in via VITE_USE_MOCK=true. Production builds without the flag
  // tree-shake the entire mock layer (dynamic import + dead-branch elimination).
  if (import.meta.env.VITE_USE_MOCK === 'true') {
    try {
      const { worker } = await import('./mock/browser');
      await worker.start({
        onUnhandledRequest: 'bypass',
        serviceWorker: { url: '/mockServiceWorker.js' },
        quiet: false,
      });
    } catch (e) {
      console.warn('[mock] worker start skipped:', e);
    }
  }

  const app = createApp(App);
  const pinia = createPinia();
  pinia.use(piniaPluginPersistedstate);

  app.use(pinia);
  app.use(router);
  app.use(i18n);
  app.use(ElementPlus);

  app.mount('#app');

  // Cross-tab logout: when another tab clears the session, force this tab to /login
  useCrossTabLogout(router);
}

bootstrap();
