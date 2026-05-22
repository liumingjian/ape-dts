import { mount, type MountingOptions } from '@vue/test-utils';
import { createI18n } from 'vue-i18n';
import { createPinia } from 'pinia';
import type { Component } from 'vue';

export interface MountOptions<P> extends MountingOptions<P> {
  locale?: 'zh-CN' | 'en-US';
}

export function mountWithProviders<P>(
  component: Component,
  options: MountOptions<P> = {},
) {
  const { locale = 'zh-CN', global: globalOpts = {}, ...rest } = options;
  const i18n = createI18n({
    legacy: false,
    locale,
    fallbackLocale: 'zh-CN',
    messages: { 'zh-CN': {}, 'en-US': {} },
    missingWarn: false,
    fallbackWarn: false,
  });
  return mount(component, {
    ...rest,
    global: {
      ...globalOpts,
      plugins: [i18n, createPinia(), ...((globalOpts.plugins as never[]) ?? [])],
    },
  });
}
