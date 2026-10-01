// Smoke test for the built WASM package: node scripts/wasm-smoke.mjs [pkg-dir]
import { readFileSync } from 'node:fs';
import assert from 'node:assert/strict';
import { pathToFileURL } from 'node:url';
import { resolve } from 'node:path';

const pkg = resolve(process.argv[2] ?? 'pkg');
const wint = await import(pathToFileURL(`${pkg}/wint.js`).href);
wint.initSync({ module: readFileSync(`${pkg}/wint_bg.wasm`) });

const plan = readFileSync('examples/plan.json', 'utf8');
const series = readFileSync('examples/series.json', 'utf8');
const csv = readFileSync('examples/series.csv', 'utf8');
const openMeteo = readFileSync('tests/fixtures/open_meteo_hourly.json', 'utf8');

assert.match(wint.version(), /^\d+\.\d+\.\d+/);

// Same expectations as the Rust integration tests.
const r = JSON.parse(wint.search(series, plan));
assert.equal(r.feasible.length, 2);
assert.equal(r.rejected.length, 2);
assert.equal(r.feasible[0].stages[0].name, 'flight');

// CSV and JSON series give identical answers on the same data.
const rc = JSON.parse(wint.search(wint.parseCsv(csv), plan));
assert.equal(rc.feasible.length, 2);

// Open-Meteo fixture + preset: hours 1 and 3 pass, 0 and 2 fail.
const om = wint.parseOpenMeteo(openMeteo);
const ro = JSON.parse(wint.search(om, wint.presetPlan('drone', 1)));
assert.deepEqual(ro.feasible.map(w => w.start_ms).sort(), [1790067600000, 1790074800000]);

// Vocabulary, presets, URL.
assert.ok(JSON.parse(wint.listMetrics()).some(m => m.name === 'wind_speed' && m.unit === 'm/s'));
assert.deepEqual(JSON.parse(wint.listPresets()).map(p => p.name), ['drone', 'outdoor-event', 'field-work']);
assert.match(wint.openMeteoUrl(52.52, 13.41, 3), /^https:\/\/api\.open-meteo\.com\/v1\/forecast\?latitude=52\.52/);

// Errors are thrown as readable messages, not opaque traps.
assert.throws(() => wint.search('{}', plan), /invalid series/);
assert.throws(() => wint.presetPlan('nope', 1), /unknown preset `nope`/);
assert.throws(() => wint.presetPlan('drone', 0), /hours must be a positive number/);
assert.throws(() => wint.parseCsv(''), /empty input/);


// ---- Ensembles: the same real fixtures and facts as the Rust tests ----
const multi = readFileSync('tests/fixtures/open_meteo_multi_model.json', 'utf8');
const ens = readFileSync('tests/fixtures/open_meteo_ensemble.json', 'utf8');
const models = JSON.parse(wint.parseOpenMeteoEnsemble(multi));
assert.deepEqual(models.members.map(m => m.name), ['ecmwf_ifs025', 'gfs_seamless', 'icon_seamless', 'meteofrance_seamless']);
const drone2h = wint.presetPlan('drone', 2);
const rm = JSON.parse(wint.searchEnsemble(JSON.stringify(models), drone2h, 1, 0.5));
assert.equal(rm.members.length, 4);
const w = rm.windows.find(x => x.unknown === 2 && x.feasible === 2);
assert.ok(w, 'a window where two models fit and two cannot judge visibility');
assert.deepEqual([w.agreement, w.coverage, w.meets_requirement, w.missing[0].metric, w.missing[0].members], [1, 0.5, true, 'visibility', 2]);
assert.deepEqual(w.outcomes.map(o => o.verdict).sort(), ['feasible', 'feasible', 'unknown', 'unknown']);
const members6 = JSON.parse(wint.parseOpenMeteoEnsemble(ens));
assert.equal(members6.members[0].name, 'control');
assert.equal(members6.members.length, 6);
const field = JSON.parse(wint.searchEnsemble(JSON.stringify(members6), wint.presetPlan('field-work', 2), 0.8, 0.5));
assert.ok(field.windows.every(x => x.unknown === 0 && x.coverage === 1));
// A plain single-model response is a one-member ensemble.
assert.equal(JSON.parse(wint.parseOpenMeteoEnsemble(openMeteo)).members[0].name, 'default');
// URLs and errors.
assert.match(wint.multiModelUrl(52.52, 13.41, 3, 'ecmwf_ifs025, gfs_seamless'), /&models=ecmwf_ifs025,gfs_seamless&/);
assert.match(wint.ensembleUrl(52.52, 13.41, 3, 'icon_seamless'), /^https:\/\/ensemble-api\.open-meteo\.com\/v1\/ensemble\?/);
assert.throws(() => wint.multiModelUrl(0, 0, 1, 'Bad Name'), /not a model name/);
assert.throws(() => wint.searchEnsemble(JSON.stringify(models), drone2h, 0, 0.5), /min_agreement must be greater than 0/);
assert.throws(() => wint.searchEnsemble('{"members":[]}', drone2h, 1, 0.5), /no members/);
assert.throws(() => wint.parseOpenMeteoEnsemble('{"hourly":{"time":[0,3600],"x":[1,2]}}'), /none of the supported variables/);

console.log('wasm smoke test: ok');
