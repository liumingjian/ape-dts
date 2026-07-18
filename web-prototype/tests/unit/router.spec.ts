import { beforeEach, describe, expect, it } from 'vitest';
import { createMemoryHistory, createRouter, type Router } from 'vue-router';
import { routes } from '@/router/index';

let router: Router;

beforeEach(() => {
  router = createRouter({ history: createMemoryHistory('/'), routes });
});

describe('Router task taxonomy redirects', () => {
  it('redirects legacy /tasks/snapshot to /tasks/migration?mode=snapshot', async () => {
    await router.push('/tasks/snapshot');
    expect(router.currentRoute.value.path).toBe('/tasks/migration');
    expect(router.currentRoute.value.query.mode).toBe('snapshot');
  });

  it('redirects legacy /tasks/cdc to /tasks/migration?mode=cdc', async () => {
    await router.push('/tasks/cdc');
    expect(router.currentRoute.value.path).toBe('/tasks/migration');
    expect(router.currentRoute.value.query.mode).toBe('cdc');
  });

  it('preserves query and hash when redirecting legacy list paths', async () => {
    await router.push('/tasks/snapshot?status=running&engine=mysql#top');
    expect(router.currentRoute.value.path).toBe('/tasks/migration');
    expect(router.currentRoute.value.query.mode).toBe('snapshot');
    expect(router.currentRoute.value.query.status).toBe('running');
    expect(router.currentRoute.value.query.engine).toBe('mysql');
    expect(router.currentRoute.value.hash).toBe('#top');
  });

  it('redirects /tasks/sync to the migration module', async () => {
    await router.push('/tasks/sync?mode=snapshot_cdc');
    expect(router.currentRoute.value.path).toBe('/tasks/migration');
    expect(router.currentRoute.value.query.mode).toBe('snapshot_cdc');
  });

  it('preserves an explicit valid mode on /tasks/sync redirects', async () => {
    await router.push('/tasks/sync?mode=cdc');
    expect(router.currentRoute.value.path).toBe('/tasks/migration');
    expect(router.currentRoute.value.query.mode).toBe('cdc');
  });

  it('redirects /tasks/replay and /tasks/verify to supported modules without losing state', async () => {
    await router.push('/tasks/replay?status=running#top');
    expect(router.currentRoute.value.path).toBe('/tasks/migration');
    expect(router.currentRoute.value.query.status).toBe('running');
    expect(router.currentRoute.value.hash).toBe('#top');

    await router.push('/tasks/verify?status=failed&engine=mysql#results');
    expect(router.currentRoute.value.path).toBe('/tasks/check');
    expect(router.currentRoute.value.query.status).toBe('failed');
    expect(router.currentRoute.value.query.engine).toBe('mysql');
    expect(router.currentRoute.value.hash).toBe('#results');
  });

  it('redirects legacy detail paths to migration detail while preserving query and hash', async () => {
    await router.push('/tasks/cdc/abc-123?tab=alerts#tail');
    expect(router.currentRoute.value.path).toBe('/tasks/migration/abc-123');
    expect(router.currentRoute.value.query.mode).toBe('cdc');
    expect(router.currentRoute.value.query.tab).toBe('alerts');
    expect(router.currentRoute.value.hash).toBe('#tail');
  });

  it('preserves an explicit valid mode on legacy sync detail redirects', async () => {
    await router.push('/tasks/sync/abc-123?mode=cdc&tab=alerts#tail');
    expect(router.currentRoute.value.path).toBe('/tasks/migration/abc-123');
    expect(router.currentRoute.value.query.mode).toBe('cdc');
    expect(router.currentRoute.value.query.tab).toBe('alerts');
    expect(router.currentRoute.value.hash).toBe('#tail');
  });

  it('redirects legacy create paths to migration create with mode defaults', async () => {
    await router.push('/tasks/create/snapshot');
    expect(router.currentRoute.value.path).toBe('/tasks/create/migration');
    expect(router.currentRoute.value.query.mode).toBe('snapshot');

    await router.push('/tasks/create/cdc');
    expect(router.currentRoute.value.path).toBe('/tasks/create/migration');
    expect(router.currentRoute.value.query.mode).toBe('cdc');

    await router.push('/tasks/create/sync');
    expect(router.currentRoute.value.path).toBe('/tasks/create/migration');
    expect(router.currentRoute.value.query.mode).toBe('snapshot_cdc');
  });

  it('preserves an explicit valid mode on legacy sync create redirects', async () => {
    await router.push('/tasks/create/sync?mode=cdc&template=fast#top');
    expect(router.currentRoute.value.path).toBe('/tasks/create/migration');
    expect(router.currentRoute.value.query.mode).toBe('cdc');
    expect(router.currentRoute.value.query.template).toBe('fast');
    expect(router.currentRoute.value.hash).toBe('#top');
  });

  it.each([
    ['/tasks/snapshot/item-1?mode=cdc&tab=logs#tail', '/tasks/migration/item-1', 'snapshot'],
    ['/tasks/cdc/item-1?mode=snapshot&tab=logs#tail', '/tasks/migration/item-1', 'cdc'],
    ['/tasks/sync/item-1?mode=cdc&tab=logs#tail', '/tasks/migration/item-1', 'cdc'],
    ['/tasks/replay/item-1?mode=cdc&tab=logs#tail', '/tasks/migration/item-1', 'snapshot'],
    ['/tasks/verify/item-1?tab=logs#tail', '/tasks/check/item-1', undefined],
  ])('redirects legacy detail %s with canonical mode and state', async (legacyPath, canonicalPath, expectedMode) => {
    await router.push(legacyPath);
    expect(router.currentRoute.value.path).toBe(canonicalPath);
    expect(router.currentRoute.value.query.mode).toBe(expectedMode);
    expect(router.currentRoute.value.query.tab).toBe('logs');
    expect(router.currentRoute.value.hash).toBe('#tail');
  });

  it.each([
    ['/tasks/create/snapshot?mode=cdc&template=fast#top', '/tasks/create/migration', 'snapshot'],
    ['/tasks/create/cdc?mode=snapshot&template=fast#top', '/tasks/create/migration', 'cdc'],
    ['/tasks/create/sync?mode=cdc&template=fast#top', '/tasks/create/migration', 'cdc'],
    ['/tasks/create/replay?mode=cdc&template=fast#top', '/tasks/create/migration', 'snapshot'],
    ['/tasks/create/verify?template=fast#top', '/tasks/create/check', undefined],
  ])('redirects legacy create %s with canonical mode and state', async (legacyPath, canonicalPath, expectedMode) => {
    await router.push(legacyPath);
    expect(router.currentRoute.value.path).toBe(canonicalPath);
    expect(router.currentRoute.value.query.mode).toBe(expectedMode);
    expect(router.currentRoute.value.query.template).toBe('fast');
    expect(router.currentRoute.value.hash).toBe('#top');
  });

  it('resolves canonical migration, check, and struct paths directly', async () => {
    await router.push('/tasks/migration');
    expect(router.currentRoute.value.path).toBe('/tasks/migration');
    expect(router.currentRoute.value.name).toBe('MigrationTasks');

    await router.push('/tasks/check');
    expect(router.currentRoute.value.path).toBe('/tasks/check');
    expect(router.currentRoute.value.name).toBe('CheckTasks');

    await router.push('/tasks/struct');
    expect(router.currentRoute.value.path).toBe('/tasks/struct');
    expect(router.currentRoute.value.name).toBe('StructTasks');
  });
});
