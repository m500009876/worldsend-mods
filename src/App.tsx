import { useEffect, useState, useCallback } from "react";
import {
  api,
  Manifest,
  NewsItem,
  ServerStatus,
  LaunchSettings,
  LaunchProgress,
} from "./api";

type Tab = "home" | "news" | "settings";

export default function App() {
  const [tab, setTab] = useState<Tab>("home");
  const [manifest, setManifest] = useState<Manifest | null>(null);
  const [news, setNews] = useState<NewsItem[]>([]);
  const [status, setStatus] = useState<ServerStatus | null>(null);
  const [settings, setSettings] = useState<LaunchSettings>({ nickname: "Player", ramGb: 6 });
  const [javaOk, setJavaOk] = useState<boolean | null>(null);

  const [launching, setLaunching] = useState(false);
  const [progressLabel, setProgressLabel] = useState("");
  const [progressPct, setProgressPct] = useState(0);
  const [error, setError] = useState("");

  const loadAll = useCallback(async () => {
    try {
      const [m, n, s, st, j] = await Promise.all([
        api.getManifest(),
        api.getNews().catch(() => []),
        api.getServerStatus().catch(() => ({ online: false })),
        api.getSettings(),
        api.checkJava(),
      ]);
      setManifest(m);
      setNews(n);
      setStatus(s);
      setSettings(st);
      setJavaOk(j);
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
          setProgressLabel("Minecraft запущен!");
          setProgressPct(100);
          setTimeout(() => setLaunching(false), 1500);
          break;
        case "Error":
          setError(p.data.message);
          setLaunching(false);
          break;
      }
    });
    return () => {
      unlisten.then((f) => f());
    };
  }, [loadAll]);

  const handleSaveSettings = async (next: LaunchSettings) => {
    setSettings(next);
    await api.saveSettings(next);
  };

  const handlePlay = async () => {
    setError("");
    setLaunching(true);
    setProgressPct(0);
    setProgressLabel("Запуск...");
    try {
      await api.startLaunch(settings);
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
          <button className={tab === "news" ? "active" : ""} onClick={() => setTab("news")}>
            Новости
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
              {launching && (
                <div className="progress">
                  <div className="progress-bar">
                    <div className="progress-fill" style={{ width: `${progressPct}%` }} />
                  </div>
                  <p className="progress-label">{progressLabel}</p>
                </div>
              )}
              <button className="play-button" onClick={handlePlay} disabled={launching || !manifest}>
                {launching ? "Идёт запуск..." : "ИГРАТЬ"}
              </button>
              <p className="muted small">
                {settings.nickname} · {settings.ramGb} GB RAM
              </p>
            </div>
          </section>
        )}

        {tab === "news" && (
          <section className="news">
            {news.length === 0 && <p className="muted">Новостей пока нет.</p>}
            {news.map((n) => (
              <div className="card" key={n.id}>
                <div className="news-head">
                  <h3>{n.title}</h3>
                  <span className="tag">{n.tag}</span>
                </div>
                <p>{n.content}</p>
              </div>
            ))}
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
                max={16}
                step={1}
                value={settings.ramGb}
                onChange={(e) =>
                  handleSaveSettings({ ...settings, ramGb: Number(e.target.value) })
                }
              />
              <div className="range-labels">
                <span>2</span>
                <span>16</span>
              </div>
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
