/**
 * Auth handlers — accept any non-empty credential. Admin user gets elevated role.
 */
import { http } from 'msw';
import { pause, ok, badRequest } from './_shared';
import { db } from '../db';

interface LoginBody {
  username?: string;
  password?: string;
}

export const authHandlers = [
  http.post('/api/auth/login', async ({ request }) => {
    await pause();
    const body = (await request.json().catch(() => ({}))) as LoginBody;
    const username = (body.username ?? '').trim();
    const password = (body.password ?? '').trim();
    if (!username || !password) {
      return badRequest('username_and_password_required', 'EMPTY_CREDENTIAL');
    }
    const user = db.users.find((u) => u.username === username)
      ?? {
        ...db.users[0],
        username,
        displayName: username === 'admin' ? '超级管理员' : username,
        role: username === 'admin' ? 'admin' as const : 'operator' as const,
      };
    user.lastLoginAt = new Date().toISOString();
    return ok({
      username: user.username,
      display_name: user.displayName,
      role: user.role,
    });
  }),

  http.post('/api/auth/logout', async () => {
    await pause(80, 200);
    return ok({ ok: true });
  }),

  http.get('/api/session', async () => {
    await pause(60, 150);
    return ok({ now: new Date().toISOString(), users: db.users });
  }),
];
