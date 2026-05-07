<template>
  <div class="wizard">
    <header class="wizard__header">
      <div class="wizard__header-inner">
        <el-button link class="wizard__back" @click="onBack">
          <IconArrowLeft /> {{ t('wizard.header.back') }}
        </el-button>
        <span class="wizard__sep">|</span>
        <h1 class="wizard__title">{{ t('wizard.header.create', { type: t(`task.type.${category}`) }) }}</h1>
        <span class="wizard__footer-spacer" />
        <el-button v-if="draftStore.isDirty(category)" type="danger" plain size="small" @click="onDiscardDraft">
          {{ t('wizard.draft.discard') }}
        </el-button>
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
                  <el-input
                    v-model="form.source.host"
                    placeholder="192.168.1.116"
                    @paste="onSourcePaste"
                  />
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
              <el-input
                v-model="form.target.host"
                placeholder="10.250.0.52:8000"
                @paste="onTargetPaste"
              />
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
                <el-input
                  v-model="form.taskName"
                  @blur="checkTaskIdUnique"
                >
                  <template #suffix>
                    <span v-if="taskIdError" class="wizard__field-error">{{ taskIdError }}</span>
                  </template>
                </el-input>
                <small class="wizard__hint">{{ t('wizard.source.taskIdHint') }}</small>
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

        <section v-if="showSyncMode" class="ape-dts-console-card wizard__card wizard__mode">
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
          <header class="wizard__card-head">
            <h3>{{ t('wizard.test.network') }}</h3>
            <el-switch
              v-if="canBypassTest"
              v-model="bypassTest"
              :active-text="t('wizard.test.bypass')"
              style="margin-left: auto"
            />
          </header>
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
              <el-checkbox v-model="form.fullType.schema" :disabled="category === 'struct'">{{ t('wizard.objects.fullType.schema') }}</el-checkbox>
              <el-checkbox v-model="form.fullType.data" :disabled="category === 'struct'">{{ t('wizard.objects.fullType.data') }}</el-checkbox>
              <el-checkbox v-model="form.fullType.index" :disabled="category === 'struct'">{{ t('wizard.objects.fullType.index') }}</el-checkbox>
            </div>
            <label>{{ t('wizard.objects.conflict._label') }}</label>
            <el-radio-group v-model="form.conflict">
              <el-radio-button label="insert">{{ t('wizard.objects.conflict.insert') }}</el-radio-button>
              <el-radio-button label="replace">{{ t('wizard.objects.conflict.replace') }}</el-radio-button>
              <el-radio-button label="ignore">{{ t('wizard.objects.conflict.ignore') }}</el-radio-button>
            </el-radio-group>
          </div>
        </section>

        <section class="ape-dts-console-card wizard__card">
          <header class="wizard__card-head"><h3>{{ t('wizard.objects.pickerTitle') }}</h3></header>
          <div class="wizard__form">
            <label>{{ t('wizard.objects.doDbs') }}</label>
            <el-input v-model="form.filter.doDbs" :placeholder="t('wizard.objects.wildcardHint')" />
            <label>{{ t('wizard.objects.doTbs') }}</label>
            <el-input v-model="form.filter.doTbs" :placeholder="t('wizard.objects.wildcardHint')" />
            <label>{{ t('wizard.objects.ignoreDbs') }}</label>
            <el-input v-model="form.filter.ignoreDbs" :placeholder="t('wizard.objects.wildcardHint')" />
            <label>{{ t('wizard.objects.ignoreTbs') }}</label>
            <el-input v-model="form.filter.ignoreTbs" :placeholder="t('wizard.objects.wildcardHint')" />
          </div>
        </section>
      </template>

      <!-- STEP 4: Processing (not for struct) -->
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
            <el-table-column :label="t('wizard.processing.col.dml')" width="200">
              <template #default="{ row }">
                <el-checkbox-group v-model="row.doEvents" class="wizard__dml-group">
                  <el-checkbox label="insert">INSERT</el-checkbox>
                  <el-checkbox label="update">UPDATE</el-checkbox>
                  <el-checkbox label="delete">DELETE</el-checkbox>
                </el-checkbox-group>
              </template>
            </el-table-column>
            <el-table-column :label="t('wizard.processing.col.filter')" min-width="220">
              <template #default="{ row }">
                <el-input v-model="row.where" size="small" placeholder="status = 'active'" />
              </template>
            </el-table-column>
            <el-table-column label="ignore_cols" min-width="180">
              <template #default="{ row }">
                <el-input v-model="row.ignoreCols" size="small" placeholder="col1,col2" />
              </template>
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

        <!-- Router maps -->
        <section class="ape-dts-console-card wizard__card">
          <header class="wizard__card-head"><h3>{{ t('wizard.processing.router._label') }}</h3></header>
          <div class="wizard__form wizard__form--2col">
            <div>
              <label>{{ t('wizard.processing.router.dbMap') }}</label>
              <el-input v-model="form.router.dbMap" type="textarea" :rows="2" placeholder="src_db:dst_db" />
            </div>
            <div>
              <label>{{ t('wizard.processing.router.tbMap') }}</label>
              <el-input v-model="form.router.tbMap" type="textarea" :rows="2" placeholder="src_db.t1:dst_db.t1" />
            </div>
            <div>
              <label>{{ t('wizard.processing.router.colMap') }}</label>
              <el-input v-model="form.router.colMap" type="textarea" :rows="2" placeholder="src_db.t1.c1:dst_db.t1.c1" />
            </div>
            <div v-if="form.target.engine === 'kafka'">
              <label>{{ t('wizard.processing.router.topicMap') }}</label>
              <el-input v-model="form.router.topicMap" type="textarea" :rows="2" placeholder="*.*:default_topic" />
            </div>
          </div>
        </section>

        <!-- Lua processor -->
        <section class="ape-dts-console-card wizard__card">
          <header class="wizard__card-head"><h3>{{ t('wizard.processing.lua._label') }}</h3></header>
          <div class="wizard__form">
            <el-radio-group v-model="luaMode">
              <el-radio-button label="none">{{ t('wizard.processing.lua.none') }}</el-radio-button>
              <el-radio-button label="inline">{{ t('wizard.processing.lua.inline') }}</el-radio-button>
              <el-radio-button label="file">{{ t('wizard.processing.lua.file') }}</el-radio-button>
            </el-radio-group>
            <template v-if="luaMode === 'inline'">
              <label>{{ t('wizard.processing.lua.inlineLabel') }}</label>
              <el-input v-model="form.processor.luaInline" type="textarea" :rows="6" :placeholder="t('wizard.processing.lua.inlinePh')" />
            </template>
            <template v-if="luaMode === 'file'">
              <label>{{ t('wizard.processing.lua.fileLabel') }}</label>
              <el-upload
                :auto-upload="false"
                :show-file-list="false"
                accept=".lua"
                :on-change="onLuaFileChange"
              >
                <el-button>{{ t('wizard.processing.lua.chooseFile') }}</el-button>
              </el-upload>
              <span v-if="form.processor.luaFileName" class="wizard__hint">{{ form.processor.luaFileName }}</span>
              <p v-if="luaFileError" class="wizard__warn-text">{{ luaFileError }}</p>
            </template>
          </div>
        </section>
      </template>

      <!-- STEP 5: Advanced (not for struct) -->
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
              <small class="wizard__hint">0 = unlimited</small>
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
            <template v-if="form.config.metricsEnabled">
              <div>
                <label>{{ t('wizard.advanced.metricsHttpHost') }}</label>
                <el-input v-model="form.config.metricsHttpHost" placeholder="127.0.0.1" />
              </div>
              <div>
                <label>{{ t('wizard.advanced.metricsHttpPort') }}</label>
                <el-input-number v-model="form.config.metricsHttpPort" :min="1024" :max="65535" style="width: 100%" />
              </div>
              <div style="grid-column: span 2">
                <label>{{ t('wizard.advanced.metricsLabels') }}</label>
                <el-input v-model="form.config.metricsLabels" placeholder="k1:v1,k2:v2" />
              </div>
            </template>
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
                  <IconAlertTriangle v-else-if="row.result === 'warn' || row.result === 'skip'" />
                  <IconCircleX v-else-if="row.result === 'fail'" />
                  <IconClock v-else />
                  {{ t(`wizard.precheck.result.${row.result}`) }}
                </span>
              </template>
            </el-table-column>
            <el-table-column :label="t('wizard.precheck.col.action')" width="200">
              <template #default="{ row }">
                <el-button
                  v-if="row.result === 'fail' || row.result === 'skip'"
                  link
                  type="primary"
                >{{ row.hint || t('wizard.precheck.action.detail') }}</el-button>
                <span v-else class="wizard__muted">—</span>
              </template>
            </el-table-column>
          </el-table>

          <el-alert
            v-if="precheckProgress >= 100 && precheckHasFail"
            type="error"
            :closable="false"
            show-icon
            class="wizard__alert"
          >
            {{ t('wizard.precheck.failBlock') }}
          </el-alert>
          <el-alert
            v-if="precheckProgress >= 100 && !precheckHasFail && !precheckHasWarn"
            type="success"
            :closable="false"
            show-icon
            class="wizard__alert"
          >
            {{ t('wizard.precheck.allPass') }}
          </el-alert>
        </section>
      </template>

      <!-- STEP 7: Confirm -->
      <template v-else-if="currentStep?.key === 'confirm'">
        <section class="ape-dts-console-card wizard__card">
          <header class="wizard__card-head">
            <h3>{{ t('wizard.confirm.section.basic') }}</h3>
          </header>
          <div class="wizard__conf-grid">
            <div><span>{{ t('wizard.confirm.field.name') }}</span><strong>{{ form.taskName }}</strong></div>
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
              {{ showPreview ? t('wizard.confirm.hidePreview') : t('wizard.confirm.iniPreview') }}
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
                <p><span>{{ t('wizard.source.name') }}</span><strong>{{ form.taskName }}</strong></p>
                <p><span>{{ t('wizard.source.desc._label') }}</span><strong>{{ form.description || '—' }}</strong></p>
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
      <el-button v-if="current > 0" @click="onPrev">
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
        :disabled="!canSubmit"
        @click="onSubmit"
      >
        {{ form.startMode === 'now' ? t('wizard.confirm.submitStart') : t('wizard.confirm.submitLater') }}
      </el-button>
    </footer>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, reactive, ref, watch } from 'vue';
