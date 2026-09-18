// Run only in the protected-by-job-conditions post-test GitHub Actions job.
// Never replace a published release or clobber an asset with different bytes.
import { readFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';

const repository = process.env.GITHUB_REPOSITORY;
const commit = process.env.GITHUB_SHA;
const version = process.env.NODIFORM_VERSION;
const token = process.env.GH_TOKEN;
if (repository !== 'bodee-angus/nodiform' || !/^[a-f0-9]{40}$/.test(commit ?? '') ||
    !/^\d+\.\d+\.\d+$/.test(version ?? '') || !token) {
  throw new Error('Missing or invalid release context. Run from the designated GitHub Actions job.');
}
const tag = `v${version}`;
const base = `https://api.github.com/repos/${repository}`;
const headers = { Authorization: `Bearer ${token}`, Accept: 'application/vnd.github+json', 'X-GitHub-Api-Version': '2022-11-28' };

async function request(url, options = {}) {
  const response = await fetch(url, { ...options, headers: { ...headers, ...options.headers }, signal: AbortSignal.timeout(120_000) });
  if (!response.ok) {
    // Do not log headers, token, temporary upload URLs, or remote response bodies.
    throw new Error(`GitHub ${options.method ?? 'GET'} failed with HTTP ${response.status}.`);
  }
  return response.json();
}

async function isCurrentMain() {
  const main = await request(`${base}/git/ref/heads/main`);
  return main.object.type === 'commit' && main.object.sha === commit;
}

if (!await isCurrentMain()) {
  console.log('A newer commit is now on main. This older build will not change the update channel.');
  process.exit(0);
}

// target_commitish does not move an existing tag. Never attach binaries to an
// unrelated source revision, including through an annotated tag.
const tagResponse = await fetch(`${base}/git/ref/tags/${tag}`, { headers, signal: AbortSignal.timeout(30_000) });
if (tagResponse.ok) {
  let object = (await tagResponse.json()).object;
  for (let depth = 0; object.type === 'tag' && depth < 8; depth++) {
    object = (await request(`${base}/git/tags/${object.sha}`)).object;
  }
  if (object.type !== 'commit' || object.sha !== commit) {
    // An already published version is intentionally immutable and is allowed
    // to belong to its original commit. The release lookup below will skip it.
    const publishedTag = await fetch(`${base}/releases/tags/${tag}`, { headers, signal: AbortSignal.timeout(30_000) });
    if (publishedTag.ok && !(await publishedTag.json()).draft) {
      console.log(`${tag} is already published. Bump Cargo.toml to publish another version.`);
      process.exit(0);
    }
    throw new Error('The existing release tag does not resolve to this tested commit. It has not been moved.');
  }
} else if (tagResponse.status !== 404) {
  throw new Error(`Cannot inspect release tag: HTTP ${tagResponse.status}.`);
}

// Check all required files before creating even a draft release.
const assets = await Promise.all(['Nodiform-x86_64.AppImage', 'Nodiform-x86_64.AppImage.zsync', 'SHA256SUMS'].map(async name => {
  const bytes = await readFile(new URL(`../dist/${name}`, import.meta.url));
  if (!bytes.length) throw new Error(`Empty release asset: ${name}`);
  return { name, bytes, digest: `sha256:${createHash('sha256').update(bytes).digest('hex')}` };
}));

let release;
// Authenticated listing includes drafts; the by-tag endpoint promises only
// published releases. Paginate so an interrupted draft can be resumed safely.
for (let page = 1; page <= 10; page++) {
  const releases = await request(`${base}/releases?per_page=100&page=${page}`);
  release = releases.find(item => item.tag_name === tag);
  if (release || releases.length < 100) break;
  if (page === 10) throw new Error('Release history exceeds the bounded lookup. Review the channel manually.');
}
if (release) {
  if (!release.draft) {
    console.log(`${tag} is already published. Its assets are immutable; bump Cargo.toml for a new release.`);
    process.exit(0);
  }
  if (release.target_commitish !== commit) {
    throw new Error('An existing draft belongs to a different commit. Review it before publishing.');
  }
} else {
  const latestResponse = await fetch(`${base}/releases/latest`, { headers, signal: AbortSignal.timeout(30_000) });
  if (latestResponse.ok) {
    const latest = await latestResponse.json();
    if (/^v\d+\.\d+\.\d+$/.test(latest.tag_name)) {
      const prior = latest.tag_name.slice(1).split('.').map(BigInt);
      const next = version.split('.').map(BigInt);
      const differing = next.findIndex((value, index) => value !== prior[index]);
      if (differing < 0 || next[differing] < prior[differing]) {
        throw new Error('Version must increase beyond the currently published channel version.');
      }
    }
  } else if (latestResponse.status !== 404) {
    throw new Error(`Cannot inspect latest version: HTTP ${latestResponse.status}.`);
  }
  const body = [
    '# Experimental alpha',
    '',
    'Native 2D graph experiments for Bazzite/Linux x86_64. This is an early testing build, not a stable release.',
    '',
    'Download **Nodiform-x86_64.AppImage** and open it with Gear Lever to add it to application search. Allow it to replace the older version when updating.',
    '',
    'The AppImage contains Nodiform, its icon, examples, and documentation. Vulkan graphics drivers and FFmpeg are supplied by the host. Preview does not require FFmpeg; recording requires a working host FFmpeg with libx264 or supported NVIDIA NVENC.',
    '',
    'Saved project files and recordings remain outside the AppImage. Updating the application does not remove them. This package does not sandbox the application.',
    '',
    'CI checks unit tests, software-Vulkan computation/rendering, and the packaged native window/rule worker under Xvfb. Bazzite and physical GPU testing remain separate.',
    '',
    'The regular GitHub release channel is used for standard AppImage/Gear Lever update discovery. “Latest” identifies the current alpha, not production maturity.',
    '',
    `Source commit: ${commit}`,
  ].join('\n');
  release = await request(`${base}/releases`, {
    method: 'POST', headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ tag_name: tag, target_commitish: commit, name: `Nodiform ${version} · Experimental Alpha`, body, draft: true, prerelease: false }),
  });
}

const upload = new URL(release.upload_url.replace('{?name,label}', ''));
if (upload.origin !== 'https://uploads.github.com') throw new Error('Unexpected upload origin.');
for (const asset of assets) {
  const present = release.assets.find(item => item.name === asset.name);
  if (present) {
    if (present.size !== asset.bytes.length || present.digest !== asset.digest || present.state !== 'uploaded') {
      throw new Error(`Existing draft asset has different bytes: ${asset.name}. It has not been overwritten.`);
    }
    continue;
  }
  upload.searchParams.set('name', asset.name);
  const uploaded = await request(upload, { method: 'POST', headers: { 'Content-Type': 'application/octet-stream' }, body: asset.bytes });
  if (uploaded.size !== asset.bytes.length || uploaded.digest !== asset.digest || uploaded.state !== 'uploaded') {
    throw new Error(`Uploaded asset failed digest verification: ${asset.name}. Release remains a draft.`);
  }
}
if (!await isCurrentMain()) {
  console.log('Main changed during upload. Leaving this release as a draft; the public channel is unchanged.');
  process.exit(0);
}
const published = await request(`${base}/releases/${release.id}`, {
  method: 'PATCH', headers: { 'Content-Type': 'application/json' },
  body: JSON.stringify({ draft: false, prerelease: false, make_latest: 'true' }),
});
console.log(`Published ${published.html_url}`);
