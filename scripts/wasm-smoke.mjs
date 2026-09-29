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

console.log('wasm smoke test: ok');
