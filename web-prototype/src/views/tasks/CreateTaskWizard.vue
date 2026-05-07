<template>
  <div class="wizard">
    <header class="wizard__header">
      <div class="wizard__header-inner">
        <el-button link class="wizard__back" @click="onBack">
          <IconArrowLeft /> {{ t('wizard.header.back') }}
        </el-button>
        <span class="wizard__sep">|</span>
        <h1 class="wizard__title">{{ t('wizard.header.create', { type: t(`task.type.${category}`) }) }}</h1>
      </div>
    </header>

    <nav class="wizard__steps">
      <ol>
        <li
          v-for="(s, idx) in steps"
          :key="s.key"
          :class="stepClass(idx)"
          @click="gotoStep(idx)"
        >
          <span class="wizard__step-badge">
            <IconCheck v-if="idx < current" />
            <span v-else>{{ idx + 1 }}</span>
          </span>
          <span class="wizard__step-label">{{ s.label }}</span>
          <span v-if="idx < steps.length - 1" class="wizard__step-bar" />
        </li>
      </ol>
    </nav>

    <div class="wizard__body">
      <!-- STEP 1: Source / target / basic / mode -->
      <template v-if="currentStep?.key === 'source'">
        <el-alert type="warning" :closable="false" show-icon class="wizard__alert">
          <template #default>
            <div class="wizard__alert-text">{{ t('wizard.alert.beforeStart') }}</div>
          </template>
        </el-alert>

        <div class="wizard__grid wizard__grid--2">
          <section class="ape-dts-console-card wizard__card">
            <header class="wizard__card-head">
              <h3>{{ t('wizard.source.section.source') }}</h3>
              <span class="wizard__card-tag wizard__card-tag--locked">{{ t('wizard.source.notEditable') }}</span>
            </header>
            <div class="wizard__form">
              <label>{{ t('wizard.source.engineType') }}</label>
              <div class="wizard__engine-grid">
                <button
                  v-for="e in engineOptions"
                  :key="e.value"
                  type="button"
                  class="wizard__engine-chip"
                  :class="{ 'wizard__engine-chip--active': form.source.engine === e.value }"
                  @click="setSourceEngine(e.value)"
                >
                  <EngineTag :engine="e.value" icon-only />
                  <span>{{ e.label }}</span>
                </button>
              </div>

              <template v-if="form.source.engine === 'gaussdb'">
                <label class="required">{{ t('wizard.source.subMode._label') }}</label>
                <el-radio-group v-model="form.source.subMode" size="small">
                  <el-radio-button
                    v-for="sm in GAUSSDB_SUB_MODES"
                    :key="sm"
                    :label="sm"
                  >{{ t(`wizard.source.subMode.${sm}`) }}</el-radio-button>
                </el-radio-group>
                <small class="wizard__hint">{{ t('wizard.source.subMode.hint') }}</small>
              </template>

              <div class="wizard__row wizard__row--split">
                <div>
                  <label class="required">{{ t('wizard.source.host') }}</label>
                  <el-input v-model="form.source.host" placeholder="192.168.1.116" />
                </div>
                <div>
                  <label class="required">{{ t('wizard.source.port') }}</label>
                  <el-input-number
                    v-model="form.source.port"
                    :min="1"
                    :max="65535"
                    controls-position="right"
                    style="width: 100%"
                  />
                </div>
              </div>
              <label class="required">{{ t('wizard.source.username') }}</label>
              <el-input v-model="form.source.username" placeholder="root" />
              <label class="required">{{ t('wizard.source.password') }}</label>
              <el-input v-model="form.source.password" type="password" show-password />
              <label>{{ t('wizard.source.database') }}</label>
              <el-input v-model="form.source.database" placeholder="app_db" />
              <div class="wizard__row wizard__row--switch">
                <label>{{ t('wizard.source.ssl') }}</label>
                <el-switch v-model="form.source.ssl" />
              </div>
            </div>
          </section>

          <section class="ape-dts-console-card wizard__card">
            <header class="wizard__card-head">
              <h3>{{ t('wizard.source.section.target') }}</h3>
              <span class="wizard__card-tag">{{ t('wizard.source.editable') }}</span>
            </header>
            <div class="wizard__form">
              <label>{{ t('wizard.source.engineType') }}</label>
              <div class="wizard__engine-grid">
                <button
                  v-for="e in engineOptions"
                  :key="e.value"
                  type="button"
                  class="wizard__engine-chip"
                  :class="{ 'wizard__engine-chip--active': form.target.engine === e.value }"
                  @click="setTargetEngine(e.value)"
                >
                  <EngineTag :engine="e.value" icon-only />
                  <span>{{ e.label }}</span>
                </button>
              </div>

              <template v-if="form.target.engine === 'gaussdb'">
                <label class="required">{{ t('wizard.source.subMode._label') }}</label>
                <el-radio-group v-model="form.target.subMode" size="small">
                  <el-radio-button
                    v-for="sm in GAUSSDB_SUB_MODES"
                    :key="sm"
                    :label="sm"
                  >{{ t(`wizard.source.subMode.${sm}`) }}</el-radio-button>
                </el-radio-group>
                <small class="wizard__hint">{{ t('wizard.source.subMode.hint') }}</small>
              </template>

              <label class="required">{{ t('wizard.source.host') }}</label>
              <el-input v-model="form.target.host" placeholder="10.250.0.52:8000" />
              <small class="wizard__hint">{{ t('wizard.source.ipHint') }}</small>

              <div class="wizard__row wizard__row--switch">
                <label>{{ t('wizard.source.pdb') }}</label>
                <el-switch v-model="form.targetHasPdb" />
              </div>

              <label class="required">{{ t('wizard.source.username') }}</label>
              <el-input v-model="form.target.username" />
              <label class="required">{{ t('wizard.source.password') }}</label>
              <el-input v-model="form.target.password" type="password" show-password />
              <div class="wizard__row wizard__row--switch">
                <label>{{ t('wizard.source.ssl') }}</label>
                <el-switch v-model="form.target.ssl" />
              </div>
            </div>
          </section>
        </div>

        <section class="ape-dts-console-card wizard__card">
          <header class="wizard__card-head">
            <h3>{{ t('wizard.source.section.basic') }}</h3>
            <span class="wizard__card-tag wizard__card-tag--locked">{{ t('wizard.source.notEditable') }}</span>
          </header>
          <div class="wizard__form wizard__form--basic">
            <div class="wizard__row wizard__row--split">
              <div>
                <label class="required">{{ t('wizard.source.name') }}</label>
                <el-input v-model="form.name" />
              </div>
              <div>
                <label>{{ t('wizard.source.topology._label') }}</label>
                <el-radio-group v-model="form.taskType">
                  <el-radio-button label="standalone">{{ t('wizard.source.topology.standalone') }}</el-radio-button>
                  <el-radio-button label="primary_backup">{{ t('wizard.source.topology.primary_backup') }}</el-radio-button>
                </el-radio-group>
              </div>
            </div>
            <label>{{ t('wizard.source.desc._label') }}</label>
            <el-input
              v-model="form.description"
              type="textarea"
              :rows="2"
              :placeholder="t('wizard.source.desc.ph')"
            />
            <div class="wizard__row wizard__row--split">
              <div>
                <label class="required">{{ t('wizard.source.rg') }}</label>
                <el-select v-model="form.resourceGroup" style="width: 100%">
                  <el-option v-for="g in resourceGroups" :key="g" :label="g" :value="g" />
                </el-select>
              </div>
              <div>
                <label class="required">{{ t('wizard.source.instanceIp') }}</label>
                <el-select v-model="form.instanceIp" style="width: 100%">
                  <el-option label="127.0.0.1" value="127.0.0.1" />
                  <el-option label="127.0.0.2" value="127.0.0.2" />
                  <el-option label="127.0.0.3" value="127.0.0.3" />
                </el-select>
                <small class="wizard__hint">{{ t('wizard.source.instanceIpHint', { n: 4 }) }}</small>
              </div>
            </div>
          </div>
        </section>

        <section class="ape-dts-console-card wizard__card wizard__mode">
          <header class="wizard__card-head">
            <h3>{{ t('wizard.source.section.mode') }}</h3>
          </header>
          <div class="wizard__mode-grid">
            <button
              v-for="m in modeOptions"
              :key="m.value"
              type="button"
              class="wizard__mode-card"
              :class="{ 'wizard__mode-card--active': form.syncMode === m.value }"
              @click="form.syncMode = m.value"
            >
              <div class="wizard__mode-title">{{ m.label }}</div>
              <div class="wizard__mode-desc">{{ m.desc }}</div>
            </button>
          </div>
        </section>
      </template>

      <!-- STEP 2: Test connection -->
      <template v-else-if="currentStep?.key === 'test'">
        <el-alert type="info" :closable="false" show-icon class="wizard__alert">
          <template #default>
            <div class="wizard__alert-text">{{ t('wizard.test.hint') }}</div>
          </template>
        </el-alert>
        <div class="wizard__grid wizard__grid--2">
          <ConnectionTestCard
            :title="t('wizard.source.section.source')"
            :endpoint="form.source"
            :result="testState.source"
            @test="runTest('source')"
          />
          <ConnectionTestCard
            :title="t('wizard.source.section.target')"
            :endpoint="form.target"
            :result="testState.target"
            @test="runTest('target')"
          />
        </div>
        <section class="ape-dts-console-card wizard__card">
          <header class="wizard__card-head"><h3>{{ t('wizard.test.network') }}</h3></header>
          <div class="wizard__network">
            <div class="wizard__network-row">
              <span>{{ t('wizard.test.instanceIp') }}</span>
              <code>{{ form.instanceIp }}</code>
            </div>
          </div>
        </section>
      </template>

      <!-- STEP 3: Objects -->
      <template v-else-if="currentStep?.key === 'objects'">
        <section class="ape-dts-console-card wizard__card">
          <header class="wizard__card-head"><h3>{{ t('wizard.objects.section.rate') }}</h3></header>
          <div class="wizard__form">
            <div class="wizard__row wizard__row--inline">
              <label>{{ t('wizard.objects.rate._label') }}</label>
              <el-radio-group v-model="form.rate.mode">
                <el-radio-button label="limited">{{ t('wizard.objects.rate.on') }}</el-radio-button>
                <el-radio-button label="unlimited">{{ t('wizard.objects.rate.off') }}</el-radio-button>
              </el-radio-group>
              <div v-if="form.rate.mode === 'limited'" class="wizard__row-rate">
                <el-input-number v-model="form.rate.maxRps" :min="100" :max="100000" />
                <span class="wizard__hint">rows/s</span>
              </div>
            </div>
          </div>
        </section>

        <section class="ape-dts-console-card wizard__card">
          <header class="wizard__card-head"><h3>{{ t('wizard.objects.section.sync') }}</h3></header>
          <div class="wizard__form">
            <label>{{ t('wizard.objects.fullType._label') }}</label>
            <div class="wizard__row wizard__row--inline">
              <el-checkbox v-model="form.fullType.schema">{{ t('wizard.objects.fullType.schema') }}</el-checkbox>
              <el-checkbox v-model="form.fullType.data">{{ t('wizard.objects.fullType.data') }}</el-checkbox>
              <el-checkbox v-model="form.fullType.index">{{ t('wizard.objects.fullType.index') }}</el-checkbox>
            </div>
            <label>{{ t('wizard.objects.conflict._label') }}</label>
            <el-radio-group v-model="form.conflict">
              <el-radio-button label="ignore">{{ t('wizard.objects.conflict.ignore') }}</el-radio-button>
              <el-radio-button label="report">{{ t('wizard.objects.conflict.report') }}</el-radio-button>
              <el-radio-button label="overwrite">{{ t('wizard.objects.conflict.overwrite') }}</el-radio-button>
            </el-radio-group>
            <p v-if="form.conflict === 'overwrite'" class="wizard__warn-text">
              {{ t('wizard.objects.conflict.hint') }}
            </p>
          </div>
        </section>

        <section class="ape-dts-console-card wizard__card">
          <header class="wizard__card-head">
            <h3>{{ t('wizard.objects.pickerTitle') }}</h3>
            <el-tabs v-model="form.objects.pickerMode" class="wizard__picker-tabs">
              <el-tab-pane :label="t('wizard.objects.picker.manual')" name="manual" />
              <el-tab-pane :label="t('wizard.objects.picker.batch')" name="batch" />
            </el-tabs>
          </header>

          <el-alert type="warning" :closable="false" show-icon class="wizard__alert wizard__alert--tight">
            {{ t('wizard.objects.ddlNotice') }}
          </el-alert>

          <div class="wizard__picker">
            <div class="wizard__picker-col">
              <div class="wizard__picker-head">
                <el-checkbox v-model="availableAllChecked" :indeterminate="availableIndeterminate" @change="toggleAllAvailable">
                  {{ t('wizard.objects.available') }}
                </el-checkbox>
                <el-input v-model="availableSearch" :placeholder="t('wizard.objects.search')" size="small" style="width: 220px">
                  <template #prefix><IconSearch /></template>
                </el-input>
              </div>
              <el-tree
                ref="availableTreeRef"
                :data="availableTree"
                show-checkbox
                node-key="id"
                :filter-node-method="filterTreeNode"
                class="wizard__tree"
                @check="onAvailableCheck"
              />
            </div>
            <div class="wizard__picker-actions">
              <el-button circle @click="moveRight">
                <IconChevronRight />
              </el-button>
              <el-button circle @click="moveLeft">
                <IconChevronLeft />
              </el-button>
            </div>
            <div class="wizard__picker-col">
              <div class="wizard__picker-head">
                <el-checkbox v-model="selectedAllChecked" @change="toggleAllSelected">
                  {{ t('wizard.objects.selected') }} ({{ selectedTree.length }})
                </el-checkbox>
                <el-input v-model="selectedSearch" :placeholder="t('wizard.objects.search')" size="small" style="width: 220px">
                  <template #prefix><IconSearch /></template>
                </el-input>
              </div>
              <el-tree
                ref="selectedTreeRef"
                :data="selectedTree"
                show-checkbox
                node-key="id"
                :filter-node-method="filterSelectedNode"
                class="wizard__tree"
              />
            </div>
          </div>
        </section>
      </template>

      <!-- STEP 4: Processing -->
      <template v-else-if="currentStep?.key === 'processing'">
        <section class="ape-dts-console-card wizard__card">
          <div class="wizard__proc-head">
            <div class="wizard__proc-left">
              <el-button @click="removeInvalidRules">
                <template #icon><IconEraser /></template>
                {{ t('wizard.processing.removeInvalid') }}
              </el-button>
              <el-button type="primary" plain @click="addProcRule">
                <template #icon><IconPlus /></template>
                {{ t('wizard.processing.addRule') }}
              </el-button>
            </div>
            <el-input v-model="procSearch" :placeholder="t('wizard.processing.searchObj')" style="width: 280px">
              <template #prefix><IconSearch /></template>
            </el-input>
          </div>

          <el-table :data="filteredProcRules" class="wizard__table" empty-text="暂无表格数据">
            <el-table-column :label="t('wizard.processing.col.obj')" min-width="180">
              <template #default="{ row }">
                <span class="wizard__mono">{{ row.target }}</span>
              </template>
            </el-table-column>
            <el-table-column :label="t('wizard.processing.col.rule')" min-width="140">
              <template #default="{ row }">
                <el-select v-model="row.scope" size="small" style="width: 100%">
                  <el-option label="整表" value="table" />
                  <el-option label="选定列" value="columns" />
                </el-select>
              </template>
            </el-table-column>
            <el-table-column :label="t('wizard.processing.col.filter')" min-width="220">
              <template #default="{ row }">
                <el-input v-model="row.filter" size="small" placeholder="status = 'active'" />
              </template>
            </el-table-column>
            <el-table-column :label="t('wizard.processing.col.rules')" min-width="160">
              <template #default="{ row }">
                <el-select v-model="row.ruleType" size="small" style="width: 100%">
                  <el-option :label="t('wizard.processing.rule.where')" value="where" />
                  <el-option :label="t('wizard.processing.rule.columnMap')" value="map" />
                  <el-option :label="t('wizard.processing.rule.dropCol')" value="drop" />
                </el-select>
              </template>
            </el-table-column>
            <el-table-column :label="t('wizard.processing.col.dml')" width="200">
              <template #default="{ row }">
                <el-checkbox-group v-model="row.dml" class="wizard__dml-group">
                  <el-checkbox label="insert">INSERT</el-checkbox>
                  <el-checkbox label="update">UPDATE</el-checkbox>
                  <el-checkbox label="delete">DELETE</el-checkbox>
                </el-checkbox-group>
              </template>
            </el-table-column>
            <el-table-column :label="t('wizard.processing.col.colRules')" width="110" align="right">
              <template #default="{ row }">{{ row.colCount ?? 0 }}</template>
            </el-table-column>
            <el-table-column :label="t('wizard.processing.col.actions')" width="90">
              <template #default="{ $index }">
                <el-button link type="danger" @click="procRules.splice($index, 1)">
                  <IconTrash />
                </el-button>
              </template>
            </el-table-column>
          </el-table>
        </section>
      </template>

      <!-- STEP 5: Advanced -->
      <template v-else-if="currentStep?.key === 'advanced'">
        <section class="ape-dts-console-card wizard__card">
          <header class="wizard__card-head"><h3>{{ t('wizard.advanced.section.runtime') }}</h3></header>
          <div class="wizard__form wizard__form--2col">
            <div>
              <label>{{ t('wizard.advanced.parallelizer') }}</label>
              <el-select v-model="form.config.parallelizer" style="width: 100%">
                <el-option v-for="p in parallelizers" :key="p.value" :label="p.label" :value="p.value" />
              </el-select>
            </div>
            <div>
              <label>{{ t('wizard.advanced.parallelSize') }}</label>
              <el-input-number v-model="form.config.parallelSize" :min="1" :max="64" style="width: 100%" />
            </div>
            <div>
              <label>{{ t('wizard.advanced.bufferSize') }}</label>
              <el-input-number v-model="form.config.bufferSize" :min="1000" :max="200000" :step="1000" style="width: 100%" />
            </div>
            <div>
              <label>{{ t('wizard.advanced.checkpoint') }}</label>
              <el-input-number v-model="form.config.checkpointIntervalSecs" :min="1" :max="600" style="width: 100%" />
            </div>
            <div>
              <label>{{ t('wizard.advanced.maxRps') }}</label>
              <el-input-number v-model="form.config.maxRps" :min="0" :max="1000000" :step="500" style="width: 100%" />
              <small class="wizard__hint">0 表示不限速</small>
            </div>
            <div>
              <label>{{ t('wizard.advanced.resume._label') }}</label>
              <el-select v-model="form.config.resumeType" style="width: 100%">
                <el-option v-for="r in resumeOptions" :key="r.value" :label="r.label" :value="r.value" />
              </el-select>
            </div>
            <div class="wizard__row wizard__row--switch" style="grid-column: span 2">
              <label>{{ t('wizard.advanced.metrics') }}</label>
              <el-switch v-model="form.config.metricsEnabled" />
            </div>
            <div v-if="form.config.metricsEnabled">
              <label>{{ t('wizard.advanced.metricsHttp') }}</label>
              <el-input-number v-model="form.config.metricsHttpPort" :min="1024" :max="65535" style="width: 100%" />
            </div>
          </div>
        </section>
      </template>

      <!-- STEP 6: Precheck -->
      <template v-else-if="currentStep?.key === 'precheck'">
        <section class="ape-dts-console-card wizard__card">
          <div class="wizard__precheck-head">
            <div class="wizard__precheck-title">
              <IconSearch class="wizard__precheck-icon" />
              <div>
                <h3>{{ t('wizard.precheck.title') }}</h3>
                <p>{{ t('wizard.precheck.desc') }}</p>
              </div>
            </div>
            <div class="wizard__precheck-progress">
              <span class="wizard__precheck-pct">{{ precheckProgress }}%</span>
              <el-button :disabled="precheckRunning" @click="runPrecheck">
                <template #icon><IconRefresh /></template>
                {{ t('wizard.precheck.reset') }}
              </el-button>
            </div>
          </div>

          <el-progress :percentage="precheckProgress" :status="precheckResultStatus" :show-text="false" />

          <el-table :data="precheckItems" class="wizard__table">
            <el-table-column :label="t('wizard.precheck.col.idx')" type="index" width="60" />
            <el-table-column :label="t('wizard.precheck.col.item')" prop="title" min-width="220" />
            <el-table-column :label="t('wizard.precheck.col.type')" prop="group" width="180" />
            <el-table-column :label="t('wizard.precheck.col.result')" width="140">
              <template #default="{ row }">
                <span class="wizard__check-result" :class="`wizard__check-result--${row.result}`">
                  <IconLoader2 v-if="row.result === 'running'" class="wizard__spin" />
                  <IconCircleCheck v-else-if="row.result === 'pass'" />
                  <IconAlertTriangle v-else-if="row.result === 'warn'" />
                  <IconCircleX v-else-if="row.result === 'fail'" />
                  <IconClock v-else />
                  {{ t(`wizard.precheck.result.${row.result}`) }}
                </span>
              </template>
            </el-table-column>
            <el-table-column :label="t('wizard.precheck.col.action')" width="140">
              <template #default="{ row }">
                <el-button
                  v-if="row.result === 'warn' || row.result === 'fail'"
                  link
                  type="primary"
                >{{ t('wizard.precheck.action.detail') }}</el-button>
                <span v-else class="wizard__muted">—</span>
              </template>
            </el-table-column>
          </el-table>
        </section>
      </template>

      <!-- STEP 7: Confirm -->
      <template v-else-if="currentStep?.key === 'confirm'">
        <section class="ape-dts-console-card wizard__card">
          <header class="wizard__card-head">
            <h3>{{ t('wizard.confirm.section.basic') }}</h3>
          </header>
          <div class="wizard__conf-grid">
            <div><span>{{ t('wizard.confirm.field.name') }}</span><strong>{{ form.name }}</strong></div>
            <div><span>{{ t('wizard.confirm.field.createdAt') }}</span><strong>{{ nowLabel }}</strong></div>
          </div>
          <div class="wizard__form wizard__form--2col">
            <div>
              <label>{{ t('wizard.confirm.startTime') }}</label>
              <el-radio-group v-model="form.startMode">
                <el-radio-button label="now">{{ t('wizard.confirm.startNow') }}</el-radio-button>
                <el-radio-button label="later">{{ t('wizard.confirm.startLater') }}</el-radio-button>
              </el-radio-group>
            </div>
            <div class="wizard__row wizard__row--switch">
              <label>{{ t('wizard.confirm.delayThreshold') }}</label>
              <el-switch v-model="form.delayAlertEnabled" />
              <el-input-number
                v-if="form.delayAlertEnabled"
                v-model="form.delayAlertSecs"
                :min="1"
                :max="3600"
                size="small"
                style="width: 120px"
              />
            </div>
          </div>
        </section>

        <section class="ape-dts-console-card wizard__card">
          <header class="wizard__card-head">
            <h3>{{ t('wizard.confirm.section.taskInfo') }}</h3>
            <el-button link type="primary" @click="togglePreview">
              {{ showPreview ? '收起预览' : t('wizard.confirm.iniPreview') }}
            </el-button>
          </header>

          <div class="wizard__flow-viz">
            <div class="wizard__flow-node">
              <EngineTag :engine="form.source.engine" />
              <div class="wizard__flow-meta">
                <span>{{ form.source.host || '—' }}</span>
                <span>port {{ form.source.port }}</span>
              </div>
            </div>

            <div class="wizard__flow-middle">
              <div class="wizard__flow-line">
                <div class="wizard__flow-arrow-head" />
              </div>
              <div class="wizard__flow-details">
                <h4>{{ t('wizard.confirm.migrationSettings') }}</h4>
                <p><span>{{ t('wizard.confirm.migrationMode') }}</span><strong>{{ modeLabel(form.syncMode) }}</strong></p>
                <p><span>{{ t('wizard.confirm.rateMode') }}</span><strong>{{ form.rate.mode === 'limited' ? `${form.rate.maxRps} rows/s` : t('wizard.objects.rate.off') }}</strong></p>
                <h4>{{ t('wizard.confirm.taskInfo') }}</h4>
                <p><span>{{ t('wizard.source.name') }}</span><strong>{{ form.name }}</strong></p>
                <p><span>{{ t('wizard.source.desc._label') }}</span><strong>{{ form.description || '—' }}</strong></p>
                <p><span>{{ t('wizard.source.topology._label') }}</span><strong>{{ t(`wizard.source.topology.${form.taskType}`) }}</strong></p>
                <h4>{{ t('wizard.confirm.syncObjects') }}</h4>
                <p><span>{{ t('wizard.confirm.syncScope') }}</span><strong>{{ t('wizard.objects.tableLevel') }}</strong></p>
                <p><span>{{ t('wizard.confirm.syncObjectsCount') }}</span><strong>{{ selectedTree.reduce((n, db) => n + (db.children?.length ?? 0), 0) }}</strong></p>
              </div>
            </div>

            <div class="wizard__flow-node">
              <EngineTag :engine="form.target.engine" />
              <div class="wizard__flow-meta">
                <span>{{ form.target.host || '—' }}</span>
                <span>port {{ form.target.port }}</span>
              </div>
            </div>
          </div>

          <transition name="slide">
            <pre v-if="showPreview" class="wizard__ini">{{ iniPreview }}</pre>
          </transition>
        </section>
      </template>
    </div>

    <footer class="wizard__footer">
      <el-button v-if="current > 0" @click="current--">
        <IconArrowLeft /> {{ t('wizard.action.back') }}
      </el-button>
      <span class="wizard__footer-spacer" />
      <el-button v-if="current < steps.length - 1" type="primary" :disabled="!canProceed" @click="onNext">
        {{ t('wizard.action.next') }}
        <IconArrowRight />
      </el-button>
      <el-button
        v-else
        type="primary"
        :loading="submitting"
        :disabled="precheckProgress < 100"
        @click="onSubmit"
      >
        {{ form.startMode === 'now' ? t('wizard.confirm.submitStart') : t('wizard.confirm.submitLater') }}
      </el-button>
    </footer>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, reactive, ref } from 'vue';
