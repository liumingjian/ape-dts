import { createApp } from 'vue';
import { createPinia } from 'pinia';
import piniaPluginPersistedstate from 'pinia-plugin-persistedstate';
import ElementPlus from 'element-plus';
import 'element-plus/theme-chalk/src/index.scss';
import zhCn from 'element-plus/es/locale/lang/zh-cn';

import App from './App.vue';
import router from './router';
import { i18n } from './locales';

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
  app.use(ElementPlus, { locale: zhCn });

  app.mount('#app');
}

bootstrap();
