#!/usr/bin/env node
// Admin CLI — manages the modpack manifest + mod files stored in your
// GitHub repo, replacing the old web admin panel.
//
// Runs LOCALLY on your computer only. Your GITHUB_TOKEN never leaves
// this script and is never embedded in the .exe — the launcher only
// ever does public, unauthenticated reads.
//
// Setup:
//   1. cp .env.example .env
//   2. Fill in GITHUB_TOKEN / GITHUB_OWNER / GITHUB_REPO in .env
//   3. node admin.js <command> ...
//
// Commands:
//   node admin.js init --mcVersion 1.21.1 --loader NeoForge --loaderVersion 21.1.236 [--loaderUrl <installer.jar url>]
//   node admin.js set-build --version v1.3.0 [--mcVersion 1.21.1] [--loader NeoForge] [--loaderVersion 21.1.236] [--loaderUrl ...]
//   node admin.js add-mod --file ./sodium.jar --modId sodium --name "Sodium" --modVersion 0.5.8 [--optional]
//   node admin.js add-folder --folder ./mods            ← загружает ВСЕ .jar из папки разом
//   node admin.js set-overrides --folder "./доп файлы"  ← config/kubejs/emotes/др. (не моды)
//   node admin.js remove-mod --modId sodium
//   node admin.js list

import { readFileSync, existsSync, readdirSync, statSync } from "node:fs";
import { createZipFromFolder } from "./zipper.js";
import { createHash } from "node:crypto";
import path from "node:path";
import process from "node:process";

// ── tiny .env loader (no dependency needed) ────────────────────────
function loadEnv() {
  const envPath = path.join(process.cwd(), ".env");
  if (!existsSync(envPath)) return;
  for (const line of readFileSync(envPath, "utf8").split("\n")) {
    const trimmed = line.trim();
    if (!trimmed || trimmed.startsWith("#")) continue;
    const eq = trimmed.indexOf("=");
    if (eq === -1) continue;
    const key = trimmed.slice(0, eq).trim();
    const value = trimmed.slice(eq + 1).trim();
    if (!(key in process.env)) process.env[key] = value;
  }
}
loadEnv();

const TOKEN = process.env.GITHUB_TOKEN;
const OWNER = process.env.GITHUB_OWNER;
const REPO = process.env.GITHUB_REPO;
const BRANCH = process.env.GITHUB_BRANCH || "main";
const RELEASE_TAG = process.env.GITHUB_RELEASE_TAG || "mods-storage";

function checkEnv() {
  if (!TOKEN || !OWNER || !REPO) {
    throw new Error(
      "Не заданы GITHUB_TOKEN / GITHUB_OWNER / GITHUB_REPO. Откройте admin-tool/.env и заполните их."
    );
  }
}

const API = "https://api.github.com";
const headers = {
  Authorization: `Bearer ${TOKEN}`,
  Accept: "application/vnd.github+json",
  "X-GitHub-Api-Version": "2022-11-28",
};

async function gh(pathname, options = {}) {
  const res = await fetch(`${API}${pathname}`, { ...options, headers: { ...headers, ...(options.headers || {}) } });
  return res;
}

// ── manifest.json read/write via Contents API ──────────────────────
async function getManifest() {
  const res = await gh(`/repos/${OWNER}/${REPO}/contents/manifest.json?ref=${BRANCH}`);
  if (res.status === 404) return { manifest: null, sha: null };
  if (!res.ok) throw new Error(`Не удалось прочитать manifest.json: ${res.status} ${await res.text()}`);
  const json = await res.json();
  const content = Buffer.from(json.content, "base64").toString("utf8");
  return { manifest: JSON.parse(content), sha: json.sha };
}

async function saveManifest(manifest, sha, message) {
  const content = Buffer.from(JSON.stringify(manifest, null, 2)).toString("base64");
  const res = await gh(`/repos/${OWNER}/${REPO}/contents/manifest.json`, {
    method: "PUT",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      message,
      content,
      branch: BRANCH,
      ...(sha ? { sha } : {}),
    }),
  });
  if (!res.ok) throw new Error(`Не удалось сохранить manifest.json: ${res.status} ${await res.text()}`);
  console.log("✔ manifest.json обновлён");
}