import { useI18n } from 'vue-i18n';
import { useRoute, useRouter, onBeforeRouteLeave } from 'vue-router';
import { ElMessage, ElMessageBox } from 'element-plus';
import dayjs from 'dayjs';
import { api } from '@/api/client';
import { useAuthStore } from '@/stores/auth';
import type {
  EngineType,
  SyncMode,
  ParallelType,
  ResumeType,
  TaskCategory,
} from '@/types/domain';
import { ENGINE_LABELS, GAUSSDB_SUB_MODES } from '@/types/domain';
import {
  buildWizardSteps,
  defaultSubModeFor,
  requiresSubMode,
} from '@/composables/useWizardSteps';
import {
  parseConnectionUrl,
  engineFromUrlScheme,
  TASK_ID_REGEX,
} from '@/composables/useWizardValidation';
import { useWizardDraftStore, type WizardDraftForm, type DraftProcRule } from '@/stores/wizardDraft';
import ConnectionTestCard from '@/components/wizard/ConnectionTestCard.vue';

const { t } = useI18n();
const route = useRoute();
const router = useRouter();
const authStore = useAuthStore();
const draftStore = useWizardDraftStore();

/* ---------- category & steps ---------- */
const category = computed<TaskCategory>(() => (route.params.type as TaskCategory) ?? 'snapshot');
const current = ref(0);

