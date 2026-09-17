// Layout study only: no imports from application stores or service actions.
export function setupMeadowPanels(canvas, signal) {
  const wrap = canvas.parentElement;
  const stage = document.createElement('div'); stage.className = 'meadow-stage';
  wrap.append(stage); stage.append(canvas, wrap.querySelector('#loading'));
  const toolbar = document.createElement('div'); toolbar.className = 'panel-preview-tools';
  toolbar.innerHTML = `<button id="panel-preview-toggle" type="button" aria-pressed="true">Панели включены</button>
    <label>Окно <select id="panel-preview-size"><option value="1000">1000 × 680</option><option value="800">800 × 600</option></select></label>
    <label>Плотность панелей <input id="panel-preview-opacity" type="range" min="35" max="90" value="68" step="1"><output id="panel-opacity-value">68%</output></label>
    <span>Макет интерфейса · данные для примера</span>`;
  document.querySelector('.scene-grid').before(toolbar);
  const shell = document.createElement('section'); shell.className = 'meadow-app-preview'; shell.setAttribute('aria-label', 'Макет окна Obsession');
  shell.innerHTML = `<div class="preview-titlebar"><span>◉ &nbsp; Obsession</span><span aria-hidden="true">— &nbsp; □ &nbsp; ×</span></div>
    <nav class="preview-nav" aria-label="Разделы макета"><div class="preview-brand">◉ <strong>Obsession<small>ПРЕДПРОСМОТР</small></strong></div>
      <small class="preview-menu-label">МЕНЮ</small>
      <button type="button" data-preview-screen="overview" aria-pressed="true">◈ &nbsp; Обзор</button>
      <span>ϟ &nbsp; DPI-обход</span><span>◇ &nbsp; ИИ-разблокировка</span><span>➤ &nbsp; Telegram</span><span>☷ &nbsp; Списки</span><span>▱ &nbsp; Профили</span>
      <button type="button" data-preview-screen="settings" aria-pressed="false">⚙ &nbsp; Настройки</button>
      <div class="preview-nav-foot">Золотое поле<small>Примерка фона</small></div>
    </nav>
    <div class="preview-main">
      <section data-preview-page="overview"><div class="preview-heading"><h2>Обзор</h2><p>Состояние защиты одним взглядом</p></div>
        <div class="preview-card preview-hero"><span class="preview-eye">◉</span><div><h3>Под защитой</h3><p>Обход активен · Discord, YouTube</p></div><span class="preview-action">Выключить</span></div>
        <div class="preview-card-grid">
          <div class="preview-card"><small>DPI-ОБХОД</small><h3>Активен</h3><p>Discord · YouTube / Twitch</p><div class="preview-card-bottom">Текущая стратегия <b>Авто</b></div></div>
          <div class="preview-card"><small>ПРОКСИ</small><h3>Выключен</h3><p>Локальный SOCKS5 / HTTP</p><div class="preview-card-bottom">Порт <b>1080</b></div></div>
          <div class="preview-card"><small>HOSTS</small><h3>Актуально</h3><p>Правила разблокировки</p><div class="preview-card-bottom">Последняя проверка <b>Сегодня</b></div></div>
          <div class="preview-card"><small>ИИ-СЕРВИСЫ</small><h3>Готово</h3><p>Доступ к выбранным сервисам</p><div class="preview-card-bottom">Состояние <b>Настроено</b></div></div>
        </div>
        <div class="preview-card preview-network"><small>ПОДКЛЮЧЕНИЕ</small><h3>Сеть доступна</h3><p>Состояния и значения показаны для примерки панелей.</p></div>
      </section>
      <section data-preview-page="settings" hidden><div class="preview-heading"><h2>Настройки</h2><p>Параметры приложения · сохраняются автоматически</p></div>
        <div class="preview-card-grid preview-settings-grid">
          <div class="preview-column"><div class="preview-card"><small>ОФОРМЛЕНИЕ</small><div class="preview-theme-grid"><span>Золотое поле</span><span>Ночь</span><span>Дождь</span><span>Тихий пруд</span></div></div>
            <div class="preview-card"><small>ОБЩИЕ</small><div class="preview-setting">Автозапуск с Windows <i class="preview-switch on"></i></div><div class="preview-setting">Запускать свёрнутым <i class="preview-switch"></i></div><div class="preview-setting">Скрывать в трей <i class="preview-switch on"></i></div></div>
            <div class="preview-card"><small>ДВИЖЕНИЕ</small><div class="preview-setting">Уменьшить анимацию <i class="preview-switch"></i></div></div></div>
          <div class="preview-column"><div class="preview-card"><small>ЗАЩИЩЁННАЯ СЛУЖБА</small><h3>Служба установлена</h3><p>Управление системными функциями</p><div class="preview-setting">Состояние <b>Готово</b></div></div>
            <div class="preview-card"><small>ПОДКЛЮЧЕНИЕ</small><div class="preview-setting">Порт прокси <b>1080</b></div><div class="preview-setting">Автоподключение <i class="preview-switch on"></i></div><div class="preview-setting">Проверка сети <b>Авто</b></div></div>
            <div class="preview-card"><small>О ПРИЛОЖЕНИИ</small><h3>Obsession</h3><p>Макет для проверки читаемости</p></div></div>
        </div>
      </section>
    </div>`;
  stage.append(shell);
  const toggle = toolbar.querySelector('#panel-preview-toggle');
  const size = toolbar.querySelector('#panel-preview-size');
  let enabled = true;
  const resize = () => {
    const width = Number(size.value), height = width === 1000 ? 680 : 600;
    wrap.classList.toggle('with-panels', enabled); shell.hidden = !enabled;
    wrap.style.aspectRatio = enabled ? `${width} / ${height}` : '735 / 505';
    if (enabled) {
      const scale = wrap.clientWidth / width;
      stage.style.cssText = `width:${width}px;height:${height}px;transform:scale(${scale});transform-origin:0 0`;
      // Cover without distorting the original drawing. Actual canvas bounds
      // remain available to the existing petting coordinate conversion.
      const sceneScale = Math.max(width / 735, height / 505);
      canvas.style.cssText = `position:absolute;width:${735 * sceneScale}px;height:${505 * sceneScale}px;left:${(width - 735 * sceneScale) / 2}px;top:${(height - 505 * sceneScale) / 2}px`;
    } else { stage.style.cssText = ''; canvas.style.cssText = ''; }
  };
  const on = (el, event, fn) => el.addEventListener(event, fn, { signal });
  on(toggle, 'click', () => { enabled = !enabled; toggle.setAttribute('aria-pressed', String(enabled)); toggle.textContent = enabled ? 'Панели включены' : 'Показать панели'; resize(); });
  on(size, 'change', resize);
  on(toolbar.querySelector('#panel-preview-opacity'), 'input', event => {
    shell.style.setProperty('--preview-density', String(Number(event.target.value) / 100));
    toolbar.querySelector('#panel-opacity-value').textContent = `${event.target.value}%`;
  });
  for (const button of shell.querySelectorAll('[data-preview-screen]')) on(button, 'click', () => {
    for (const tab of shell.querySelectorAll('[data-preview-screen]')) tab.setAttribute('aria-pressed', String(tab === button));
    for (const page of shell.querySelectorAll('[data-preview-page]')) page.hidden = page.dataset.previewPage !== button.dataset.previewScreen;
    shell.querySelector('.preview-main').scrollTop = 0;
  });
  const observer = new ResizeObserver(resize); observer.observe(wrap); resize();
  signal.addEventListener('abort', () => { observer.disconnect(); toolbar.remove(); shell.remove(); wrap.append(canvas, stage.querySelector('#loading')); stage.remove(); wrap.classList.remove('with-panels'); wrap.style.aspectRatio = ''; canvas.style.cssText = ''; }, { once: true });
}