// ── GitHub Release asset storage for mod jars ───────────────────────
async function getOrCreateRelease() {
  let res = await gh(`/repos/${OWNER}/${REPO}/releases/tags/${RELEASE_TAG}`);
  if (res.ok) return res.json();
  if (res.status !== 404) throw new Error(`Ошибка получения релиза: ${res.status} ${await res.text()}`);

  res = await gh(`/repos/${OWNER}/${REPO}/releases`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      tag_name: RELEASE_TAG,
      name: "Mod files storage (auto-managed, do not delete)",
      body: "Служебный релиз: сюда admin.js загружает файлы модов. Не удаляйте.",
      draft: false,
      prerelease: false,
    }),
  });
  if (!res.ok) throw new Error(`Не удалось создать релиз: ${res.status} ${await res.text()}`);
  return res.json();
}

async function deleteAsset(assetId) {
  await gh(`/repos/${OWNER}/${REPO}/releases/assets/${assetId}`, { method: "DELETE" }).catch(() => {});
}

async function uploadBufferAsAsset(buffer, fileName, contentType) {
  const release = await getOrCreateRelease();
  const existing = release.assets.find((a) => a.name === fileName);
  if (existing) {
    console.log(`  найден старый файл ${fileName} в релизе, заменяю...`);
    await deleteAsset(existing.id);
  }

  const uploadUrl = release.upload_url.replace(/{.*}$/, "");
  const res = await fetch(`${uploadUrl}?name=${encodeURIComponent(fileName)}`, {
    method: "POST",
    headers: {
      Authorization: `Bearer ${TOKEN}`,
      Accept: "application/vnd.github+json",
      "Content-Type": contentType,
      "Content-Length": String(buffer.byteLength),
    },
    body: buffer,
  });
  if (!res.ok) throw new Error(`Не удалось загрузить файл: ${res.status} ${await res.text()}`);
  const asset = await res.json();
  return { downloadUrl: asset.browser_download_url };
}

async function uploadModFile(filePath, fileName) {
  const buffer = readFileSync(filePath);
  const sha256 = createHash("sha256").update(buffer).digest("hex");
  const fileSizeBytes = buffer.byteLength;
  const { downloadUrl } = await uploadBufferAsAsset(buffer, fileName, "application/java-archive");
  return { downloadUrl, sha256, fileSizeBytes };
}

function guessModInfoFromFileName(fileName) {
  const base = fileName.replace(/\.jar$/i, "");

  // Collect every version-looking token, e.g. "1.21.1", "0.5.8", "15.1.2-beta".
  // Filenames usually go modname-loader-mcversion-modversion.jar, so the
  // FIRST token marks where the mod's own name ends, and the LAST token is
  // the actual mod version (mc version tends to sit in the middle).
  const versionRegex = /\d+(?:\.\d+){1,3}(?:-(?:beta|alpha|rc\d*|pre\d*))?/gi;
  const matches = [...base.matchAll(versionRegex)];

  const version = matches.length > 0 ? matches[matches.length - 1][0] : "unknown";
  const firstMatch = matches.length > 0 ? matches[0] : null;

  let namePart = firstMatch ? base.slice(0, firstMatch.index) : base;
  namePart = namePart.replace(
    /[-_\s](fabric|forge|neoforge|quilt|mc|for)?[-_\s]?$/i,
    ""
  );
  namePart = namePart.replace(/[-_]+$/, "");
  if (!namePart) namePart = base;

  const modId = namePart.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-+|-+$/g, "") || "mod";
  const displayName = namePart
    .replace(/[-_]+/g, " ")
    .trim()
    .split(" ")
    .filter(Boolean)
    .map((w) => w.charAt(0).toUpperCase() + w.slice(1))
    .join(" ") || fileName;

  return { modId, displayName, version };
}


function parseArgs(argv) {
  const args = { _: [] };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a.startsWith("--")) {
      const key = a.slice(2);
      const next = argv[i + 1];
      if (next !== undefined && !next.startsWith("--")) {
        args[key] = next;
        i++;
      } else {
        args[key] = true;
      }
    } else {
      args._.push(a);
    }
  }
  return args;
}

function requireManifest(manifest) {
  if (!manifest) {
    throw new Error(
      "manifest.json ещё не создан. Сначала выполните: node admin.js init --mcVersion ... --loader ... --loaderVersion ..."
    );
  }
}

