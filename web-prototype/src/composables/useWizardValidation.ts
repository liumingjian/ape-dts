import type { TaskCategory, EngineType, GaussdbSubMode } from '@/types/domain';
import type { WizardStepKey } from '@/composables/useWizardSteps';

export type ErrorMap = Record<string, string>;

export interface WizardFormEndpoint {
  engine: EngineType;
  subMode?: GaussdbSubMode;
  host: string;
  port: number;
  username: string;
  password: string;
  database: string;
  ssl: boolean;
}

export interface WizardFormConfig {
  parallelizer: string;
  parallelSize: number;
  bufferSize: number;
  checkpointIntervalSecs: number;
  maxRps: number;
  resumeType: string;
  metricsEnabled: boolean;
  metricsHttpPort: number;
}

export interface WizardFormModel {
  name: string;
  description: string;
  taskType: 'standalone' | 'primary_backup';
  resourceGroup: string;
  instanceIp: string;
  source: WizardFormEndpoint;
  target: WizardFormEndpoint;
  syncMode: string;
  config: WizardFormConfig;
}

/**
 * Validate a single wizard step against the live WizardForm model.
 * Returns a map of field key → i18n error key.
 */
export function validateStep(step: WizardStepKey, form: Partial<WizardFormModel>, category: TaskCategory): ErrorMap {
  const errors: ErrorMap = {};

  switch (step) {
    case 'source':
      if (!form.name) errors.name = 'task.name.required';
      if (!form.source?.engine) errors['source.engine'] = 'engine.required';
      if (!form.target?.engine) errors['target.engine'] = 'engine.required';
      if (!form.source?.host) errors['source.host'] = 'host.required';
      if (!form.target?.host) errors['target.host'] = 'host.required';
      if (form.source?.engine === 'gaussdb' && !form.source?.subMode) {
        errors['source.subMode'] = 'gaussdb.subMode.required';
      }
      if (form.target?.engine === 'gaussdb' && !form.target?.subMode) {
        errors['target.subMode'] = 'gaussdb.subMode.required';
      }
      break;

    case 'test':
      // Test connection validation is handled by the ConnectionTestCard component
      break;

    case 'objects':
      if (category !== 'struct') {
        // Non-struct requires at least some selection; this is validated in UI
      }
      break;

    case 'processing':
      break;

    case 'advanced':
      if (category !== 'struct') {
        if (!form.config?.parallelSize || form.config.parallelSize <= 0) {
          errors['config.parallelSize'] = 'parallel.size.positive';
        }
        if (!form.config?.bufferSize || form.config.bufferSize <= 0) {
          errors['config.bufferSize'] = 'buffer.size.positive';
        }
      }
      break;

    case 'precheck':
      break;

    case 'confirm':
      break;
  }

  return errors;
}