const steps = computed(() => buildWizardSteps(category.value, t));
const currentStep = computed(() => steps.value[current.value]);

/* ---------- sync_mode visibility ---------- */
const showSyncMode = computed(() => category.value === 'snapshot');

/* ---------- engine options ---------- */
const engineOptions = (Object.keys(ENGINE_LABELS) as EngineType[]).map((k) => ({
  value: k, label: ENGINE_LABELS[k],
}));
const resourceGroups = ['default', 'production', 'staging', 'dev'];

/* ---------- parallelizers / resume options ---------- */
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

/* ---------- mode options ---------- */
const modeOptions = computed(() => [
  { value: 'snapshot_cdc' as SyncMode, label: t('wizard.mode.snapshot_cdc.label'), desc: t('wizard.mode.snapshot_cdc.desc') },
  { value: 'snapshot' as SyncMode, label: t('wizard.mode.snapshot.label'), desc: t('wizard.mode.snapshot.desc') },
  { value: 'cdc' as SyncMode, label: t('wizard.mode.cdc.label'), desc: t('wizard.mode.cdc.desc') },
]);

/* ---------- default form ---------- */
function defaultForm(): WizardDraftForm {
  return {
    taskName: `apedts-${Math.random().toString(36).slice(2, 8).toUpperCase()}`,
    description: '',
    taskType: 'standalone',
    resourceGroup: 'default',
    instanceIp: '127.0.0.1',
    source: { engine: 'mysql', subMode: undefined, host: '', port: 3306, username: 'root', password: '', database: '', ssl: false },
    target: { engine: 'mysql', subMode: undefined, host: '', port: 3306, username: 'root', password: '', database: '', ssl: false },
    targetHasPdb: false,
    syncMode: (route.query.mode as SyncMode) || 'snapshot_cdc',
    rate: { mode: 'unlimited', maxRps: 10000 },
    fullType: { schema: true, data: true, index: false },
    conflict: 'insert',
    filter: { doDbs: '', doTbs: '', ignoreDbs: '', ignoreTbs: '', doEvents: ['insert', 'update', 'delete'] },
    config: {
      parallelizer: 'snapshot', parallelSize: 4, bufferSize: 16000,
      checkpointIntervalSecs: 10, maxRps: 0, resumeType: 'from_log',
      metricsEnabled: true, metricsHttpPort: 9090, metricsHttpHost: '127.0.0.1', metricsLabels: '',
    },
    router: { dbMap: '', tbMap: '', colMap: '', topicMap: '' },
    processor: { luaInline: '', luaFile: null, luaFileName: '' },
    procRules: [],
    startMode: 'now',
    delayAlertEnabled: false,
    delayAlertSecs: 60,
    currentStep: 0,
  };
}

