import { describe, expect, it } from 'vitest';
import { renderIni } from '@/utils/iniRenderer';
import {
  gaussdbOracleCdcTask,
  minimalCdcTask,
  minimalSnapshotTask,
  minimalStructTask,
} from '../helpers/fixtures';

describe('IniRenderer (golden)', () => {
  it('emits global, extractor, sinker sections for snapshot task', () => {
    const ini = renderIni(minimalSnapshotTask);
    expect(ini).toContain('[global]\ntask_id=snapshot_task_1');
    expect(ini).toContain('[extractor]\ndb_type=mysql\nextract_type=snapshot');
    expect(ini).toContain('[sinker]\ndb_type=mysql\nsink_type=write');
  });

  it('uses sink_type=check for Check tasks', () => {
    const ini = renderIni({ ...minimalSnapshotTask, kind: 'check' });
    expect(ini).toMatch(/\[sinker\][\s\S]*sink_type=check/);
  });

  it('uses sink_type=struct for Struct tasks', () => {
    const ini = renderIni(minimalStructTask);
    expect(ini).toMatch(/\[sinker\][\s\S]*sink_type=struct/);
  });

  it('serialises filter.do_dbs as comma list', () => {
    const ini = renderIni({
      ...minimalSnapshotTask,
      filter: { doDbs: ['db_1', 'db_2*'] },
    });
    expect(ini).toContain('do_dbs=db_1,db_2*');
  });

  it('serialises router.db_map as separate lines', () => {
    const ini = renderIni({
      ...minimalCdcTask,
      router: { dbMap: { src_a: 'dst_a', src_b: 'dst_b' } },
    });
    expect(ini).toContain('db_map=src_a:dst_a');
    expect(ini).toContain('db_map=src_b:dst_b');
  });

  it('renders metrics section when configured', () => {
    const ini = renderIni(minimalCdcTask);
    expect(ini).toContain('[metrics]\nhttp_host=127.0.0.1\nhttp_port=9090');
  });

  it('preserves engine sub-mode hint for GaussDB-Oracle (smoke)', () => {
    const ini = renderIni(gaussdbOracleCdcTask);
    expect(ini).toContain('db_type=gaussdb');
    expect(ini).toContain('db_type=oracle');
  });

  it.todo('byte-exact golden for snapshot mysql→mysql with default options');
  it.todo('byte-exact golden for cdc pg→kafka with topic_map');
  it.todo('byte-exact golden for check mysql→mysql with rdb_check parallelizer');
  it.todo('byte-exact golden for struct pg→clickhouse');
  it.todo('escapes special characters in url credentials');
});
