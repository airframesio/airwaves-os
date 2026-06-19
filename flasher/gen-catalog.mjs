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
  // Tier 2
  'rock-5c':        { name: 'Radxa ROCK 5C',      description: 'RK3588S2 board, compact and efficient.',             icon: 'rock',    arch: 'arm64', vendor: 'Radxa',          tags: ['supported'] },
  'rock-5b-plus':   { name: 'Radxa ROCK 5B Plus', description: 'RK3588 board with onboard eMMC and PoE option.',      icon: 'rock',    arch: 'arm64', vendor: 'Radxa',          tags: ['supported'] },
  nanopct6:         { name: 'FriendlyELEC NanoPC-T6', description: 'RK3588 board with dual 2.5GbE and M.2.',          icon: 'sbc',     arch: 'arm64', vendor: 'FriendlyELEC',   tags: ['supported'] },
  'nanopi-r6s':     { name: 'FriendlyELEC NanoPi R6S', description: 'RK3588S router/feeder with dual 2.5GbE + GbE.',  icon: 'sbc',     arch: 'arm64', vendor: 'FriendlyELEC',   tags: ['supported'] },
  odroidn2:         { name: 'Hardkernel Odroid N2/N2+', description: 'Amlogic S922X board, a long-time favourite.',   icon: 'sbc',     arch: 'arm64', vendor: 'Hardkernel',     tags: ['supported'] },
  odroidc4:         { name: 'Hardkernel Odroid C4', description: 'Amlogic S905X3 board, low-cost and efficient.',     icon: 'sbc',     arch: 'arm64', vendor: 'Hardkernel',     tags: ['supported'] },
  orangepizero3:    { name: 'Orange Pi Zero 3',   description: 'Tiny, inexpensive Allwinner H618 feeder.',            icon: 'orangepi',arch: 'arm64', vendor: 'Orange Pi',      tags: ['supported'] },
  'orangepi3-lts':  { name: 'Orange Pi 3 LTS',    description: 'Allwinner H6 board with Wi-Fi and eMMC.',             icon: 'orangepi',arch: 'arm64', vendor: 'Orange Pi',      tags: ['supported'] },
  lepotato:         { name: 'Libre Computer Le Potato', description: 'Amlogic S905X board, very low cost.',           icon: 'sbc',     arch: 'arm64', vendor: 'Libre Computer', tags: ['supported'] },
  'khadas-vim3':    { name: 'Khadas VIM3',        description: 'Amlogic A311D board with NPU and eMMC.',              icon: 'sbc',     arch: 'arm64', vendor: 'Khadas',         tags: ['supported'] },
  // Tier 3
  'rock-5t':        { name: 'Radxa ROCK 5T',      description: 'RK3588 board with dual 2.5GbE.',                      icon: 'rock',    arch: 'arm64', vendor: 'Radxa',          tags: ['supported'] },
  'orangepi5-max':  { name: 'Orange Pi 5 Max',    description: 'Top-end RK3588 board with 2.5GbE.',                   icon: 'orangepi',arch: 'arm64', vendor: 'Orange Pi',      tags: ['supported'] },
  orangepi3b:       { name: 'Orange Pi 3B',       description: 'RK3566 board, a strong-value feeder.',                icon: 'orangepi',arch: 'arm64', vendor: 'Orange Pi',      tags: ['supported'] },
  'radxa-zero3':    { name: 'Radxa ZERO 3W',      description: 'Tiny RK3566 board, Pi-Zero form factor.',             icon: 'rock',    arch: 'arm64', vendor: 'Radxa',          tags: ['supported'] },
  'rock-3a':        { name: 'Radxa ROCK 3A',      description: 'RK3568 board with M.2 and GbE.',                      icon: 'rock',    arch: 'arm64', vendor: 'Radxa',          tags: ['supported'] },
  odroidm1:         { name: 'Hardkernel Odroid M1', description: 'RK3568 board with NVMe and eMMC.',                  icon: 'sbc',     arch: 'arm64', vendor: 'Hardkernel',     tags: ['supported'] },
  odroidc2:         { name: 'Hardkernel Odroid C2', description: 'Classic Amlogic S905 board.',                       icon: 'sbc',     arch: 'arm64', vendor: 'Hardkernel',     tags: ['supported'] },
  orangepizero2w:   { name: 'Orange Pi Zero 2W',  description: 'Tiny Allwinner H618 board, Pi-Zero form factor.',     icon: 'orangepi',arch: 'arm64', vendor: 'Orange Pi',      tags: ['supported'] },
  'nanopi-r4s':     { name: 'FriendlyELEC NanoPi R4S', description: 'RK3399 router/feeder with dual GbE.',           icon: 'sbc',     arch: 'arm64', vendor: 'FriendlyELEC',   tags: ['supported'] },
  'orangepi4-lts':  { name: 'Orange Pi 4 LTS',    description: 'RK3399 board with Wi-Fi and eMMC.',                   icon: 'orangepi',arch: 'arm64', vendor: 'Orange Pi',      tags: ['supported'] },
};
const SHORT = {
  rpi4b: 'Raspberry Pi', 'uefi-x86': 'x86 UEFI', 'rock-5b': 'ROCK 5B', 'rock-5a': 'ROCK 5A', orangepi5: 'Orange Pi 5', 'orangepi5-plus': 'Orange Pi 5 Plus',
  'rock-5c': 'ROCK 5C', 'rock-5b-plus': 'ROCK 5B+', nanopct6: 'NanoPC-T6', 'nanopi-r6s': 'NanoPi R6S', odroidn2: 'Odroid N2', odroidc4: 'Odroid C4',
  orangepizero3: 'Orange Pi Zero 3', 'orangepi3-lts': 'Orange Pi 3 LTS', lepotato: 'Le Potato', 'khadas-vim3': 'Khadas VIM3',
  'rock-5t': 'ROCK 5T', 'orangepi5-max': 'Orange Pi 5 Max', orangepi3b: 'Orange Pi 3B', 'radxa-zero3': 'Radxa ZERO 3W', 'rock-3a': 'ROCK 3A',
  odroidm1: 'Odroid M1', odroidc2: 'Odroid C2', orangepizero2w: 'Orange Pi Zero 2W', 'nanopi-r4s': 'NanoPi R4S', 'orangepi4-lts': 'Orange Pi 4 LTS',
};
const CODENAME = 'Sideband';
// No shell: args passed directly to gh (all inputs are constants anyway).
const gh = (...args) => execFileSync('gh', args, { encoding: 'utf8' });