const form = reactive<WizardDraftForm>(defaultForm());

/* ---------- load draft from store ---------- */
onMounted(() => {
  const saved = draftStore.load(category.value);
  if (saved) {
    Object.assign(form, saved);
    // Restore step position (but validate — deep-link rejection)
    const targetStep = saved.currentStep ?? 0;
    current.value = Math.min(targetStep, 0); // Always start at 0 for deep-link protection
  }
  // Snapshot original for dirty tracking
  draftStore.snapshotOriginal(category.value);

  if (route.query.mode) form.syncMode = route.query.mode as SyncMode;
});

/* ---------- auto-save draft on change ---------- */
watch(
  () => ({ ...form }),
  () => {
    form.currentStep = current.value;
    draftStore.save(category.value, { ...form });
  },
  { deep: true },
);

/* ---------- deep-link rejection ---------- */
watch(
  () => route.query.step,
  (step) => {
    if (step !== undefined && step !== null) {
      // Reject deep-link to mid-wizard — snap back to step 0
      router.replace({ path: route.path, query: {} });
      ElMessage.warning(t('wizard.draft.deepLinkRejected'));
    }
  },
);

/* ---------- task_id uniqueness check ---------- */
const taskIdError = ref('');

async function checkTaskIdUnique() {
  const name = form.taskName.trim();
  if (!name || !TASK_ID_REGEX.test(name)) {
    if (name && !TASK_ID_REGEX.test(name)) {
      taskIdError.value = t('wizard.source.taskIdInvalid');
    } else {
      taskIdError.value = '';
    }
    return;
  }
  try {
    const res = await api.get<{ items: { id: string }[]; total: number }>(`/tasks?task_id=${encodeURIComponent(name)}`);
    if (res.total > 0) {
      taskIdError.value = t('wizard.source.taskIdTaken');
    } else {
      taskIdError.value = '';
    }
  } catch {
    // Network error — don't block, let server-side check catch it
    taskIdError.value = '';
  }
}

/* ---------- URL credential paste parsing ---------- */
function onSourcePaste(e: ClipboardEvent) {
  const text = e.clipboardData?.getData('text') ?? '';
  if (!text.includes('://')) return; // not a URL, let default paste happen
  const parsed = parseConnectionUrl(text);
  if (parsed) {
    e.preventDefault();
    form.source.host = parsed.host;
    form.source.port = parsed.port;
    form.source.username = parsed.username;
    form.source.password = parsed.password;
    form.source.database = parsed.database;
    // Auto-detect engine from URL scheme
    const engine = engineFromUrlScheme(text);
    if (engine) {
      form.source.engine = engine;
      form.source.port = parsed.port || defaultPort(engine);
      form.source.subMode = defaultSubModeFor(engine);
    }
  }
}

function onTargetPaste(e: ClipboardEvent) {
  const text = e.clipboardData?.getData('text') ?? '';
  if (!text.includes('://')) return;
  const parsed = parseConnectionUrl(text);
  if (parsed) {
    e.preventDefault();
    form.target.host = parsed.host;
    form.target.port = parsed.port;
    form.target.username = parsed.username;
    form.target.password = parsed.password;
    form.target.database = parsed.database;
    const engine = engineFromUrlScheme(text);
    if (engine) {
      form.target.engine = engine;
      form.target.port = parsed.port || defaultPort(engine);
      form.target.subMode = defaultSubModeFor(engine);
    }
  }
}

/* ---------- engine helpers ---------- */
function setSourceEngine(e: EngineType) {
  const prevEngine = form.source.engine;
  form.source.engine = e;
  form.source.port = defaultPort(e);
  form.source.subMode = defaultSubModeFor(e);
  // Engine change clears downstream if past step 3
  if (current.value >= 2 && prevEngine !== e) {
    clearDownstream();
    ElMessage.warning(t('wizard.draft.engineChangeCleared'));
  }
}

