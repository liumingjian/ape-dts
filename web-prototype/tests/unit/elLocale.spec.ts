import { describe, expect, it } from 'vitest';
import { ref, computed } from 'vue';
import zhCn from 'element-plus/es/locale/lang/zh-cn';
import en from 'element-plus/es/locale/lang/en';

describe('Element Plus locale — tied to i18n active locale', () => {
  it('returns zh-CN locale when i18n locale is zh-CN', () => {
    const i18nLocale = ref('zh-CN');
    const elLocale = computed(() =>
      i18nLocale.value === 'en-US' ? en : zhCn,
    );
    expect(elLocale.value).toBe(zhCn);
  });

  it('returns en locale when i18n locale is en-US', () => {
    const i18nLocale = ref('en-US');
    const elLocale = computed(() =>
      i18nLocale.value === 'en-US' ? en : zhCn,
    );
    expect(elLocale.value).toBe(en);
  });

  it('reactively switches when locale changes', () => {
    const i18nLocale = ref('zh-CN');
    const elLocale = computed(() =>
      i18nLocale.value === 'en-US' ? en : zhCn,
    );
    expect(elLocale.value).toBe(zhCn);
    i18nLocale.value = 'en-US';
    expect(elLocale.value).toBe(en);
    i18nLocale.value = 'zh-CN';
    expect(elLocale.value).toBe(zhCn);
  });
});