import { useI18n } from 'vue-i18n';
import { useRoute, useRouter } from 'vue-router';
import { ElMessage } from 'element-plus';
import dayjs from 'dayjs';
import { api } from '@/api/client';
import type {
  EngineType,
  SyncMode,
  ParallelType,
  ResumeType,
  TaskCategory,
  GaussdbSubMode,
} from '@/types/domain';
import { ENGINE_LABELS, GAUSSDB_SUB_MODES } from '@/types/domain';
import {
  buildWizardSteps,
  defaultSubModeFor,
  requiresSubMode,
} from '@/composables/useWizardSteps';
import ConnectionTestCard from '@/components/wizard/ConnectionTestCard.vue';

const { t } = useI18n();
const route = useRoute();
const router = useRouter();

const category = computed<TaskCategory>(() => (route.params.type as TaskCategory) ?? 'snapshot');
const current = ref(0);

const steps = computed(() => buildWizardSteps(category.value, t));
const currentStep = computed(() => steps.value[current.value]);

const defaultMode: SyncMode = (route.query.mode as SyncMode) || 'snapshot_cdc';

const engineOptions = (Object.keys(ENGINE_LABELS) as EngineType[]).map((k) => ({
  value: k, label: ENGINE_LABELS[k],
}));

const resourceGroups = ['default', 'production', 'staging', 'dev'];
const parallelizers: { value: ParallelType; label: string }[] = [
  { value: 'snapshot', label: t('wizard.advanced.parallel.snapshot') },
  { value: 'rdb_merge', label: t('wizard.advanced.parallel.rdb_merge') },
  { value: 'rdb_partition', label: t('wizard.advanced.parallel.rdb_partition') },
  { value: 'rdb_check', label: t('wizard.advanced.parallel.rdb_check') },
  { value: 'serial', label: t('wizard.advanced.parallel.serial') },
  { value: 'table', label: t('wizard.advanced.parallel.table') },
];
const resumeOptions: { value: ResumeType; label: string }[] = [
  { value: 'from_log', label: t('wizard.advanced.resume.from_log') },
  { value: 'from_target', label: t('wizard.advanced.resume.from_target') },
  { value: 'from_db', label: t('wizard.advanced.resume.from_db') },
];