function setTargetEngine(e: EngineType) {
  const prevEngine = form.target.engine;
  form.target.engine = e;
  form.target.port = defaultPort(e);
  form.target.subMode = defaultSubModeFor(e);
  if (current.value >= 2 && prevEngine !== e) {
    clearDownstream();
    ElMessage.warning(t('wizard.draft.engineChangeCleared'));
  }
}

function defaultPort(e: EngineType) {
  const map: Record<EngineType, number> = {
    mysql: 3306, tidb: 4000, postgres: 5432, gaussdb: 5432, oracle: 1521,
    mongo: 27017, redis: 6379, kafka: 9092, starrocks: 9030,
    clickhouse: 9000, doris: 9030, foxlake: 443,
  };
  return map[e] ?? 3306;
}

function clearDownstream() {
  form.filter.doDbs = '';
  form.filter.doTbs = '';
  form.filter.ignoreDbs = '';
  form.filter.ignoreTbs = '';
  form.router.dbMap = '';
  form.router.tbMap = '';
  form.router.colMap = '';
  form.router.topicMap = '';
  form.processor.luaInline = '';
  form.processor.luaFile = null;
  form.processor.luaFileName = '';
  procRules.value = [];
  // Reset parallelizer to default for engine
  form.config.parallelizer = 'snapshot';
  form.config.parallelSize = 4;
}

/* ---------- test connection ---------- */
const testState = reactive<{
  source: { status: 'idle' | 'running' | 'ok' | 'fail'; latency?: number; message?: string };
  target: { status: 'idle' | 'running' | 'ok' | 'fail'; latency?: number; message?: string };
}>({
  source: { status: 'idle' },
  target: { status: 'idle' },
});
const bypassTest = ref(false);
const canBypassTest = computed(() => authStore.user?.role === 'admin' || authStore.user?.role === 'operator');

async function runTest(which: 'source' | 'target') {
  testState[which] = { status: 'running' };
  try {
    const res = await api.post<{
      source: { ok: boolean; latency_ms?: number; message?: string };
      target: { ok: boolean; latency_ms?: number; message?: string };
    }>('/tasks/preview/test_connection', formToTaskDraft());
    const side = res[which];
    if (side.ok) {
      testState[which] = { status: 'ok', latency: side.latency_ms };
    } else {
      testState[which] = { status: 'fail', message: side.message };
    }
  } catch (err: unknown) {
    const msg = (err as { message?: string })?.message ?? String(err);
    testState[which] = { status: 'fail', message: msg };
  }
}

/* ---------- processing (step 4) ---------- */
const procRules = ref<DraftProcRule[]>([]);
const procSearch = ref('');
const luaMode = ref<'none' | 'inline' | 'file'>('none');
const luaFileError = ref('');
const LUA_FILE_SIZE_CAP = 1_048_576; // 1 MB

function addProcRule() {
  const doTbs = form.filter.doTbs || '*.*';
  procRules.value.push({
    target: doTbs,
    doEvents: ['insert', 'update', 'delete'],
    where: '',
    ignoreCols: '',
  });
}

function removeInvalidRules() {
  // Keep all rules — user manages them manually
  ElMessage.success(t('wizard.processing.removeInvalid'));
}

const filteredProcRules = computed(() => {
  if (!procSearch.value) return procRules.value;
  return procRules.value.filter((r) => r.target.includes(procSearch.value));
});

function onLuaFileChange(uploadFile: { raw?: File; name: string }) {
  const raw = uploadFile.raw;
  if (!raw) return;
  if (!uploadFile.name.endsWith('.lua')) {
    luaFileError.value = t('wizard.processing.lua.notLua');
    return;
  }
  if (raw.size > LUA_FILE_SIZE_CAP) {
    luaFileError.value = t('wizard.processing.lua.tooLarge', { max: LUA_FILE_SIZE_CAP / 1024 / 1024 });
    return;
  }
  luaFileError.value = '';
  form.processor.luaFileName = uploadFile.name;
  const reader = new FileReader();
  reader.onload = () => {
    form.processor.luaFile = reader.result as string;
  };
  reader.readAsText(raw);
}

/* ---------- precheck (step 6) ---------- */
interface PrecheckItem {
  key: string; title: string; group: string;
  result: 'pending' | 'running' | 'pass' | 'warn' | 'fail' | 'skip';
  hint?: string;
}

