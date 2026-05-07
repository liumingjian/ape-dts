import { test, expect } from '@playwright/test';

test.describe('happy path · login → dashboard → snapshot wizard', () => {
  test('unauthenticated visit redirects to /login', async ({ page }) => {
    await page.goto('/');
    await expect(page).toHaveURL(/\/login(\?.*)?$/);
    await expect(page.getByText('ape-dts Console', { exact: false }).first()).toBeVisible();
  });

  test('admin/admin123 logs in and lands on the dashboard', async ({ page }) => {
    await page.goto('/login');

    const usernameInput = page.locator('input[autocomplete="username"]');
    const passwordInput = page.locator('input[autocomplete="current-password"]');
    await expect(usernameInput).toBeVisible();
    await usernameInput.fill('admin');
    await passwordInput.fill('admin123');

    await page.getByRole('button', { name: /Sign in|登录/i }).click();

    await expect(page).toHaveURL(/\/dashboard$/, { timeout: 15_000 });
    await expect(page.locator('body')).toContainText(/Dashboard|仪表盘|控制台/);
  });

  test('navigates to Snapshot Migration list and opens wizard step 1', async ({ page }) => {
    await page.goto('/login');
    await page.locator('input[autocomplete="username"]').fill('admin');
    await page.locator('input[autocomplete="current-password"]').fill('admin123');
    await page.getByRole('button', { name: /Sign in|登录/i }).click();
    await expect(page).toHaveURL(/\/dashboard$/);

    await page.goto('/tasks/snapshot');
    await expect(page).toHaveURL(/\/tasks\/snapshot$/);

    await page.goto('/tasks/create/snapshot');
    await expect(page).toHaveURL(/\/tasks\/create\/snapshot$/);
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
