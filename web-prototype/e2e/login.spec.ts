import { test, expect, type Page } from '@playwright/test';

async function loginAsAdmin(page: Page) {
  await page.goto('/login');
  await page.locator('input[autocomplete="username"]').fill('admin');
  await page.locator('input[autocomplete="current-password"]').fill('admin123');
  await page.getByRole('button', { name: /Sign in|登录/i }).click();
  await expect(page).toHaveURL(/\/dashboard$/);
}

test.describe('canonical Console navigation', () => {
  test('anonymous deep link preserves the exact return target through login', async ({ page }) => {
    await page.goto('/tasks/migration?mode=cdc&status=running#top');
    await expect(page).toHaveURL((url) =>
      url.pathname === '/login'
      && url.searchParams.get('redirect') === '/tasks/migration?mode=cdc&status=running#top',
    );

    await page.locator('input[autocomplete="username"]').fill('admin');
    await page.locator('input[autocomplete="current-password"]').fill('admin123');
    await page.getByRole('button', { name: /Sign in|登录/i }).click();
    await expect(page).toHaveURL(/\/tasks\/migration\?mode=cdc&status=running#top$/);
  });

  test('authenticated login redirects to the dashboard', async ({ page }) => {
    await loginAsAdmin(page);
    await page.goto('/login');
    await expect(page).toHaveURL(/\/dashboard$/);
  });

  test('canonical list, create, and detail deep links survive reload', async ({ page }) => {
    await loginAsAdmin(page);

    for (const path of [
      '/tasks/migration?mode=cdc&status=running#top',
      '/tasks/check',
      '/tasks/struct',
      '/tasks/create/migration?mode=cdc',
      '/tasks/check/check-task-001?tab=alerts',
      '/tasks/struct/struct-task-001?tab=config',
      '/tasks/migration/cdc-task-001?mode=cdc&tab=logs#tail',
    ]) {
      await page.goto(path);
      await page.reload();
      await expect(page).toHaveURL(new RegExp(`${path.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}$`));
      await expect(page.locator('main')).toBeVisible();
    }
  });

  test('legacy list, create, and detail links preserve state after redirect and reload', async ({ page }) => {
    await loginAsAdmin(page);

    const cases = [
      ['/tasks/snapshot?mode=cdc&source=saved#tail', '/tasks/migration', 'snapshot'],
      ['/tasks/cdc?mode=snapshot&source=saved#tail', '/tasks/migration', 'cdc'],
      ['/tasks/sync?mode=cdc&source=saved#tail', '/tasks/migration', 'cdc'],
      ['/tasks/replay?source=saved#tail', '/tasks/migration', undefined],
      ['/tasks/verify?source=saved#tail', '/tasks/check', undefined],
      ['/tasks/create/snapshot?mode=cdc&source=saved#tail', '/tasks/create/migration', 'snapshot'],
      ['/tasks/create/cdc?mode=snapshot&source=saved#tail', '/tasks/create/migration', 'cdc'],
      ['/tasks/create/sync?mode=cdc&source=saved#tail', '/tasks/create/migration', 'cdc'],
      ['/tasks/create/replay?source=saved#tail', '/tasks/create/migration', 'snapshot'],
      ['/tasks/create/verify?source=saved#tail', '/tasks/create/check', undefined],
      ['/tasks/snapshot/task-001?mode=cdc&source=saved#tail', '/tasks/migration/task-001', 'snapshot'],
      ['/tasks/cdc/task-001?mode=snapshot&source=saved#tail', '/tasks/migration/task-001', 'cdc'],
      ['/tasks/sync/task-001?mode=cdc&source=saved#tail', '/tasks/migration/task-001', 'cdc'],
      ['/tasks/replay/task-001?source=saved#tail', '/tasks/migration/task-001', 'snapshot'],
      ['/tasks/verify/task-001?source=saved#tail', '/tasks/check/task-001', undefined],
    ] as const;

    for (const [legacyPath, canonicalPath, mode] of cases) {
      await page.goto(legacyPath);
      await expect(page).toHaveURL((url) =>
        url.pathname === canonicalPath
        && url.searchParams.get('source') === 'saved'
        && url.searchParams.get('mode') === (mode ?? null)
        && url.hash === '#tail',
      );
      await page.reload();
      await expect(page).toHaveURL((url) =>
        url.pathname === canonicalPath
        && url.searchParams.get('source') === 'saved'
        && url.searchParams.get('mode') === (mode ?? null)
        && url.hash === '#tail',
      );
    }
  });

  test('hidden migration pages keep Data Migration active in the sidebar and breadcrumbs', async ({ page }) => {
    await loginAsAdmin(page);
    await page.goto('/tasks/migration/cdc-task-001?mode=cdc');

    const activeItem = page.locator('.sidebar__menu .el-menu-item.is-active');
    await expect(activeItem).toContainText(/Data Migration|数据迁移/);
    await expect(page.locator('.topbar__crumb')).toContainText(/Data Migration|数据迁移/);
    await expect(page.locator('.sidebar__menu')).not.toContainText(/^CDC$/);
  });

  test('opens the canonical CDC migration wizard at step 1', async ({ page }) => {
    await loginAsAdmin(page);
    await page.goto('/tasks/create/migration?mode=cdc');
    await expect(page).toHaveURL(/\/tasks\/create\/migration\?mode=cdc$/);
    await expect(
      page.getByRole('heading', { name: /源数据库信息|Source database/i }).first(),
    ).toBeVisible({ timeout: 15_000 });
    await expect(page.getByRole('button', { name: /MySQL/ }).first()).toBeVisible();
    await expect(page.getByRole('button', { name: /PostgreSQL/ }).first()).toBeVisible();
    await expect(page.getByRole('button', { name: /GaussDB/ }).first()).toBeVisible();
  });

  test.skip('full wizard submit + start + see metrics + stop (parked for follow-up)', async () => {
    // Pending real-API wiring beyond MSW happy paths.
  });
});
