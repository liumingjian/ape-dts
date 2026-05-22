import { defineConfig } from 'vite';
import vue from '@vitejs/plugin-vue';
import AutoImport from 'unplugin-auto-import/vite';
import Components from 'unplugin-vue-components/vite';
import Icons from 'unplugin-icons/vite';
import IconsResolver from 'unplugin-icons/resolver';
import { ElementPlusResolver } from 'unplugin-vue-components/resolvers';
import { fileURLToPath, URL } from 'node:url';

const apiProxyTarget = process.env.VITE_API_PROXY_TARGET ?? 'http://127.0.0.1:8080';

export default defineConfig({
  plugins: [
    vue(),
    AutoImport({
      imports: ['vue', 'vue-router', 'pinia', '@vueuse/core'],
      resolvers: [
        ElementPlusResolver({ importStyle: 'sass' }),
        IconsResolver({ prefix: 'Icon' }),
      ],
      dts: 'auto-imports.d.ts',
      dirs: ['src/composables', 'src/stores'],
    }),
    Components({
      resolvers: [
        ElementPlusResolver({ importStyle: 'sass' }),
        IconsResolver({ prefix: 'Icon', enabledCollections: ['tabler'] }),
      ],
      dts: 'components.d.ts',
      dirs: ['src/components', 'src/layouts'],
    }),
    Icons({ autoInstall: false, compiler: 'vue3' }),
  ],
  resolve: {
    alias: {
      '@': fileURLToPath(new URL('./src', import.meta.url)),
    },
  },
  css: {
    preprocessorOptions: {
      scss: {
        additionalData: `@use "@/styles/element-overrides.scss" as *;`,
        api: 'modern-compiler',
      },
    },
  },
  server: {
    host: '127.0.0.1',
    port: 5173,
    open: false,
    proxy: {
      '/api': {
        target: apiProxyTarget,
        changeOrigin: true,
        // Backend bounds slow probes (e.g. test_connection) at 10s and the
        // panic-guarded handler always replies with a structured envelope.
        // 30s on both sides leaves comfortable headroom for the response
        // itself and prevents the proxy from dropping the socket — which
        // would surface as "[vite] http proxy error: socket hang up".
        timeout: 30_000,
        proxyTimeout: 30_000,
      },
    },
  },
});