async function main() {
  checkEnv();
  const args = parseArgs(process.argv.slice(2));
  const command = args._[0];

  if (command === "init") {
    const { manifest: existing } = await getManifest();
    if (existing) {
      throw new Error("manifest.json уже существует. Используйте set-build для изменения.");
    }
    if (!args.mcVersion || !args.loader || !args.loaderVersion) {
      throw new Error("Нужны флаги: --mcVersion --loader --loaderVersion (и опционально --loaderUrl)");
    }
    const manifest = {
      version: args.version || "v1.0.0",
      mcVersion: args.mcVersion,
      loader: args.loader,
      loaderVersion: args.loaderVersion,
      neoForgeUrl: args.loaderUrl || "",
      publishedAt: new Date().toISOString(),
      mods: [],
    };
    await saveManifest(manifest, null, "init manifest.json");
    console.log("Готово. Теперь добавляйте моды: node admin.js add-mod --file ... --modId ... --name ... --modVersion ...");
    return;
  }

  if (command === "set-build") {
    const { manifest, sha } = await getManifest();
    requireManifest(manifest);
    if (args.version) manifest.version = args.version;
    if (args.mcVersion) manifest.mcVersion = args.mcVersion;
    if (args.loader) manifest.loader = args.loader;
    if (args.loaderVersion) manifest.loaderVersion = args.loaderVersion;
    if (args.loaderUrl) manifest.neoForgeUrl = args.loaderUrl;
    manifest.publishedAt = new Date().toISOString();
    await saveManifest(manifest, sha, `set-build ${manifest.version}`);
    console.log("Все установленные .exe подхватят это при следующем запуске игры.");
    return;
  }

  if (command === "add-mod") {
    const { manifest, sha } = await getManifest();
    requireManifest(manifest);
    if (!args.file || !args.modId || !args.name || !args.modVersion) {
      throw new Error("Нужны флаги: --file --modId --name --modVersion [--optional]");
    }
    if (!existsSync(args.file)) {
      throw new Error(`Файл не найден: ${args.file}`);
    }
    const fileName = path.basename(args.file);
    console.log(`Загружаю ${fileName} в GitHub Releases...`);
    const { downloadUrl, sha256, fileSizeBytes } = await uploadModFile(args.file, fileName);

    const entry = {
      fileName,
      modId: args.modId,
      displayName: args.name,
      version: args.modVersion,
      downloadUrl,
      sha256,
      fileSizeBytes,
      required: !args.optional,
    };

    const idx = manifest.mods.findIndex((m) => m.modId === args.modId);
    if (idx >= 0) manifest.mods[idx] = entry;
    else manifest.mods.push(entry);

    manifest.publishedAt = new Date().toISOString();
    await saveManifest(manifest, sha, `add-mod ${args.modId} ${args.modVersion}`);
    console.log(`✔ Мод "${args.name}" (${args.modVersion}) добавлен в сборку.`);
    return;
  }

  if (command === "add-folder") {
    const { manifest, sha } = await getManifest();
    requireManifest(manifest);
    if (!args.folder) {
      throw new Error("Нужен флаг: --folder ./mods");
    }
    if (!existsSync(args.folder) || !statSync(args.folder).isDirectory()) {
      throw new Error(`Папка не найдена: ${args.folder}`);
    }

    const files = readdirSync(args.folder).filter((f) => f.toLowerCase().endsWith(".jar"));
    if (files.length === 0) {
      throw new Error(`В папке ${args.folder} не найдено .jar файлов.`);
    }

    console.log(`Найдено ${files.length} файлов. Загружаю...\n`);

    let added = 0;
    let updated = 0;
    for (const fileName of files) {
      const filePath = path.join(args.folder, fileName);
      const guess = guessModInfoFromFileName(fileName);

      process.stdout.write(`  ${fileName} → modId="${guess.modId}", версия="${guess.version}" ... `);
      const { downloadUrl, sha256, fileSizeBytes } = await uploadModFile(filePath, fileName);

      const entry = {
        fileName,
        modId: guess.modId,
        displayName: guess.displayName,
        version: guess.version,
        downloadUrl,
        sha256,
        fileSizeBytes,
        required: true,
      };

      const idx = manifest.mods.findIndex((m) => m.modId === guess.modId);
      if (idx >= 0) {
        manifest.mods[idx] = entry;
        updated++;
      } else {
        manifest.mods.push(entry);
        added++;
      }
      console.log("готово");
    }

    manifest.publishedAt = new Date().toISOString();
    await saveManifest(manifest, sha, `add-folder: ${files.length} файлов`);
    console.log(`\n✔ Загружено: ${added} новых, ${updated} обновлено.`);
    console.log(
      "Если какой-то modId/версия определились неверно — поправьте вручную командой:\n" +
      '  node admin.js add-mod --file ... --modId ... --name "..." --modVersion ...\n' +
      "(тот же modId перезапишет запись из этой пакетной загрузки)."
    );
    return;
  }

  if (command === "set-overrides") {
    const { manifest, sha } = await getManifest();
    requireManifest(manifest);
    if (!args.folder) {
      throw new Error('Нужен флаг: --folder "./доп файлы" (папка, где ЛЕЖАТ config/kubejs/emotes/... напрямую, без обёртки)');
    }
    if (!existsSync(args.folder) || !statSync(args.folder).isDirectory()) {
      throw new Error(`Папка не найдена: ${args.folder}`);
    }

    console.log(`Архивирую ${args.folder}...`);
    const zipBuffer = createZipFromFolder(args.folder);
    const sha256 = createHash("sha256").update(zipBuffer).digest("hex");
    console.log(`Архив собран: ${(zipBuffer.length / 1024 / 1024).toFixed(1)} МБ. Загружаю в GitHub Releases...`);

    const { downloadUrl } = await uploadBufferAsAsset(zipBuffer, "overrides.zip", "application/zip");

    manifest.overridesUrl = downloadUrl;
    manifest.overridesSha256 = sha256;
    manifest.publishedAt = new Date().toISOString();
    await saveManifest(manifest, sha, "set-overrides");
    console.log("✔ Дополнительные файлы (config/kubejs/emotes/...) подключены к сборке.");
    console.log("Лаунчер распакует их в папку игры при следующем запуске у всех игроков.");
    return;
  }

  if (command === "remove-mod") {
    const { manifest, sha } = await getManifest();
    requireManifest(manifest);
    if (!args.modId) {
      throw new Error("Нужен флаг: --modId");
    }
    const idx = manifest.mods.findIndex((m) => m.modId === args.modId);
    if (idx === -1) {
      throw new Error(`Мод с modId="${args.modId}" не найден в сборке.`);
    }
    manifest.mods.splice(idx, 1);
    manifest.publishedAt = new Date().toISOString();
    await saveManifest(manifest, sha, `remove-mod ${args.modId}`);
    console.log(`✔ Мод "${args.modId}" удалён из сборки (файл в Releases можно удалить вручную).`);
    return;
  }

  if (command === "list") {
    const { manifest } = await getManifest();
    requireManifest(manifest);
    console.log(`Сборка: ${manifest.version} | MC ${manifest.mcVersion} | ${manifest.loader} ${manifest.loaderVersion}`);
    console.log(`Ядро: ${manifest.neoForgeUrl || "(не задано!)"}`);
    console.log(`Доп. файлы (config/kubejs/...): ${manifest.overridesUrl ? "заданы" : "не заданы"}`);
    console.log(`Модов: ${manifest.mods.length}`);
    for (const m of manifest.mods) {
      console.log(`  - [${m.modId}] ${m.displayName} ${m.version}${m.required ? "" : " (опционально)"}`);
    }
    return;
  }

  console.log(`Использование:
  node admin.js init --mcVersion 1.21.1 --loader NeoForge --loaderVersion 21.1.236 [--loaderUrl <url>]
  node admin.js set-build [--version v1.3.0] [--mcVersion ...] [--loader ...] [--loaderVersion ...] [--loaderUrl ...]
  node admin.js add-mod --file ./mod.jar --modId sodium --name "Sodium" --modVersion 0.5.8 [--optional]
  node admin.js add-folder --folder ./mods
  node admin.js set-overrides --folder "./доп файлы"
  node admin.js remove-mod --modId sodium
  node admin.js list`);
}

main().catch((e) => {
  console.error("Ошибка:", e.message);
  process.exitCode = 1;
});
