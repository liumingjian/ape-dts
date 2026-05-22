import { createI18n } from 'vue-i18n';
import zhCN from './zh-CN.json';
import enUS from './en-US.json';

export const SUPPORTED_LOCALES = [
  { code: 'zh-CN', label: '中文（简体）' },
  { code: 'en-US', label: 'English' },
] as const;

export type LocaleCode = (typeof SUPPORTED_LOCALES)[number]['code'];

const STORAGE_KEY = 'console.locale';
const initial = (localStorage.getItem(STORAGE_KEY) as LocaleCode) || 'zh-CN';

export const i18n = createI18n({
  legacy: false,
  locale: initial,
  fallbackLocale: 'zh-CN',
  messages: {
    'zh-CN': zhCN,
    'en-US': enUS,
  },
});

export function setLocale(locale: LocaleCode) {
  i18n.global.locale.value = locale;
  localStorage.setItem(STORAGE_KEY, locale);
  document.documentElement.lang = locale;
}
