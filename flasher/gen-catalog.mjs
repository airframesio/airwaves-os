#!/usr/bin/env node
/*
 * Generate flasher/catalog.json — the live catalog the Airwaves OS flasher fetches
 * (DEFAULT_CATALOG_URL = .../airwaves-os/main/flasher/catalog.json). Schema is
 * defined by airwaves-os-flasher (crates/awflash-core/src/model.rs).
 *
 * Builds the image list from the ACTUAL GitHub release assets (real URLs, sizes,
 * and download sha256), so it always matches what was published. Re-run after a
 * release:  node flasher/gen-catalog.mjs
 *
 * Add a new release by appending to RELEASES below.
 */
import { execFileSync } from 'node:child_process';
import { mkdtempSync, readFileSync, readdirSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const REPO = 'airframesio/airwaves-os';

// Releases to publish, newest first per channel. tag = GitHub release tag.
const RELEASES = [
  { tag: '1.0.38-dev', channel: 'dev',    version: '1.0.38', released: '2026-06-18' },
  { tag: 'v1.0.37',    channel: 'stable', version: '1.0.37', released: '2026-06-16' },
];

// Curated device metadata (only boards we actually ship images for show up — the
// flasher prunes imageless devices). icon/arch/vendor drive the picker UI.
const DEVICES = {
  rpi4b:            { name: 'Raspberry Pi 3 / 4', description: 'Popular 64-bit feeder board with great SDR support. (Pi 5 not yet supported.)', icon: 'rpi',     arch: 'arm64', vendor: 'Raspberry Pi', tags: ['supported', 'popular'] },
  'uefi-x86':       { name: 'PC / Mini-PC (x86 UEFI)', description: 'Any 64-bit UEFI PC or mini-PC.',                 icon: 'x86',     arch: 'amd64', vendor: 'PC / x86',     tags: ['supported', 'popular'] },
  'rock-5b':        { name: 'Radxa ROCK 5B',      description: 'High-performance RK3588 board for demanding multi-band stations.', icon: 'rock', arch: 'arm64', vendor: 'Radxa', tags: ['supported'] },
  'rock-5a':        { name: 'Radxa ROCK 5A',      description: 'Compact RK3588S board; a strong-value feeder.',       icon: 'rock',    arch: 'arm64', vendor: 'Radxa',        tags: ['supported'] },
  orangepi5:        { name: 'Orange Pi 5',        description: 'Compact RK3588S board, a strong-value feeder.',       icon: 'orangepi',arch: 'arm64', vendor: 'Orange Pi',    tags: ['supported'] },
  'orangepi5-plus': { name: 'Orange Pi 5 Plus',   description: 'RK3588 board with dual 2.5GbE for multi-feed stations.', icon: 'orangepi', arch: 'arm64', vendor: 'Orange Pi', tags: ['supported'] },
};
const SHORT = { rpi4b: 'Raspberry Pi', 'uefi-x86': 'x86 UEFI', 'rock-5b': 'ROCK 5B', 'rock-5a': 'ROCK 5A', orangepi5: 'Orange Pi 5', 'orangepi5-plus': 'Orange Pi 5 Plus' };
const CODENAME = 'Sideband';
// No shell: args passed directly to gh (all inputs are constants anyway).
const gh = (...args) => execFileSync('gh', args, { encoding: 'utf8' });

// board id from an asset name: Airwaves_OS_<verfield>_<Board>_<release>_... .img.xz
function boardOf(name) {
  const p = name.split('_');
  if (p.length < 4 || p[2] === 'LiveUSB') return null;   // skip the x86 Live USB variant
  return p[3].toLowerCase();
}

const images = [];
const usedDevices = new Set();

for (const rel of RELEASES) {
  const assets = JSON.parse(gh('release', 'view', rel.tag, '--repo', REPO, '--json', 'assets')).assets;
  // pull the .sha256 sidecars so we can publish download_sha256
  const dir = mkdtempSync(join(tmpdir(), 'awcat-'));
  gh('release', 'download', rel.tag, '--repo', REPO, '--dir', dir, '--pattern', '*.img.xz.sha256');
  const shaByFile = {};
  for (const f of readdirSync(dir)) {
    const txt = readFileSync(join(dir, f), 'utf8').trim();
    const [hash, fname] = txt.split(/\s+/);
    if (hash && fname) shaByFile[fname.replace(/^\*/, '')] = hash;
  }

  for (const a of assets) {
    if (!a.name.endsWith('.img.xz')) continue;
    const device = boardOf(a.name);
    if (!device || !DEVICES[device]) continue;
    usedDevices.add(device);
    images.push({
      id: `${device}-${rel.channel}-${rel.version}`,
      device,
      channel: rel.channel,
      osVersion: rel.version,
      codename: CODENAME,
      name: `Airwaves OS ${rel.version} (${SHORT[device] ?? device})`,
      released: rel.released,
      notesUrl: `https://github.com/${REPO}/releases/tag/${rel.tag}`,
      url: a.url,
      compression: 'xz',
      downloadSize: a.size,
      downloadSha256: shaByFile[a.name] ?? undefined,
      minStorageBytes: 4 * 1024 * 1024 * 1024,
    });
  }
}

// mark newest per device+channel
const cmp = (a, b) => a.split('.').map(Number).reduce((acc, n, i) => acc || n - b.split('.').map(Number)[i], 0);
for (const img of images) {
  const peers = images.filter((x) => x.device === img.device && x.channel === img.channel);
  img.latest = peers.every((p) => cmp(img.osVersion, p.osVersion) >= 0);
}

const devices = [...usedDevices].sort().map((id) => ({ id, ...DEVICES[id] }));
devices.sort((a, b) => a.name.localeCompare(b.name));

const catalog = {
  schema: 1,
  name: 'Airwaves OS',
  updated: new Date().toISOString().replace(/\.\d+Z$/, 'Z'),
  channels: ['stable', 'beta', 'dev'],
  devices,
  images,
  source: 'remote',
};
writeFileSync(new URL('./catalog.json', import.meta.url), JSON.stringify(catalog, null, 2) + '\n');
console.log(`Wrote flasher/catalog.json: ${devices.length} devices, ${images.length} images`);
for (const i of images) console.log(`  ${i.channel}  ${i.device}  ${i.osVersion}  ${(i.downloadSize/1e6|0)}MB  sha=${i.downloadSha256 ? 'yes' : 'NO'}`);
