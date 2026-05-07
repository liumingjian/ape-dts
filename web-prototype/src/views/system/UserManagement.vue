<template>
  <div class="user-mgmt">
    <PageHeader :title="t('system.users.title')" :subtitle="t('system.users.subtitle')" />

    <div class="ape-dts-console-page user-mgmt__body">
      <div v-loading="loading" class="ape-dts-console-card user-mgmt__panel">
        <div class="user-mgmt__toolbar">
          <el-button type="primary" data-testid="btn-create-user" @click="showCreateDialog = true">
            {{ t('system.users.createUser') }}
          </el-button>
          <el-button @click="loadUsers">
            {{ t('common.refresh') }}
          </el-button>
        </div>

        <el-table :data="users" class="user-mgmt__table" data-testid="user-table" empty-text="">
          <el-table-column prop="username" :label="t('system.users.username')" min-width="120" />
          <el-table-column prop="displayName" :label="t('system.users.displayName')" min-width="120">
            <template #default="{ row }">{{ row.displayName || '—' }}</template>
          </el-table-column>
          <el-table-column prop="role" :label="t('system.users.role')" width="120">
            <template #default="{ row }">
              <el-tag :type="roleTagType(row.role)" size="small">{{ row.role }}</el-tag>
            </template>
          </el-table-column>
          <el-table-column prop="disabled" :label="t('system.users.status')" width="100">
            <template #default="{ row }">
              <el-tag :type="row.disabled ? 'danger' : 'success'" size="small">
                {{ row.disabled ? t('common.disable') : t('common.enable') }}
              </el-tag>
            </template>
          </el-table-column>
          <el-table-column prop="createdAt" :label="t('system.users.createdAt')" min-width="160">
            <template #default="{ row }">{{ row.createdAt || '—' }}</template>
          </el-table-column>
          <el-table-column :label="t('common.action')" width="200" fixed="right">
            <template #default="{ row }">
              <el-button link type="primary" @click="openEditDialog(row)">{{ t('common.edit') }}</el-button>
              <el-button link type="warning" @click="toggleDisable(row)">
                {{ row.disabled ? t('common.enable') : t('common.disable') }}
              </el-button>
              <el-button link type="danger" @click="confirmDelete(row)">{{ t('common.delete') }}</el-button>
            </template>
          </el-table-column>
        </el-table>
      </div>
    </div>

    <!-- Create User Dialog -->
    <el-dialog v-model="showCreateDialog" :title="t('system.users.createUser')" width="480" destroy-on-close>
      <el-form :model="createForm" label-width="100px">
        <el-form-item :label="t('system.users.username')" required>
          <el-input v-model="createForm.username" />
        </el-form-item>
        <el-form-item :label="t('system.users.password')" required>
          <el-input v-model="createForm.password" type="password" show-password />
        </el-form-item>
        <el-form-item :label="t('system.users.displayName')">
          <el-input v-model="createForm.displayName" />
        </el-form-item>
        <el-form-item :label="t('system.users.role')" required>
          <el-select v-model="createForm.role" style="width: 100%">
            <el-option value="admin" label="Admin" />
            <el-option value="operator" label="Operator" />
            <el-option value="viewer" label="Viewer" />
          </el-select>
        </el-form-item>
      </el-form>
      <template #footer>
        <el-button @click="showCreateDialog = false">{{ t('common.cancel') }}</el-button>
        <el-button type="primary" :loading="creating" @click="createUser">{{ t('common.confirm') }}</el-button>
      </template>
    </el-dialog>

    <!-- Edit User Dialog -->
    <el-dialog v-model="showEditDialog" :title="t('common.edit')" width="480" destroy-on-close>
      <el-form :model="editForm" label-width="100px">
        <el-form-item :label="t('system.users.displayName')">
          <el-input v-model="editForm.displayName" />
        </el-form-item>
        <el-form-item :label="t('system.users.password')">
          <el-input v-model="editForm.password" type="password" show-password :placeholder="t('system.users.passwordPlaceholder')" />
        </el-form-item>
        <el-form-item :label="t('system.users.role')">
          <el-select v-model="editForm.role" style="width: 100%">
            <el-option value="admin" label="Admin" />
            <el-option value="operator" label="Operator" />
            <el-option value="viewer" label="Viewer" />
          </el-select>
        </el-form-item>
      </el-form>
      <template #footer>
        <el-button @click="showEditDialog = false">{{ t('common.cancel') }}</el-button>
        <el-button type="primary" :loading="saving" @click="saveUser">{{ t('common.confirm') }}</el-button>
      </template>
    </el-dialog>
  </div>