interface WizardForm {
  name: string;
  description: string;
  taskType: 'standalone' | 'primary_backup';
  resourceGroup: string;
  instanceIp: string;
  source: {
    engine: EngineType; subMode?: GaussdbSubMode; host: string; port: number; username: string;
    password: string; database: string; ssl: boolean;
  };
  target: {
    engine: EngineType; subMode?: GaussdbSubMode; host: string; port: number; username: string;
    password: string; database: string; ssl: boolean;
  };
  targetHasPdb: boolean;
  syncMode: SyncMode;
  rate: { mode: 'limited' | 'unlimited'; maxRps: number };
  fullType: { schema: boolean; data: boolean; index: boolean };
  conflict: 'ignore' | 'report' | 'overwrite';
  objects: { pickerMode: 'manual' | 'batch' };
  config: {
    parallelizer: ParallelType;
    parallelSize: number;
    bufferSize: number;
    checkpointIntervalSecs: number;
    maxRps: number;
    resumeType: ResumeType;
    metricsEnabled: boolean;
    metricsHttpPort: number;
  };
  startMode: 'now' | 'later';
  delayAlertEnabled: boolean;
  delayAlertSecs: number;
}

const form = reactive<WizardForm>({
  name: `apedts-${Math.random().toString(36).slice(2, 8).toUpperCase()}`,
  description: '',
  taskType: 'standalone',
  resourceGroup: 'default',
  instanceIp: '127.0.0.1',
  source: { engine: 'mysql', subMode: undefined, host: '', port: 3306, username: 'root', password: '', database: '', ssl: false },
  target: { engine: 'gaussdb', subMode: 'pg-mode', host: '', port: 5432, username: 'root', password: '', database: '', ssl: true },
  targetHasPdb: false,
  syncMode: defaultMode,
  rate: { mode: 'unlimited', maxRps: 10000 },
  fullType: { schema: true, data: true, index: false },
  conflict: 'overwrite',
  objects: { pickerMode: 'manual' },
  config: {
    parallelizer: 'snapshot', parallelSize: 4, bufferSize: 16000,
    checkpointIntervalSecs: 10, maxRps: 0, resumeType: 'from_log',
    metricsEnabled: true, metricsHttpPort: 9090,
  },
  startMode: 'now',
  delayAlertEnabled: false,
  delayAlertSecs: 60,
});