const precheckItems = ref<PrecheckItem[]>([]);
const precheckProgress = ref(0);
const precheckRunning = ref(false);

const precheckHasFail = computed(() => precheckItems.value.some((i) => i.result === 'fail'));
const precheckHasWarn = computed(() => precheckItems.value.some((i) => i.result === 'warn' || i.result === 'skip'));

const precheckResultStatus = computed<'success' | 'exception' | 'warning' | ''>(() => {
  if (precheckItems.value.some((i) => i.result === 'fail')) return 'exception';
  if (precheckItems.value.some((i) => i.result === 'warn' || i.result === 'skip')) return 'warning';
  if (precheckProgress.value >= 100) return 'success';
  return '';
});

async function runPrecheck() {
  precheckRunning.value = true;
  precheckProgress.value = 0;
  precheckItems.value = [];
  try {
    const res = await api.post<{
      items: { key: string; title: string; group: string; status: 'pass' | 'fail' | 'skip'; hint?: string }[];
    }>('/tasks/preview/precheck', formToTaskDraft());
    const items = res.items.map((r) => ({
      key: r.key,
      title: r.title,
      group: r.group,
      result: r.status as PrecheckItem['result'],
      hint: r.hint,
    }));
    precheckItems.value = items;
    precheckProgress.value = 100;
  } catch (err: unknown) {
    // If the API fails, show a single error item
    const msg = (err as { message?: string })?.message ?? String(err);
    precheckItems.value = [{
      key: 'error', title: t('wizard.precheck.error'), group: 'API', result: 'fail', hint: msg,
    }];
    precheckProgress.value = 100;
  } finally {
    precheckRunning.value = false;
  }
}

/* ---------- confirm (step 7) ---------- */
const showPreview = ref(false);
const iniPreview = ref('');
const createdTaskId = ref('');

async function togglePreview() {
  if (!showPreview.value) {
    // If we already have a created task, fetch preview_ini directly
    if (createdTaskId.value) {
      try {
        iniPreview.value = await api.get<string>(`/tasks/${createdTaskId.value}/preview_ini`);
      } catch {
        iniPreview.value = t('wizard.confirm.previewError');
      }
    } else {
      try {
        const res = await api.post<{ ini: string }>('/tasks/preview-ini', formToTaskDraft());
        iniPreview.value = res.ini ?? res;
      } catch {
        iniPreview.value = t('wizard.confirm.previewError');
      }
    }
  }
  showPreview.value = !showPreview.value;
}

function modeLabel(m: SyncMode) {
  return t(`wizard.mode.${m}.label`);
}
const nowLabel = computed(() => dayjs().format('YYYY/MM/DD HH:mm:ss'));

/* ---------- form → DTO ---------- */
function formToTaskDraft() {
  const extractType = syncModeToExtractType(form.syncMode, category.value);
  const kind = category.value;
  const doDbs = form.filter.doDbs?.trim() || '';
  const doTbs = form.filter.doTbs?.trim() || '';
  const ignoreDbs = form.filter.ignoreDbs?.trim() || '';
  const ignoreTbs = form.filter.ignoreTbs?.trim() || '';

  const sourceSubMode = form.source.engine === 'gaussdb' ? form.source.subMode : undefined;
  const targetSubMode = form.target.engine === 'gaussdb' ? form.target.subMode : undefined;

  const sourceDb = sourceSubMode === 'pg-mode' ? 'gaussdb_pg'
    : sourceSubMode === 'mysql-mode' ? 'gaussdb_mysql'
    : sourceSubMode === 'oracle-mode' ? 'gaussdb_oracle'
    : form.source.engine;

  const targetDb = targetSubMode === 'pg-mode' ? 'gaussdb_pg'
    : targetSubMode === 'mysql-mode' ? 'gaussdb_mysql'
    : targetSubMode === 'oracle-mode' ? 'gaussdb_oracle'
    : form.target.engine;

  return {
    name: form.taskName,
    kind,
    engineSource: form.source.engine,
    engineTarget: form.target.engine,
    subMode: sourceSubMode ?? targetSubMode,
    sourceEndpoint: {
      url: buildUrl(form.source, sourceDb),
    },
    targetEndpoint: {
      url: buildUrl(form.target, targetDb),
    },
    extractor: {
      extract_type: extractType,
    },
    sinker: {},
    filter: {
      do_dbs: doDbs,
      do_tbs: doTbs,
      ignore_dbs: ignoreDbs,
      ignore_tbs: ignoreTbs,
      do_events: form.filter.doEvents?.join(',') || '',
    },
    router: parseMapField(form.router.dbMap, form.router.tbMap, form.router.colMap, form.router.topicMap),
    parallelizer: {
      parallel_type: form.config.parallelizer,
      parallel_size: form.config.parallelSize,
    },
    pipeline: {
      buffer_size: form.config.bufferSize,
      checkpoint_interval_secs: form.config.checkpointIntervalSecs,
      max_rps: form.config.maxRps,
    },
    resumer: {
      resume_type: form.config.resumeType,
    },
    processor: (form.processor.luaInline || form.processor.luaFile)
      ? {
          lua_code_file: 'inline',
          lua_code: form.processor.luaInline || form.processor.luaFile || undefined,
        }
      : undefined,
    runtime: {},
    metrics: form.config.metricsEnabled
      ? {
          http_host: form.config.metricsHttpHost,
          http_port: form.config.metricsHttpPort,
          labels: form.config.metricsLabels,
        }
      : undefined,
    resourceGroupId: form.resourceGroup,
  };
}