</template>

<script setup lang="ts">
import { onMounted, reactive, ref } from 'vue';
import { useI18n } from 'vue-i18n';
import { ElMessage, ElMessageBox } from 'element-plus';
import { api } from '@/api/client';
import PageHeader from '@/components/PageHeader.vue';

const { t } = useI18n();

interface UserRow {
  id: string;
  username: string;
  displayName: string;
  role: 'admin' | 'operator' | 'viewer';
  disabled: boolean;
  createdAt: string;
}

const users = ref<UserRow[]>([]);
const loading = ref(false);

async function loadUsers() {
  loading.value = true;
  try {
    const res = await api.get<{ items: UserRow[] }>('/users');
    users.value = res.items ?? (Array.isArray(res) ? res : []);
  } catch {
    users.value = [];
  } finally {
    loading.value = false;
  }
}

onMounted(loadUsers);

function roleTagType(role: string) {
  if (role === 'admin') return 'danger';
  if (role === 'operator') return 'warning';
  return 'info';
}

/* ---------- Create ---------- */
const showCreateDialog = ref(false);
const creating = ref(false);
const createForm = reactive({ username: '', password: '', displayName: '', role: 'viewer' as const });

async function createUser() {
  if (!createForm.username || !createForm.password) {
    ElMessage.warning(t('system.users.requiredFields'));
    return;
  }
  creating.value = true;
  try {
    await api.post('/users', {
      username: createForm.username,
      password: createForm.password,
      displayName: createForm.displayName,
      role: createForm.role,
    });
    ElMessage.success(t('system.users.created'));
    showCreateDialog.value = false;
    Object.assign(createForm, { username: '', password: '', displayName: '', role: 'viewer' });
    await loadUsers();
  } catch (err: unknown) {
    const msg = (err as { message?: string })?.message ?? String(err);
    ElMessage.error(msg);
  } finally {
    creating.value = false;
  }
}

/* ---------- Edit ---------- */
const showEditDialog = ref(false);
const saving = ref(false);
const editForm = reactive({ id: '', displayName: '', password: '', role: 'viewer' as UserRow['role'] });

function openEditDialog(row: UserRow) {
  editForm.id = row.id;
  editForm.displayName = row.displayName;
  editForm.password = '';
  editForm.role = row.role;
  showEditDialog.value = true;
}

async function saveUser() {
  saving.value = true;
  try {
    const body: Record<string, unknown> = {
      displayName: editForm.displayName,
      role: editForm.role,
    };
    if (editForm.password) body.password = editForm.password;
    await api.patch(`/users/${editForm.id}`, body);
    ElMessage.success(t('system.users.saved'));
    showEditDialog.value = false;
    await loadUsers();
  } catch (err: unknown) {
    const msg = (err as { message?: string })?.message ?? String(err);
    ElMessage.error(msg);
  } finally {
    saving.value = false;
  }
}

/* ---------- Toggle disable ---------- */
async function toggleDisable(row: UserRow) {
  try {
    await api.patch(`/users/${row.id}`, { disabled: !row.disabled });
    ElMessage.success(t('system.users.saved'));
    await loadUsers();
  } catch (err: unknown) {
    const msg = (err as { message?: string })?.message ?? String(err);
    ElMessage.error(msg);
  }
}

/* ---------- Delete ---------- */
async function confirmDelete(row: UserRow) {
  try {
    await ElMessageBox.confirm(
      t('system.users.deleteConfirm', { name: row.username }),
      t('common.delete'),
      { confirmButtonText: t('common.confirm'), cancelButtonText: t('common.cancel'), type: 'warning' },
    );
    await api.del(`/users/${row.id}`);
    ElMessage.success(t('system.users.deleted'));
    await loadUsers();
  } catch {
    /* cancelled or API error — message already shown */
  }
}
</script>

<style scoped>
.user-mgmt__body {
  padding: 0 24px 24px;
}
.user-mgmt__panel {
  padding: 20px;
}
.user-mgmt__toolbar {
  display: flex;
  gap: 12px;
  margin-bottom: 16px;
}
</style>