const modeOptions = computed(() => [
  { value: 'snapshot_cdc' as SyncMode, label: t('wizard.mode.snapshot_cdc.label'), desc: t('wizard.mode.snapshot_cdc.desc') },
  { value: 'snapshot' as SyncMode, label: t('wizard.mode.snapshot.label'), desc: t('wizard.mode.snapshot.desc') },
  { value: 'cdc' as SyncMode, label: t('wizard.mode.cdc.label'), desc: t('wizard.mode.cdc.desc') },
]);

function setSourceEngine(e: EngineType) {
  form.source.engine = e;
  form.source.port = defaultPort(e);
  form.source.subMode = defaultSubModeFor(e);
}
function setTargetEngine(e: EngineType) {
  form.target.engine = e;
  form.target.port = defaultPort(e);
  form.target.subMode = defaultSubModeFor(e);
}
function defaultPort(e: EngineType) {
  const map: Record<EngineType, number> = {
    mysql: 3306, tidb: 4000, postgres: 5432, gaussdb: 5432, oracle: 1521,
    mongo: 27017, redis: 6379, kafka: 9092, starrocks: 9030,
    clickhouse: 9000, doris: 9030, foxlake: 443,
  };
  return map[e] ?? 3306;
}

/* ---------- test connection ---------- */
const testState = reactive<{
  source: { status: 'idle' | 'running' | 'ok' | 'fail'; latency?: number; message?: string };
  target: { status: 'idle' | 'running' | 'ok' | 'fail'; latency?: number; message?: string };
}>({
  source: { status: 'idle' },
  target: { status: 'idle' },
});

