/**
 * MSW handlers aggregate — combines all module handlers.
 */
import { authHandlers } from './auth';
import { taskHandlers } from './tasks';
import { alertHandlers } from './alerts';
import { alertMonitorHandlers } from './alertMonitor';
import { miscHandlers } from './misc';
import { dashboardHandlers } from './dashboard';

export const handlers = [
  ...authHandlers,
  ...taskHandlers,
  ...alertHandlers,
  ...alertMonitorHandlers,
  ...miscHandlers,
  ...dashboardHandlers,
];