// GitHub's release-asset listing is eventually-consistent: a single
// `gh release view --json assets` can return an incomplete set right after an
// upload (some assets transiently absent). Assets never legitimately vanish, so
// read a few times and union by name — converges to the complete list.
function fetchAssets(tag) {
  const byName = new Map();
  let stable = 0, last = -1;
  for (let i = 0; i < 6 && stable < 2; i++) {
    const assets = JSON.parse(gh('release', 'view', tag, '--repo', REPO, '--json', 'assets')).assets;
    for (const a of assets) if (!byName.has(a.name)) byName.set(a.name, a);
    stable = byName.size === last ? stable + 1 : 0;
    last = byName.size;
  }
  return [...byName.values()];
}

// board id from an asset name: Airwaves_OS_<verfield>_<Board>_<release>_... .img.xz
function boardOf(name) {
  const p = name.split('_');
  if (p.length < 4 || p[2] === 'LiveUSB') return null;   // skip the x86 Live USB variant
  return p[3].toLowerCase();
}

const images = [];
const usedDevices = new Set();

for (const rel of RELEASES) {
  const assets = fetchAssets(rel.tag);
  const wantSha = new Set(assets.filter((a) => a.name.endsWith('.img.xz')).map((a) => a.name));
  // pull the .sha256 sidecars so we can publish download_sha256. The download is
  // likewise flaky, so retry into the same dir until every image has a hash.
  const dir = mkdtempSync(join(tmpdir(), 'awcat-'));
  const shaByFile = {};
  for (let i = 0; i < 4; i++) {
    try { gh('release', 'download', rel.tag, '--repo', REPO, '--dir', dir, '--pattern', '*.img.xz.sha256', '--clobber'); } catch {}
    for (const f of readdirSync(dir)) {
      const txt = readFileSync(join(dir, f), 'utf8').trim();
      const [hash, fname] = txt.split(/\s+/);
      if (hash && fname) shaByFile[fname.replace(/^\*/, '')] = hash;
    }
    if ([...wantSha].every((n) => shaByFile[n])) break;
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