async function runTest(which: 'source' | 'target') {
  const ep = form[which];
  testState[which] = { status: 'running' };
  try {
    const res = await api.post<{ ok: boolean; latencyMs: number; message?: string }>(
      '/tasks/test-connection', { endpoint: ep },
    );
    if (res.ok) testState[which] = { status: 'ok', latency: res.latencyMs };
    else testState[which] = { status: 'fail', message: res.message };
  } catch (err) {
    testState[which] = { status: 'fail', message: String(err) };
  }
}

/* ---------- object picker (step 3) ---------- */
interface ObjNode { id: string; label: string; children?: ObjNode[]; leaf?: boolean }

const availableTree = ref<ObjNode[]>(buildMockSchemaTree());
const selectedTree = ref<ObjNode[]>([]);
const availableTreeRef = ref<any>(null);
const selectedTreeRef = ref<any>(null);
const availableSearch = ref('');
const selectedSearch = ref('');
const availableAllChecked = ref(false);
const availableIndeterminate = ref(false);
const selectedAllChecked = ref(false);

function buildMockSchemaTree(): ObjNode[] {
  const dbs = ['confluence', 'datax_web', 'db_ck', 'db_test', 'orders_db'];
  const tablesPerDb: Record<string, string[]> = {
    confluence: ['spaces', 'pages', 'users', 'comments'],
    datax_web: ['jobs', 'triggers', 'logs'],
    db_ck: ['events', 'metrics'],
    db_test: ['t1', 't2', 't3', 't4'],
    orders_db: ['orders', 'order_items', 'payments', 'shipments'],
  };
  return dbs.map((db) => ({
    id: `db:${db}`,
    label: `database ${db}`,
    children: tablesPerDb[db].map((t) => ({
      id: `${db}.${t}`,
      label: t,
      leaf: true,
    })),
  }));
}

function filterTreeNode(value: string, data: Record<string, unknown>) {
  if (!value) return true;
  const label = data.label;
  return typeof label === 'string' && label.toLowerCase().includes(value.toLowerCase());
}
function filterSelectedNode(value: string, data: Record<string, unknown>) {
  if (!value) return true;
  const label = data.label;
  return typeof label === 'string' && label.toLowerCase().includes(value.toLowerCase());
}

watchEffect(() => {
  availableTreeRef.value?.filter?.(availableSearch.value);
  selectedTreeRef.value?.filter?.(selectedSearch.value);
});

function onAvailableCheck() {
  const checked = availableTreeRef.value?.getCheckedKeys?.() ?? [];
  availableIndeterminate.value = checked.length > 0 && checked.length < flattenLeaves(availableTree.value).length;
  availableAllChecked.value = checked.length === flattenLeaves(availableTree.value).length && checked.length > 0;
}

function flattenLeaves(tree: ObjNode[]): string[] {
  const out: string[] = [];
  for (const n of tree) {
    if (n.leaf) out.push(n.id);
    else if (n.children) out.push(...flattenLeaves(n.children));
  }
  return out;
}

function toggleAllAvailable(v: unknown) {
  if (v) availableTreeRef.value?.setCheckedKeys?.(flattenLeaves(availableTree.value));
  else availableTreeRef.value?.setCheckedKeys?.([]);
}
function toggleAllSelected(v: unknown) {
  if (v) selectedTreeRef.value?.setCheckedKeys?.(flattenLeaves(selectedTree.value));
  else selectedTreeRef.value?.setCheckedKeys?.([]);
}

function moveRight() {
  const checked = availableTreeRef.value?.getCheckedKeys?.(true) ?? [];
  if (!checked.length) return;
  const toMove: ObjNode[] = [];
  for (const db of availableTree.value) {
    if (!db.children) continue;
    const kept: ObjNode[] = [];
    const moved: ObjNode[] = [];
    for (const tbl of db.children) {
      if (checked.includes(tbl.id)) moved.push(tbl);
      else kept.push(tbl);
    }
    if (moved.length) {
      toMove.push({ id: db.id, label: db.label, children: moved });
    }
    db.children = kept;
  }
  availableTree.value = availableTree.value.filter((d) => (d.children?.length ?? 0) > 0);

  for (const dbNode of toMove) {
    const existing = selectedTree.value.find((d) => d.id === dbNode.id);
    if (existing) {
      existing.children = [...(existing.children ?? []), ...(dbNode.children ?? [])];
    } else {
      selectedTree.value.push(dbNode);
    }
  }
  availableTreeRef.value?.setCheckedKeys?.([]);
  onAvailableCheck();
  addProcRulesFor(toMove);
}

