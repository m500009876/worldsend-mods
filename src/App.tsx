import { useEffect, useState, useCallback } from "react";
import {
  api,
  Manifest,
  ServerStatus,
  LaunchSettings,
  LaunchProgress,
} from "./api";

type Tab = "home" | "settings";

export default function App() {
  const [tab, setTab] = useState<Tab>("home");
  const [manifest, setManifest] = useState<Manifest | null>(null);
  const [status, setStatus] = useState<ServerStatus | null>(null);
  const [settings, setSettings] = useState<LaunchSettings>({ nickname: "Player", ramGb: 6 });
  const [javaOk, setJavaOk] = useState<boolean | null>(null);
  const [systemRamGb, setSystemRamGb] = useState(0);

  const [launching, setLaunching] = useState(false);
  const [gameRunning, setGameRunning] = useState(false);
  const [progressLabel, setProgressLabel] = useState("");
  const [progressPct, setProgressPct] = useState(0);
  const [error, setError] = useState("");

  const loadAll = useCallback(async () => {
    try {
      const [m, s, st, j, ram] = await Promise.all([
        api.getManifest(),
        api.getServerStatus().catch(() => ({ online: false })),
        api.getSettings(),
        api.checkJava(),
        api.getSystemRamGb().catch(() => 0),
      ]);
      setManifest(m);
      setStatus(s);
      setSettings(st);
      setJavaOk(j);
      setSystemRamGb(ram);
    } catch (e) {
      setError(String(e));
    }
  }, []);

  useEffect(() => {
    loadAll();
    const unlisten = api.onLaunchProgress((p: LaunchProgress) => {
      switch (p.stage) {
        case "Checking":
          setProgressLabel(p.data.message);
          setProgressPct(5);
          break;
        case "InstallingJava":
          setProgressLabel(p.data.message);
          setProgressPct(10);
          break;
        case "InstallingLoader":
          setProgressLabel(p.data.message);
          setProgressPct(15);
          break;
        case "InstallingOverrides":
          setProgressLabel(p.data.message);
          setProgressPct(18);
          break;
        case "Downloading": {
          const pct = p.data.total ? Math.round((p.data.downloaded / p.data.total) * 100) : undefined;
          setProgressLabel(
            pct !== undefined ? `Загрузка: ${p.data.name} (${pct}%)` : `Загрузка: ${p.data.name}`
          );
          break;
        }
        case "SyncingMods":
          setProgressLabel(`Синхронизация модов: ${p.data.name} (${p.data.current}/${p.data.total})`);
          setProgressPct(20 + Math.round((p.data.current / Math.max(1, p.data.total)) * 70));
          break;
        case "DeletingMod":
          setProgressLabel(`Удаление устаревшего мода: ${p.data.name}`);
          break;
        case "Ready":
          setProgressLabel("Готово, запуск игры...");
          setProgressPct(95);
          break;
        case "Launching":
          // Minecraft process has spawned and stayed alive — the launcher
          // stays disabled while the game is running and only re-enables
          // once the "Closed" event confirms the game window was closed.
          setProgressLabel("Minecraft запущен!");
          setProgressPct(100);
          setGameRunning(true);
          break;
        case "Closed":
          setLaunching(false);
          setGameRunning(false);
          setProgressLabel("");
          setProgressPct(0);
          break;
        case "Error":
          setError(p.data.message);
          setLaunching(false);
          setGameRunning(false);
          break;
      }
    });
    return () => {
      unlisten.then((f) => f());
    };
  }, [loadAll]);

  // Never let the slider go above what the PC actually has installed
  // (leaving the OS at least 1GB), falling back to 16 if we couldn't
  // detect the system's RAM at all.
  const ramSliderMax = systemRamGb > 2 ? Math.max(2, systemRamGb - 1) : 16;

  const handleSaveSettings = async (next: LaunchSettings) => {
    setSettings(next);
    await api.saveSettings(next);
  };

  const handlePlay = async () => {
    setError("");
    setLaunching(true);
    setGameRunning(false);
    setProgressPct(0);
    setProgressLabel("Запуск...");
    try {
      await api.startLaunch(settings);
      // Once startLaunch resolves the "launch-progress" events (Launching /
      // Closed / Error) drive the rest of the button state — see the
      // listener above.
    } catch (e) {
      setError(String(e));
      setLaunching(false);
    }
  };

  return (
    <div className="app">
      <header className="topbar">
        <div className="brand">
          <span className="brand-mark">W</span>
          <span>Project World&apos;s End</span>
        </div>
        <nav className="tabs">
          <button className={tab === "home" ? "active" : ""} onClick={() => setTab("home")}>
            Главная
          </button>
          <button className={tab === "settings" ? "active" : ""} onClick={() => setTab("settings")}>
            Настройки
          </button>
        </nav>
        <div className={`server-pill ${status?.online ? "online" : "offline"}`}>
          <span className="dot" />
          {status?.online
            ? `Онлайн${status.players ? ` · ${status.players.online}/${status.players.max}` : ""}`
            : "Оффлайн"}
        </div>
      </header>

      <main className="content">
        {tab === "home" && (
          <section className="home">
            <div className="card build-card">
              <h2>Текущая сборка</h2>
              {manifest ? (
                <div className="build-grid">
                  <div>
                    <span className="label">Версия сборки</span>
                    <span className="value">{manifest.version}</span>
                  </div>
                  <div>
                    <span className="label">Minecraft</span>
                    <span className="value">{manifest.mcVersion}</span>
                  </div>
                  <div>
                    <span className="label">Ядро</span>
                    <span className="value">{manifest.loader}</span>
                  </div>
                  <div>
                    <span className="label">Версия ядра</span>
                    <span className="value">{manifest.loaderVersion}</span>
                  </div>
                  <div>
                    <span className="label">Модов в сборке</span>
                    <span className="value">{manifest.mods.length}</span>
                  </div>
                </div>
              ) : (
                <p className="muted">Загрузка данных сборки...</p>
              )}
              <p className="muted small">
                Эти настройки задаются администратором и применяются автоматически —
                менять их вручную не нужно.
              </p>
            </div>

            {javaOk === false && (
              <div className="alert">
                Java не найдена на этом компьютере. Установите Java {"21"}+ и перезапустите
                лаунчер.
              </div>
            )}
            {error && <div className="alert alert-error">{error}</div>}

            <div className="play-block">
              {(launching || gameRunning) && (
                <div className="progress">
                  <div className="progress-bar">
                    <div className="progress-fill" style={{ width: `${progressPct}%` }} />
                  </div>
                  <p className="progress-label">{progressLabel}</p>
                </div>
              )}
              <button
                className="play-button"
                onClick={handlePlay}
                disabled={launching || gameRunning || !manifest}
              >
                {gameRunning ? "MINECRAFT ЗАПУЩЕН" : launching ? "Идёт запуск..." : "ИГРАТЬ"}
              </button>
              <p className="muted small">
                {settings.nickname} · {settings.ramGb} GB RAM
              </p>
            </div>
          </section>
        )}

        {tab === "settings" && (
          <section className="settings">
            <div className="card">
              <h2>Никнейм</h2>
              <input
                type="text"
                value={settings.nickname}
                onChange={(e) => handleSaveSettings({ ...settings, nickname: e.target.value })}
                maxLength={16}
              />
            </div>
            <div className="card">
              <h2>Оперативная память: {settings.ramGb} GB</h2>
              <input
                type="range"
                min={2}
                max={ramSliderMax}
                step={1}
                value={settings.ramGb}
                onChange={(e) =>
                  handleSaveSettings({ ...settings, ramGb: Number(e.target.value) })
                }
              />
              <div className="range-labels">
                <span>2</span>
                <span>{ramSliderMax}</span>
              </div>
              <p className="muted small">
                {systemRamGb > 0
                  ? `На этом компьютере установлено ${systemRamGb} GB ОЗУ. Значение подобрано автоматически при первом запуске, но его можно изменить.`
                  : "Не удалось определить объём ОЗУ на этом компьютере — доступен ручной выбор."}
              </p>
            </div>
            <div className="card">
              <h2>Параметры сборки (только чтение)</h2>
              <p className="muted small">
                Ядро, версия ядра и версия Minecraft устанавливаются администратором
                через панель управления и одинаковы у всех игроков.
              </p>
              {manifest && (
                <div className="build-grid">
                  <div>
                    <span className="label">Ядро</span>
                    <span className="value">{manifest.loader}</span>
                  </div>
                  <div>
                    <span className="label">Версия ядра</span>
                    <span className="value">{manifest.loaderVersion}</span>
                  </div>
                  <div>
                    <span className="label">Minecraft</span>
                    <span className="value">{manifest.mcVersion}</span>
                  </div>
                </div>
              )}
            </div>
          </section>
        )}
      </main>
    </div>
  );
}
