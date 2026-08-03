# World's End Launcher (.exe)

Полностью автономное desktop-приложение (Tauri 2, Rust + React). **Никакого
сайта/сервера больше нет** — лаунчер сам:

- читает `manifest.json` из вашего GitHub-репозитория (версия сборки,
  версия Minecraft, ядро, версия ядра, список модов) — публично, без токена;
- ставит ядро (запускает официальный installer.jar с `--installClient`);
- скачивает/обновляет/удаляет моды по SHA-256, файлы которых лежат в
  GitHub Releases того же репозитория;
- реальным пингом (протокол Server List Ping) проверяет онлайн ли сервер
  и сколько игроков — тоже без бэкенда;
- запускает Minecraft, читая настоящий JSON-профиль версии от
  Forge/NeoForge (а не угадывая аргументы запуска);
- автоматически подключается к `185.219.84.148:30909`.

Когда вы меняете `manifest.json` в репозитории — **все** установленные у
игроков `.exe` подхватывают изменения при следующем запуске. Пересобирать
`.exe` для этого не нужно.

## ⚠️ Про токен, который вы прислали в чат

Вы вставили настоящий `GITHUB_TOKEN` с правом записи прямо в переписку.
**Обязательно отзовите его**: github.com → Settings → Developer settings →
Personal access tokens → найдите его → Delete, и создайте новый.

Токен с правом записи **никогда** не встраивается в `.exe`, который
раздаётся игрокам — это позволило бы кому угодно вытащить его из файла
программы и получить доступ к вашему репозиторию на запись. Поэтому:

- `.exe` делает только **публичные, анонимные чтения** (raw.githubusercontent.com
  + скачивание файлов из Releases) — токен ему не нужен вообще.
- Токен нужен только вам, локально, для `admin-tool/` (см. ниже) — он
  никогда никуда не публикуется и не попадает в собранный `.exe`.

## Настройка репозитория (один раз)

1. Репозиторий `worldsend-mods` должен быть **публичным** (там не будет
   ничего секретного — только версия сборки и файлы модов).
2. В нём можно хранить и код лаунчера (эту папку), и `manifest.json`, и
   релиз с файлами модов — всё в одном репозитории, они друг другу не мешают.
3. Инициализируйте сборку (см. ниже, `admin-tool`).

## Как обновлять сборку — `admin-tool/`

Маленький CLI-скрипт, который вы запускаете **на своём компьютере**
(токен остаётся только у вас):

```bash
cd admin-tool
cp .env.example .env
# впишите в .env новый (свежий!) GITHUB_TOKEN, GITHUB_OWNER=m500009876, GITHUB_REPO=worldsend-mods

# Один раз — создать manifest.json:
node admin.js init --mcVersion 1.21.1 --loader NeoForge --loaderVersion 21.4.0-beta \
  --loaderUrl "https://maven.neoforged.net/releases/net/neoforged/neoforge/21.4.0-beta/neoforge-21.4.0-beta-installer.jar"

# Добавить/обновить мод (сам загрузит файл в Releases и посчитает SHA-256):
node admin.js add-mod --file ./sodium.jar --modId sodium --name "Sodium" --modVersion 0.5.8

# Удалить мод из сборки:
node admin.js remove-mod --modId sodium

# Сменить версию сборки/ядро:
node admin.js set-build --version v1.3.0 --loaderVersion 21.5.0

# Посмотреть текущую сборку:
node admin.js list
```

### Большая сборка — загрузить сразу всю папку `mods`

Вместо `add-mod` по одному файлу — одна команда на всю папку:

```bash
node admin.js add-folder --folder ./mods
```

Она сама:
- находит все `.jar` в папке;
- для каждого пытается угадать `modId`/название/версию по имени файла
  (например `sodium-fabric-0.5.8.jar` → modId `sodium`, версия `0.5.8`);
- грузит все файлы в GitHub Releases и одним коммитом обновляет `manifest.json`.

Названия файлов у модов не стандартизированы, поэтому угадывание версии
иногда промахивается (особенно если в имени файла несколько чисел — версия
мода и версия Minecraft). Скрипт печатает, что определил для каждого
файла — проверьте вывод. Если что-то не так, поправьте точечно:
```bash
node admin.js add-mod --file ./mods/тот-самый.jar --modId правильный-id --name "Правильное имя" --modVersion 1.2.3
```
(тот же `modId` перезапишет неверную запись из пакетной загрузки).

Требуется Node.js 18+ (используется встроенный `fetch`, зависимостей нет).
Всё это — обычные коммиты в ваш репозиторий, историю изменений видно на github.com.

## Сборка `.exe`

### Способ 1: GitHub Actions (проще всего)

Workflow уже лежит в `.github/workflows/build.yml`. В репозитории:
```bash
git tag v1.0.0
git push origin v1.0.0
```
Через несколько минут в **Releases** появится черновик с `.exe`/`.msi`.

### Способ 2: локально на Windows

```powershell
# Node.js 20+, Rust (rustup.rs), Visual Studio Build Tools (C++ workload)
npm install
npm run tauri build
# Готово: src-tauri/target/release/bundle/nsis/*.exe
```

Репозиторий (`m500009876/worldsend-mods`) уже зашит в код по умолчанию
(`src-tauri/src/config.rs`) — пересобирать с флагами не нужно, если не
меняете название репозитория.

## Требования на компьютере игрока

- Java 21+ (лаунчер проверяет и предупреждает, если не найдена)
- Windows 10/11

## Структура

```
src-tauri/src/
  main.rs, lib.rs      — точка входа, Tauri-команды
  config.rs            — owner/repo GitHub, IP сервера (публичные данные)
  models.rs            — структуры манифеста/новостей/настроек
  api.rs               — чтение manifest.json/news.json с GitHub (без токена)
  mc_ping.rs           — реальный пинг Minecraft-сервера (Server List Ping)
  java.rs               — поиск установленной Java
  launcher.rs           — установка ядра, синхронизация модов, запуск игры
  version_profile.rs    — разбор JSON-профиля версии Forge/NeoForge
src/                    — React-интерфейс (Главная / Новости / Настройки)
admin-tool/             — локальный CLI для обновления manifest.json (не часть .exe)
```

## Формат `manifest.json` (для справки/ручного редактирования)

```json
{
  "version": "v1.3.0",
  "mcVersion": "1.21.1",
  "loader": "NeoForge",
  "loaderVersion": "21.4.0-beta",
  "neoForgeUrl": "https://.../neoforge-21.4.0-beta-installer.jar",
  "publishedAt": "2026-08-01T12:00:00.000Z",
  "mods": [
    {
      "fileName": "sodium-fabric-0.5.8.jar",
      "modId": "sodium",
      "displayName": "Sodium",
      "version": "0.5.8",
      "downloadUrl": "https://github.com/.../releases/download/mods-storage/....jar",
      "sha256": "...",
      "fileSizeBytes": 1234567,
      "required": true
    }
  ]
}
```

`news.json` (необязательный, в том же репозитории) — массив объектов
`{"id","title","content","tag","createdAt"}`.

## Известные ограничения

- Установка Java "в один клик" не реализована — лаунчер только проверяет
  её наличие.
- Авто-обновление самого `.exe` не подключено (можно добавить через
  `tauri-plugin-updater` при необходимости).
- Полностью протестированы современные версии MC (1.13+, формат
  `arguments.jvm/game`). Для очень старых версий есть базовый fallback,
  без гарантий.