function moveLeft() {
  const checked = selectedTreeRef.value?.getCheckedKeys?.(true) ?? [];
  if (!checked.length) return;
  const toMove: ObjNode[] = [];
  for (const db of selectedTree.value) {
    if (!db.children) continue;
    const kept: ObjNode[] = [];
    const moved: ObjNode[] = [];
    for (const tbl of db.children) {
      if (checked.includes(tbl.id)) moved.push(tbl);
      else kept.push(tbl);
    }
    if (moved.length) toMove.push({ id: db.id, label: db.label, children: moved });
    db.children = kept;
  }
  selectedTree.value = selectedTree.value.filter((d) => (d.children?.length ?? 0) > 0);
  for (const dbNode of toMove) {
    const existing = availableTree.value.find((d) => d.id === dbNode.id);
    if (existing) existing.children = [...(existing.children ?? []), ...(dbNode.children ?? [])];
    else availableTree.value.push(dbNode);
  }
  selectedTreeRef.value?.setCheckedKeys?.([]);
  procRules.value = procRules.value.filter((r) => !checked.includes(r.target));
}

/* ---------- processing (step 4) ---------- */
interface ProcRule {
  target: string;
  scope: 'table' | 'columns';
  filter: string;
  ruleType: 'where' | 'map' | 'drop';
  dml: string[];
  colCount: number;
}

const procRules = ref<ProcRule[]>([]);
const procSearch = ref('');

function addProcRulesFor(dbs: ObjNode[]) {
  for (const db of dbs) {
    for (const tbl of db.children ?? []) {
      if (!procRules.value.some((r) => r.target === tbl.id)) {
        procRules.value.push({
          target: tbl.id,
          scope: 'table',
          filter: '',
          ruleType: 'where',
          dml: ['insert', 'update', 'delete'],
          colCount: 0,
        });
      }
    }
  }
}

function addProcRule() {
  const available = selectedTree.value.flatMap((db) => db.children ?? []);
  const candidate = available.find((t) => !procRules.value.some((r) => r.target === t.id));
  if (!candidate) {
    ElMessage.info('所有已选对象均已添加规则');
    return;
  }
  procRules.value.push({
    target: candidate.id, scope: 'table', filter: '',
    ruleType: 'where', dml: ['insert', 'update', 'delete'], colCount: 0,
  });
}

function removeInvalidRules() {
  const valid = new Set(selectedTree.value.flatMap((db) => (db.children ?? []).map((t) => t.id)));
  procRules.value = procRules.value.filter((r) => valid.has(r.target));
  ElMessage.success('已剔除无效规则');
}

const filteredProcRules = computed(() => {
  if (!procSearch.value) return procRules.value;
  return procRules.value.filter((r) => r.target.includes(procSearch.value));
});

/* ---------- precheck (step 6) ---------- */
interface PrecheckItem {
  key: string; title: string; group: string;
  result: 'pending' | 'running' | 'pass' | 'warn' | 'fail';
}

const precheckItems = ref<PrecheckItem[]>([]);
const precheckProgress = ref(0);
const precheckRunning = ref(false);
const precheckResultStatus = computed<'success' | 'exception' | 'warning' | ''>(() => {
  if (precheckItems.value.some((i) => i.result === 'fail')) return 'exception';
  if (precheckItems.value.some((i) => i.result === 'warn')) return 'warning';
  if (precheckProgress.value >= 100) return 'success';
  return '';
});

function seedPrecheckItems() {
  const groups = [
    ['目标库磁盘空间检查', '目标库磁盘空间检查'],
    ['查看 IP 映射关系、任务类型是否占无限制', '数据库参数检查'],
    ['源端是否支持打开审计相关操作', '数据库参数检查'],
    ['检查源数据库的 max_allowed_packet 参数', '数据库参数检查'],
    ['源数据库连接、用户权限检查', '数据库参数检查'],
    ['源数据库 binlog 格式检查', '数据库参数检查'],
    ['源数据库 binlog_row_image 参数是否为 FULL', '数据库参数检查'],
    ['源数据库 binlog 日志是否存在', '数据库参数检查'],
    ['源数据库库名是否合法', '数据库参数检查'],
    ['源数据库 server_id 是否存在位移数不符合要求', '数据库参数检查'],
    ['源数据库和目标数据库表名大小写敏感性检查', '数据库参数检查'],
    ['源数据库 GTID 功能检查', '数据库参数检查'],
    ['源服务机器是否在运行', '数据库参数检查'],
    ['目标数据库存在与源数据库同为的名对象', '数据库参数检查'],
    ['源库存在不支持的结构同步对象', '数据库参数检查'],
    ['源数据库或被映射的名合是否含特殊字符', '数据库参数检查'],
    ['表结构一致性检查', '数据库参数检查'],
    ['源数据库选择复有检查', '数据库参数检查'],
    ['目标数据库用户权限是否足够', '数据库用户权限检查'],
    ['源数据库用户权限是否足够', '数据库用户权限检查'],
    ['源数据库基本是否支持', '数据库基本检查'],
    ['目标数据库连接是否成功', '网络情况'],
    ['源数据库连接是否成功', '网络情况'],
  ];
  precheckItems.value = groups.map(([title, group], idx) => ({
    key: `chk-${idx}`,
    title: title as string,
    group: group as string,
    result: 'pending',
  }));
}

async function runPrecheck() {
  precheckRunning.value = true;
  precheckProgress.value = 0;
  seedPrecheckItems();
  const items = precheckItems.value;
  for (let i = 0; i < items.length; i++) {
    items[i].result = 'running';
    await new Promise((r) => setTimeout(r, 160));
    const rand = Math.random();
    items[i].result = rand > 0.96 ? 'fail' : rand > 0.88 ? 'warn' : 'pass';
    precheckProgress.value = Math.round(((i + 1) / items.length) * 100);
  }
  precheckRunning.value = false;
  if (items.every((i) => i.result === 'pass')) ElMessage.success('预检查通过');
  else ElMessage.warning('预检查完成，存在需关注项');
}

/* ---------- confirm (step 7) ---------- */
const showPreview = ref(false);
const iniPreview = ref('');
async function togglePreview() {
  if (!showPreview.value) {
    const res = await api.post<{ ini: string }>('/tasks/preview-ini', formToTaskDraft());
    iniPreview.value = res.ini;
  }
  showPreview.value = !showPreview.value;
}

function modeLabel(m: SyncMode) {
  return t(`wizard.mode.${m}.label`);
}
const nowLabel = computed(() => dayjs().format('YYYY/MM/DD HH:mm:ss'));

function formToTaskDraft() {
  const extractType = syncModeToExtractType(form.syncMode, category.value);
  const selectedDbs = selectedTree.value.map((db) => db.label?.replace('database ', '') ?? db.id.replace('db:', ''));
  return {
    name: form.name,
    description: form.description,
    category: category.value,
    source: { ...form.source },
    target: { ...form.target },
    syncMode: form.syncMode,
    extractType,
    taskType: form.taskType,
    resourceGroup: form.resourceGroup,
    instanceIp: form.instanceIp,
    syncObjects: {
      totalTables: selectedTree.value.reduce((n, db) => n + (db.children?.length ?? 0), 0),
      selectedTables: selectedTree.value.reduce((n, db) => n + (db.children?.length ?? 0), 0),
    },
    config: form.config,
    filter: {
      doDbs: selectedDbs.length > 0 ? selectedDbs : undefined,
      doTbs: selectedTree.value.flatMap((db) => (db.children ?? []).map((t) => t.id)).length > 0
        ? selectedTree.value.flatMap((db) => (db.children ?? []).map((t) => t.id))
        : undefined,
    },
  };
}

function syncModeToExtractType(mode: SyncMode, cat: TaskCategory) {
  if (cat === 'struct') return 'struct' as const;
  if (cat === 'check') return 'snapshot' as const;
  if (mode === 'snapshot') return 'snapshot' as const;
  if (mode === 'cdc') return 'cdc' as const;
  return 'snapshot_and_cdc' as const;
}

