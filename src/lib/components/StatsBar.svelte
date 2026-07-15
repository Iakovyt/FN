<script lang="ts">
  export let activeModules = 0;
  export let trafficBytesPerSec = 0;
  export let uptimeSecs = 0;

  function fmtTraffic(bps: number): string {
    if (bps <= 0) return "0 б/с";
    const units = ["б/с", "кб/с", "Мб/с", "Гб/с"];
    let v = bps;
    let i = 0;
    while (v >= 1024 && i < units.length - 1) {
      v /= 1024;
      i++;
    }
    return `${v >= 100 || i === 0 ? Math.round(v) : v.toFixed(1)} ${units[i]}`;
  }

  function fmtUptime(secs: number): string {
    const h = Math.floor(secs / 3600);
    const m = Math.floor((secs % 3600) / 60);
    if (h > 0) return `${h}ч ${m}м`;
    const s = Math.floor(secs % 60);
    return `${m}м ${s}с`;
  }

  function pluralModules(n: number): string {
    const mod10 = n % 10;
    const mod100 = n % 100;
    if (mod10 === 1 && mod100 !== 11) return "активный модуль";
    if (mod10 >= 2 && mod10 <= 4 && (mod100 < 10 || mod100 >= 20)) return "активных модуля";
    return "активных модулей";
  }
</script>

<div class="stats">
  <p>
    <span class="val">{activeModules}</span><br />
    <span class="lbl">{pluralModules(activeModules)}</span>
  </p>
  <p>
    <span class="val">{fmtTraffic(trafficBytesPerSec)}</span><br />
    <span class="lbl">трафик</span>
  </p>
  <p>
    <span class="val">{fmtUptime(uptimeSecs)}</span><br />
    <span class="lbl">аптайм</span>
  </p>
</div>

<style>
  .stats {
    position: relative;
    background: var(--card-stats);
    backdrop-filter: blur(10px);
    border: 0.5px solid var(--border-soft);
    border-radius: 12px;
    padding: 12px 14px;
    display: flex;
    justify-content: space-around;
    text-align: center;
  }
  .stats p {
    margin: 0;
  }
  .val {
    color: var(--text);
    font-size: 14px;
    font-weight: 600;
  }
  .lbl {
    color: var(--text-muted);
    font-size: 10px;
    margin-top: 2px;
  }
</style>