function buildUrl(ep: { host: string; port: number; username: string; password: string; database: string; ssl: boolean }, dbType: string) {
  const scheme = dbType === 'mysql' || dbType === 'gaussdb_mysql' ? 'mysql'
    : dbType === 'postgres' || dbType === 'gaussdb_pg' ? 'postgres'
    : dbType === 'oracle' || dbType === 'gaussdb_oracle' ? 'oracle'
    : dbType === 'mongo' ? 'mongodb'
    : dbType === 'redis' ? 'redis'
    : dbType === 'kafka' ? 'kafka'
    : 'mysql';
  const dbPart = ep.database ? `/${ep.database}` : '';
  return `${scheme}://${ep.username}:${ep.password}@${ep.host}:${ep.port}${dbPart}`;
}

function parseMapField(dbMap: string, tbMap: string, colMap: string, topicMap: string) {
  const router: Record<string, string> = {};
  if (dbMap) {
    router.db_map = dbMap.split('\n').filter((l) => l.includes(':')).map((l) => {
      const [k, v] = l.split(':');
      return `${k.trim()}:${v.trim()}`;
    }).join(',');
  }
  if (tbMap) {
    router.tb_map = tbMap.split('\n').filter((l) => l.includes(':')).map((l) => {
      const [k, v] = l.split(':');
      return `${k.trim()}:${v.trim()}`;
    }).join(',');
  }
  if (colMap) {
    router.col_map = colMap.split('\n').filter((l) => l.includes(':')).map((l) => {
      const [k, v] = l.split(':');
      return `${k.trim()}:${v.trim()}`;
    }).join(',');
  }
  if (topicMap && form.target.engine === 'kafka') {
    router.topic_map = topicMap.split('\n').filter((l) => l.includes(':')).map((l) => {
      const [k, v] = l.split(':');
      return `${k.trim()}:${v.trim()}`;
    }).join(',');
  }
  return Object.keys(router).length > 0 ? router : undefined;
}

function syncModeToExtractType(mode: SyncMode, cat: TaskCategory) {
  if (cat === 'struct') return 'struct' as const;
  if (cat === 'check') return 'snapshot' as const;
  // snapshot_cdc mode: precheck tests the snapshot phase;
  // the backend engine doesn't yet support snapshot_and_cdc as an extract_type
  if (mode === 'snapshot_cdc') return 'snapshot' as const;
  if (mode === 'snapshot') return 'snapshot' as const;
  if (mode === 'cdc') return 'cdc' as const;
  return 'snapshot' as const;
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
      !!form.taskName &&
      TASK_ID_REGEX.test(form.taskName) &&
      !taskIdError.value &&
      endpointSubModeOk('source') &&
      endpointSubModeOk('target')
    );
  }
  if (key === 'test') {
    return bypassTest.value ||
      (testState.source.status === 'ok' && testState.target.status === 'ok');
  }
  if (key === 'objects') {
    return !!(form.filter.doDbs.trim() || form.filter.doTbs.trim());
  }
  if (key === 'precheck') {
    return precheckProgress.value >= 100;
  }
  return true;
});

const canSubmit = computed(() => {
  return precheckProgress.value >= 100 && !submitting.value;
});

function onNext() {
  const key = currentStep.value?.key;
  if (!canProceed.value) {
    if (key === 'source') {
      if (!endpointSubModeOk('source') || !endpointSubModeOk('target')) {
        ElMessage.warning(t('wizard.source.subMode.required'));
      } else if (taskIdError.value) {
        ElMessage.warning(taskIdError.value);
      }
    }
    if (key === 'test') ElMessage.warning(t('wizard.action.testFirst'));
    if (key === 'objects') ElMessage.warning(t('wizard.objects.atLeastOne'));
    if (key === 'precheck') ElMessage.warning(t('wizard.precheck.failBlock'));
    return;
  }
  current.value++;
  // Auto-trigger precheck when landing on step 6
  if (currentStep.value?.key === 'precheck' && precheckProgress.value === 0) {
    runPrecheck();
  }
}