/* ---------- navigation ---------- */
function endpointSubModeOk(side: 'source' | 'target') {
  return !requiresSubMode(form[side].engine) || !!form[side].subMode;
}

const canProceed = computed(() => {
  const key = currentStep.value?.key;
  if (key === 'source') {
    return (
      !!form.source.host &&
      !!form.target.host &&
      !!form.name &&
      endpointSubModeOk('source') &&
      endpointSubModeOk('target')
    );
  }
  if (key === 'test') {
    return testState.source.status === 'ok' && testState.target.status === 'ok';
  }
  if (key === 'precheck') {
    return precheckProgress.value >= 100;
  }
  return true;
});

function onNext() {
  const key = currentStep.value?.key;
  if (!canProceed.value) {
    if (key === 'source' && (!endpointSubModeOk('source') || !endpointSubModeOk('target'))) {
      ElMessage.warning(t('wizard.source.subMode.required'));
      return;
    }
    if (key === 'test') ElMessage.warning(t('wizard.action.testFirst'));
    if (key === 'precheck') ElMessage.warning(t('wizard.action.precheckFirst'));
    return;
  }
  current.value++;
  if (currentStep.value?.key === 'precheck') {
    runPrecheck();
  }
}

function stepClass(idx: number) {
  return {
    'wizard__step--active': idx === current.value,
    'wizard__step--done': idx < current.value,
    'wizard__step--future': idx > current.value,
  };
}

function gotoStep(idx: number) {
  // Only allow backwards navigation freely; forwards respects proceed check.
  if (idx <= current.value) current.value = idx;
}

function onBack() {
  router.push({ path: `/tasks/${category.value}` });
}

/* ---------- submit ---------- */
const submitting = ref(false);
async function onSubmit() {
  submitting.value = true;
  try {
    await api.post('/tasks', formToTaskDraft());
    ElMessage.success(form.startMode === 'now' ? t('wizard.toast.created') : t('wizard.toast.createdLater'));
    router.push({ path: `/tasks/${category.value}` });
  } catch {
    ElMessage.error('创建失败');
  } finally {
    submitting.value = false;
  }
}

onMounted(() => {
  // apply pre-selected mode from route query (from dashboard / list modal)
  if (route.query.mode) form.syncMode = route.query.mode as SyncMode;
});
</script>

<style scoped>
.wizard {
  display: flex;
  flex-direction: column;
  min-height: 100%;
  background: var(--color-canvas);
}
.wizard__header {
  background: var(--color-surface);
  border-bottom: 1px solid var(--color-border);
}
.wizard__header-inner {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 14px 24px;
}
.wizard__back {
  color: var(--color-ink-muted);
}
.wizard__sep {
  color: var(--color-border);
}
.wizard__title {
  margin: 0;
  font-size: var(--text-xl);
  font-weight: 600;
  color: var(--color-ink);
}
.wizard__steps {
  background: var(--color-surface);
  border-bottom: 1px solid var(--color-border);
  padding: 18px 24px;
}
.wizard__steps ol {
  display: flex;
  align-items: center;
  justify-content: flex-start;
  gap: 0;
  margin: 0;
  padding: 0;
  list-style: none;
  flex-wrap: wrap;
}
.wizard__steps li {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  color: var(--color-ink-faint);
  font-size: 13px;
  cursor: pointer;
  position: relative;
  padding-right: 20px;
}
.wizard__step-badge {
  width: 22px;
  height: 22px;
  border-radius: 50%;
  border: 1px solid var(--color-border-strong);
  display: inline-flex;
  align-items: center;
  justify-content: center;
  background: var(--color-surface);
  color: var(--color-ink-faint);
  font-size: 12px;
  font-weight: 600;
  flex-shrink: 0;
}
.wizard__step-label {
  white-space: nowrap;
}
.wizard__step-bar {
  width: 36px;
  height: 1px;
  background: var(--color-border);
  margin: 0 4px;
}
.wizard__step--active {
  color: var(--color-primary-700);
  font-weight: 500;
}
.wizard__step--active .wizard__step-badge {
  background: var(--color-primary-600);
  color: #fff;
  border-color: var(--color-primary-600);
}
.wizard__step--done {
  color: var(--color-ink-muted);
}
.wizard__step--done .wizard__step-badge {
  background: var(--color-primary-50);
  color: var(--color-primary-700);
  border-color: var(--color-primary-200);
}
.wizard__body {
  flex: 1;
  padding: 20px 24px;
  display: flex;
  flex-direction: column;
  gap: 16px;
}
.wizard__grid {
  display: grid;
  gap: 16px;
}
.wizard__grid--2 {
  grid-template-columns: repeat(2, minmax(0, 1fr));
}
@media (max-width: 1200px) {
  .wizard__grid--2 { grid-template-columns: 1fr; }
}
.wizard__alert {
  border-radius: var(--radius-md);
}
.wizard__alert--tight {
  margin: 12px 0 0;
}
.wizard__alert-text {
  white-space: pre-line;
  font-size: 13px;
  color: var(--color-ink-muted);
}
.wizard__card {
  padding: 0;
  display: flex;
  flex-direction: column;
}
.wizard__card-head {
  padding: 14px 20px;
  border-bottom: 1px solid var(--color-border);
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}
.wizard__card-head h3 {
  margin: 0;
  font-size: 14px;
  font-weight: 600;
  color: var(--color-ink);
}
.wizard__card-tag {
  font-size: 11px;
  padding: 2px 8px;
  background: var(--color-success-soft);
  color: var(--color-success);
  border-radius: 999px;
  font-weight: 500;
}
.wizard__card-tag--locked {
  background: var(--color-danger-soft);
  color: var(--color-danger);
}
.wizard__form {
  padding: 16px 20px 18px;
  display: flex;
  flex-direction: column;
  gap: 10px;
}
.wizard__form--2col {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 12px 16px;
}
.wizard__form label {
  font-size: 13px;
  color: var(--color-ink-muted);
  margin-bottom: 2px;
}
.wizard__form label.required::before {
  content: '*';
  color: var(--color-danger);
  margin-right: 4px;
}
.wizard__hint {
  color: var(--color-ink-faint);
  font-size: 12px;
}
.wizard__warn-text {
  margin: 0;
  color: var(--color-danger);
  font-size: 12px;
}
.wizard__engine-grid {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
}
.wizard__engine-chip {
  all: unset;
  cursor: pointer;
  padding: 6px 10px;
  border: 1px solid var(--color-border);
  border-radius: var(--radius);
  display: inline-flex;
  align-items: center;
  gap: 6px;
  font-size: 12px;
  color: var(--color-ink-muted);
  background: var(--color-surface);
  transition: all var(--dur) var(--ease-soft);
}
.wizard__engine-chip:hover {
  border-color: var(--color-primary-500);
}
.wizard__engine-chip--active {
  background: var(--color-primary-50);
  border-color: var(--color-primary-600);
  color: var(--color-primary-700);
}
.wizard__row {
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.wizard__row--split {
  flex-direction: row;
  gap: 16px;
}
.wizard__row--split > div {
  flex: 1;
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.wizard__row--switch {
  flex-direction: row;
  align-items: center;
  gap: 12px;
}
.wizard__row--inline {
  flex-direction: row;
  align-items: center;
  gap: 12px;
  flex-wrap: wrap;
}
.wizard__row-rate {
  display: inline-flex;
  align-items: center;
  gap: 8px;
}
.wizard__mode {
  overflow: visible;
}
.wizard__mode-grid {
  display: grid;
  grid-template-columns: repeat(3, 1fr);
  gap: 0;
  padding: 0;
}
.wizard__mode-card {
  all: unset;
  cursor: pointer;
  padding: 20px;
  border-right: 1px solid var(--color-border);
  transition: all var(--dur) var(--ease-soft);
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 8px;
  text-align: center;
}
.wizard__mode-card:last-child { border-right: none; }
.wizard__mode-card:hover { background: var(--color-surface-2); }
.wizard__mode-card--active {
  background: var(--color-primary-50);
  color: var(--color-primary-700);
  box-shadow: inset 0 -2px 0 var(--color-primary-600);
}
.wizard__mode-title {
  font-weight: 600;
  font-size: 14px;
}
.wizard__mode-desc {
  font-size: 12px;
  color: var(--color-ink-subtle);
}
.wizard__mode-card--active .wizard__mode-desc {
  color: var(--color-primary-700);
  opacity: 0.8;
}
.wizard__network {
  padding: 14px 20px;
}
.wizard__network-row {
  display: flex;
  align-items: center;
  gap: 16px;
  font-size: 13px;
  color: var(--color-ink-muted);
}
.wizard__network-row code {
  background: var(--color-surface-2);
  padding: 2px 6px;
  border-radius: var(--radius-sm);
  font-family: var(--font-mono);
}
.wizard__picker {
  display: grid;
  grid-template-columns: 1fr 48px 1fr;
  gap: 12px;
  padding: 12px 20px 18px;
}
.wizard__picker-col {
  border: 1px solid var(--color-border);
  border-radius: var(--radius-md);
  background: var(--color-surface-2);
  display: flex;
  flex-direction: column;
  max-height: 360px;
}
.wizard__picker-head {
  padding: 8px 10px;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  border-bottom: 1px solid var(--color-border);
  background: var(--color-surface);
}
.wizard__picker-actions {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 10px;
}
.wizard__tree {
  flex: 1;
  padding: 8px;
  overflow: auto;
  background: var(--color-surface);
  border-bottom-left-radius: var(--radius-md);
  border-bottom-right-radius: var(--radius-md);
}
.wizard__picker-tabs {
  margin: 0;
}
.wizard__picker-tabs :deep(.el-tabs__header) { margin: 0; }
.wizard__proc-head {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 12px 20px;
  gap: 12px;
}
.wizard__proc-left {
  display: inline-flex;
  gap: 8px;
}
.wizard__dml-group {
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.wizard__table {
  border-top: 1px solid var(--color-border);
}
.wizard__mono {
  font-family: var(--font-mono);
  font-size: 12px;
  color: var(--color-ink);
}
.wizard__precheck-head {
  padding: 16px 20px;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}
.wizard__precheck-title {
  display: flex;
  align-items: center;
  gap: 12px;
}
.wizard__precheck-icon {
  width: 32px;
  height: 32px;
  padding: 6px;
  background: var(--color-primary-50);
  color: var(--color-primary-700);
  border-radius: 50%;
}
.wizard__precheck-title h3 { margin: 0; font-size: 14px; }
.wizard__precheck-title p { margin: 4px 0 0; font-size: 12px; color: var(--color-ink-subtle); max-width: 720px; line-height: 1.5; }
.wizard__precheck-progress {
  display: inline-flex;
  align-items: center;
  gap: 12px;
}
.wizard__precheck-pct {
  font-size: 22px;
  font-weight: 600;
  color: var(--color-primary-700);
  font-variant-numeric: tabular-nums;
}
.wizard__check-result {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  font-size: 13px;
}
.wizard__check-result--pending { color: var(--color-ink-subtle); }
.wizard__check-result--running { color: var(--color-info); }
.wizard__check-result--pass { color: var(--color-success); }
.wizard__check-result--warn { color: var(--color-warning); }
.wizard__check-result--fail { color: var(--color-danger); }
.wizard__muted { color: var(--color-ink-faint); }
.wizard__spin { animation: spin 1s linear infinite; }
@keyframes spin { from { transform: rotate(0); } to { transform: rotate(360deg); } }

.wizard__conf-grid {
  padding: 16px 20px;
  display: grid;
  grid-template-columns: repeat(2, 1fr);
  gap: 12px 24px;
  border-bottom: 1px solid var(--color-border);
}
.wizard__conf-grid > div {
  display: flex;
  gap: 8px;
  font-size: 13px;
}
.wizard__conf-grid span { color: var(--color-ink-subtle); min-width: 120px; }
.wizard__conf-grid strong { color: var(--color-ink); font-weight: 500; }
.wizard__flow-viz {
  display: grid;
  grid-template-columns: 1fr 2fr 1fr;
  gap: 12px;
  padding: 24px;
  align-items: center;
}
.wizard__flow-node {
  background: var(--color-primary-50);
  border: 1px solid var(--color-primary-200);
  border-radius: var(--radius-md);
  padding: 16px;
  display: flex;
  flex-direction: column;
  gap: 8px;
  align-items: center;
  min-height: 160px;
  justify-content: center;
}
.wizard__flow-meta {
  display: flex;
  flex-direction: column;
  gap: 2px;
  align-items: center;
  font-size: 12px;
  color: var(--color-ink-muted);
  font-family: var(--font-mono);
}
.wizard__flow-middle {
  position: relative;
  padding: 0 20px;
  min-height: 160px;
}
.wizard__flow-line {
  position: absolute;
  left: 0;
  right: 0;
  top: 16px;
  height: 2px;
  background: linear-gradient(to right, var(--color-primary-500), var(--color-accent));
}
.wizard__flow-arrow-head {
  position: absolute;
  right: 0;
  top: -4px;
  width: 0;
  height: 0;
  border-left: 10px solid var(--color-accent);
  border-top: 5px solid transparent;
  border-bottom: 5px solid transparent;
}
.wizard__flow-details {
  padding-top: 40px;
  font-size: 12px;
  display: flex;
  flex-direction: column;
  gap: 6px;
  background: var(--color-surface);
  border-radius: var(--radius-md);
  padding: 12px;
  margin-top: 30px;
  border: 1px solid var(--color-border);
}
.wizard__flow-details h4 {
  margin: 6px 0 4px;
  font-size: 12px;
  font-weight: 600;
  color: var(--color-ink);
  border-bottom: 1px dashed var(--color-border);
  padding-bottom: 4px;
}
.wizard__flow-details p {
  margin: 0;
  display: flex;
  justify-content: space-between;
  gap: 12px;
}
.wizard__flow-details span { color: var(--color-ink-subtle); }
.wizard__flow-details strong { color: var(--color-ink); font-weight: 500; }
.wizard__ini {
  margin: 0 20px 20px;
  padding: 14px;
  background: #0F172A;
  color: #E2E8F0;
  border-radius: var(--radius-md);
  font-family: var(--font-mono);
  font-size: 12px;
  max-height: 360px;
  overflow: auto;
  white-space: pre;
}
.wizard__footer {
  position: sticky;
  bottom: 0;
  background: var(--color-surface);
  border-top: 1px solid var(--color-border);
  padding: 12px 24px;
  display: flex;
  align-items: center;
  gap: 12px;
  z-index: 5;
}
.wizard__footer-spacer { flex: 1; }
.slide-enter-active, .slide-leave-active {
  transition: all var(--dur-slow) var(--ease-soft);
  max-height: 400px;
  overflow: hidden;
}
.slide-enter-from, .slide-leave-to {
  max-height: 0;
  opacity: 0;
}
</style>