function onPrev() {
  current.value--;
}

function stepClass(idx: number) {
  return {
    'wizard__step--active': idx === current.value,
    'wizard__step--done': idx < current.value,
    'wizard__step--future': idx > current.value,
  };
}

function gotoStep(idx: number) {
  if (idx <= current.value) current.value = idx;
}

/* ---------- discard draft ---------- */
function onDiscardDraft() {
  ElMessageBox.confirm(t('wizard.draft.discardConfirm'), t('wizard.draft.discardTitle'), {
    confirmButtonText: t('wizard.draft.discard'),
    cancelButtonText: t('common.cancel'),
    type: 'warning',
  }).then(() => {
    draftStore.discard(category.value);
    Object.assign(form, defaultForm());
    current.value = 0;
    taskIdError.value = '';
    testState.source = { status: 'idle' };
    testState.target = { status: 'idle' };
    precheckItems.value = [];
    precheckProgress.value = 0;
    procRules.value = [];
    iniPreview.value = '';
    showPreview.value = false;
    createdTaskId.value = '';
    ElMessage.success(t('wizard.draft.discardDone'));
  }).catch(() => { /* cancelled */ });
}

/* ---------- dirty-draft navigation prompt ---------- */
function onBack() {
  if (draftStore.isDirty(category.value)) {
    ElMessageBox.confirm(t('wizard.draft.leaveConfirm'), t('wizard.draft.leaveTitle'), {
      confirmButtonText: t('wizard.draft.leaveDiscard'),
      cancelButtonText: t('common.cancel'),
      distinguishCancelAndClose: true,
      type: 'warning',
    }).then(() => {
      draftStore.discard(category.value);
      router.push({ path: `/tasks/${category.value}` });
    }).catch(() => { /* cancelled — stay */ });
  } else {
    router.push({ path: `/tasks/${category.value}` });
  }
}

onBeforeRouteLeave((_to, _from, next) => {
  if (draftStore.isDirty(category.value)) {
    ElMessageBox.confirm(t('wizard.draft.leaveConfirm'), t('wizard.draft.leaveTitle'), {
      confirmButtonText: t('wizard.draft.leaveDiscard'),
      cancelButtonText: t('common.cancel'),
      distinguishCancelAndClose: true,
      type: 'warning',
    }).then(() => {
      draftStore.discard(category.value);
      next();
    }).catch(() => {
      next(false);
    });
  } else {
    next();
  }
});

/* ---------- submit ---------- */
const submitting = ref(false);

async function onSubmit() {
  if (precheckHasFail.value) {
    try {
      await ElMessageBox.confirm(
        t('wizard.precheck.failBlock'),
        t('wizard.precheck.failTitle'),
        { confirmButtonText: t('common.confirm'), cancelButtonText: t('common.cancel'), type: 'warning' },
      );
    } catch { return; }
  }
  submitting.value = true;
  try {
    const res = await api.post<{ id: string; category: string }>('/tasks', formToTaskDraft());
    createdTaskId.value = res.id;
    // Clear draft on successful submit
    draftStore.discard(category.value);
    ElMessage.success(form.startMode === 'now' ? t('wizard.toast.created') : t('wizard.toast.createdLater'));
    router.push({ path: `/tasks/${category.value}/${res.id}` });
  } catch (err: unknown) {
    const apiErr = err as { code?: string; message?: string };
    if (apiErr.code === 'task_id_taken') {
      taskIdError.value = t('wizard.source.taskIdTaken');
    }
    ElMessage.error(apiErr.message ?? t('wizard.toast.createFailed'));
  } finally {
    submitting.value = false;
  }
}

/* ---------- watch for category change resetting steps ---------- */
watch(category, () => {
  current.value = 0;
  // Load draft for new category if exists
  const saved = draftStore.load(category.value);
  if (saved) {
    Object.assign(form, saved);
  } else {
    Object.assign(form, defaultForm());
  }
  draftStore.snapshotOriginal(category.value);
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
.wizard__form--basic {
  padding: 16px 20px 18px;
  display: flex;
  flex-direction: column;
  gap: 10px;
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
.wizard__field-error {
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
.wizard__check-result--warn, .wizard__check-result--skip { color: var(--color-warning); }
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
