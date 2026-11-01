'use strict';

const $ = selector => document.querySelector(selector);
const $$ = selector => [...document.querySelectorAll(selector)];
const escapeHTML = value => String(value ?? '').replace(/[&<>"']/g, ch => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[ch]));
const downIcon = '<svg class="icon" viewBox="0 0 24 24" width="20" height="20" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 3v12m-5-5 5 5 5-5M5 16v4h14v-4"/></svg>';
const params = new URLSearchParams(location.search);
const platforms = ['windows', 'linux', 'astra'];
let platform = platforms.includes(params.get('platform')) ? params.get('platform') : /Linux/i.test(navigator.userAgent) && !/Android/i.test(navigator.userAgent) ? 'linux' : 'windows';
let filter = platforms.includes(params.get('filter')) ? params.get('filter') : 'all';
let query = params.get('q') || '';
let visibleCount = 8;
let catalog = null;
let lastLoad = 0;
let loading = false;

const menu = $('.menu-toggle');
menu?.addEventListener('click', () => {
  const opened = menu.getAttribute('aria-expanded') !== 'true';
  menu.setAttribute('aria-expanded', String(opened));
  menu.setAttribute('aria-label', opened ? 'Закрыть меню' : 'Открыть меню');
  $('#main-nav').classList.toggle('open', opened);
});
document.addEventListener('keydown', event => {
  if (event.key === 'Escape' && menu?.getAttribute('aria-expanded') === 'true') {
    menu.click(); menu.focus();
  }
});

const dialog = $('#detail-dialog');
function showDialog(title, content, imageMode = false) {
  $('#dialog-title').textContent = title;
  $('#dialog-body').replaceChildren(content);
  dialog.classList.toggle('image-dialog', imageMode);
  dialog.showModal();
}
$('[data-close-dialog]')?.addEventListener('click', () => dialog.close());
dialog?.addEventListener('click', event => {
  if (event.target !== dialog) return;
  const rect = dialog.getBoundingClientRect();
  if (event.clientX < rect.left || event.clientX > rect.right || event.clientY < rect.top || event.clientY > rect.bottom) dialog.close();
});

const screenshots = {
  grpc: ['gRPC-вызов локального демонстрационного сервиса', 'Reqon: gRPC-запрос и ответ локального сервиса', 'grpc-current'],
  http: ['HTTP-запрос к локальному демонстрационному API', 'Reqon: HTTP-запрос с параметрами и JSON-ответ', 'http-current'],
  environments: ['Переменные локального окружения', 'Reqon: таблица переменных и выбор активного окружения', 'environments-compact'],
};
$$('[data-screenshot]').forEach(button => button.addEventListener('click', () => {
  const key = button.dataset.screenshot;
  selectTab(button);
  $('#screenshot-panel').setAttribute('aria-labelledby', button.id);
  $('#product-screenshot').src = `/assets/screenshots/${screenshots[key][2]}.png`;
  $('#product-screenshot').alt = screenshots[key][1];
  $('#screenshot-caption').textContent = screenshots[key][0];
}));
$('[data-zoom]')?.addEventListener('click', () => {
  const image = $('#product-screenshot').cloneNode();
  image.removeAttribute('id');
  showDialog('Интерфейс Reqon', image, true);
});
$$('[data-image-dialog]').forEach(button => button.addEventListener('click', () => {
  const image = button.querySelector('img').cloneNode();
  showDialog('Интерфейс Reqon', image, true);
}));

function selectTab(button) {
  button.closest('[role="tablist"]').querySelectorAll('[role="tab"]').forEach(tab => {
    const selected = button === tab;
    tab.setAttribute('aria-selected', String(selected));
    tab.tabIndex = selected ? 0 : -1;
  });
}
$$('[role="tablist"]').forEach(list => list.addEventListener('keydown', event => {
  const tabs = [...list.querySelectorAll('[role="tab"]')];
  let index = tabs.indexOf(document.activeElement);
  if (index === -1) return;
  if (event.key === 'ArrowRight') index = (index + 1) % tabs.length;
  else if (event.key === 'ArrowLeft') index = (index + tabs.length - 1) % tabs.length;
  else if (event.key === 'Home') index = 0;
  else if (event.key === 'End') index = tabs.length - 1;
  else return;
  event.preventDefault(); tabs[index].click(); tabs[index].focus();
}));

function formatSize(size) {
  return `${new Intl.NumberFormat('ru-RU', { maximumFractionDigits: 1 }).format(size / 1048576)} МБ`;
}
function formatDate(date) {
  return new Intl.DateTimeFormat('ru-RU', { day: 'numeric', month: 'short', year: 'numeric' }).format(new Date(date));
}
function updateQuery() {
  const next = new URL(location.href);
  if ($('#latest-release')) next.searchParams.set('platform', platform);
  if ($('#release-list')) {
    if (filter === 'all') next.searchParams.delete('filter'); else next.searchParams.set('filter', filter);
    if (query) next.searchParams.set('q', query); else next.searchParams.delete('q');
  }
  history.replaceState(null, '', next);
}

const platformNotes = {
  windows: 'Установщик для Windows x64. Python и необходимые библиотеки включены.',
  linux: 'Переносимый архив · x86-64 · glibc ≥ 2.36. Распакуйте в новую папку. Для 0.13.4 запуск на ALT Linux пока не подтверждён тестами.',
  astra: 'DEB-пакет для Astra Linux 1.8, amd64. Версия 0.13.2 содержит прежний Qt; при ошибке none_dealloc смотрите инструкцию для Linux.',
};
function renderLatest() {
  if (!$('#latest-release') || !catalog) return;
  const selectedTab = $(`[data-platform="${platform}"]`);
  selectTab(selectedTab);
  $('#latest-release').setAttribute('aria-labelledby', selectedTab.id);
  const release = catalog.latest[platform];
  if (!release) {
    $('#latest-release').innerHTML = '<div class="empty-state"><h3>Сборка пока недоступна</h3><p>Выберите другую платформу или откройте архив релизов.</p></div>';
    return;
  }
  const safeId = escapeHTML(release.id);
  $('#latest-release').innerHTML = `
    <div class="latest-top">Последняя стабильная версия</div>
    <div class="latest-copy">
      <div><h2>Reqon ${escapeHTML(release.version)}</h2>
        <div class="latest-meta"><span>${escapeHTML(release.platformLabel)}</span><span>·</span><span>${release.format}</span><span>·</span><span>${formatSize(release.size)}</span><span>·</span><time datetime="${escapeHTML(release.uploadedAt)}" title="Дата загрузки файла">${formatDate(release.uploadedAt)}</time></div>
      </div>
      <div class="actions"><a class="button button-primary" href="${escapeHTML(release.url)}" data-download="${safeId}">Скачать ${release.format}${downIcon}</a><button class="button button-secondary" data-notes="${safeId}">Что нового</button><button class="note-button" data-checksum="${safeId}">SHA-256</button></div>
      <p class="platform-note">${escapeHTML(platformNotes[platform])} ${release.source === 'github' ? 'Файл загружается напрямую с GitHub.' : ''}</p>
    </div>`;
}

function renderArchive() {
  if (!$('#release-list') || !catalog) return;
  const normalized = query.trim().toLowerCase().replace(/^v(?=\d)/, '');
  const matches = catalog.releases.filter(r => (filter === 'all' || filter === r.platform)
    && `${r.version} ${r.platformLabel} ${r.format}`.toLowerCase().includes(normalized));
  const plural = new Intl.PluralRules('ru-RU').select(matches.length);
  $('#release-count').textContent = `${matches.length} ${{ one: 'сборка', few: 'сборки', many: 'сборок', other: 'сборки' }[plural]}`;
  $$('[data-filter]').forEach(button => {
    const selected = button.dataset.filter === filter;
    button.classList.toggle('selected', selected);
    button.setAttribute('aria-pressed', String(selected));
  });
  $('#load-more').hidden = matches.length <= visibleCount;
  if (!matches.length) {
    $('#release-list').innerHTML = `<div class="empty-state"><h3>${catalog.releases.length ? 'Версия не найдена' : 'Релизы ещё не опубликованы'}</h3><p>${catalog.releases.length ? 'Укажите другой номер или сбросьте фильтры.' : 'Список появится после публикации первой сборки.'}</p>${catalog.releases.length ? '<button class="button button-secondary" data-reset-filters>Сбросить фильтры</button>' : ''}</div>`;
    return;
  }
  $('#release-list').innerHTML = matches.slice(0, visibleCount).map(release => {
    const id = escapeHTML(release.id);
    const latest = catalog.latest[release.platform]?.id === release.id;
    return `<article class="release-row"><div class="release-version">v${escapeHTML(release.version)}${latest ? '<small>● Последняя стабильная</small>' : ''}</div><div class="release-platform">${escapeHTML(release.platformLabel)} <small><time datetime="${escapeHTML(release.uploadedAt)}">${formatDate(release.uploadedAt)}</time> · ${release.format}</small></div><span class="release-size">${formatSize(release.size)}</span><div class="release-actions"><button class="note-button" data-notes="${id}" aria-label="Изменения в версии ${release.version} для ${release.platformLabel}">Изменения</button><button class="note-button" data-checksum="${id}" aria-label="SHA-256 версии ${release.version} для ${release.platformLabel}">SHA-256</button><a class="button button-secondary" href="${escapeHTML(release.url)}" data-download="${id}" aria-label="Скачать Reqon ${release.version} для ${release.platformLabel}">Скачать ${downIcon}</a></div></article>`;
  }).join('');
}

$$('[data-platform]').forEach(button => button.addEventListener('click', () => {
  platform = button.dataset.platform; selectTab(button); renderLatest(); updateQuery();
}));
$$('[data-filter]').forEach(button => button.addEventListener('click', () => {
  filter = button.dataset.filter; visibleCount = 8; renderArchive(); updateQuery();
}));
if ($('#release-search')) {
  $('#release-search').value = query;
  $('#release-search').addEventListener('input', event => {
    query = event.target.value; visibleCount = 8; renderArchive(); updateQuery();
  });
}
$('#load-more')?.addEventListener('click', () => { visibleCount += 8; renderArchive(); });

let toastTimer;
function toast(message) {
  clearTimeout(toastTimer); $('#toast').textContent = message; $('#toast').hidden = false;
  toastTimer = setTimeout(() => { $('#toast').hidden = true; }, 4500);
}

document.addEventListener('click', async event => {
  const target = event.target.closest('[data-notes], [data-checksum], [data-retry], [data-reset-filters]');
  if (!target) return;
  if (target.hasAttribute('data-retry')) return loadReleases();
  if (target.hasAttribute('data-reset-filters')) {
    query = ''; filter = 'all'; visibleCount = 8; $('#release-search').value = ''; renderArchive(); updateQuery(); return;
  }
  const release = catalog?.releases.find(r => r.id === (target.dataset.notes || target.dataset.checksum));
  if (!release) return;
  if (target.hasAttribute('data-notes')) {
    const content = document.createElement('pre');
    content.className = 'notes-text';
    content.textContent = release.notes || 'Для этой версии отдельное описание изменений не опубликовано.';
    showDialog(`Reqon ${release.version} · ${release.platformLabel}`, content);
    return;
  }
  const content = document.createElement('div');
  const file = document.createElement('p'); file.className = 'dialog-file'; file.textContent = release.filename; content.append(file);
  if (release.sha256) {
    const checksum = document.createElement('code'); checksum.className = 'checksum-value'; checksum.textContent = release.sha256;
    const copy = document.createElement('button'); copy.className = 'button button-primary'; copy.textContent = 'Скопировать SHA-256';
    copy.addEventListener('click', async () => {
      try { await navigator.clipboard.writeText(release.sha256); toast('SHA-256 скопирован'); }
      catch { const range = document.createRange(); range.selectNodeContents(checksum); const selection = getSelection(); selection.removeAllRanges(); selection.addRange(range); toast('Сумма выделена. Нажмите Ctrl+C для копирования.'); }
    });
    content.append(checksum, copy);
  } else {
    const note = document.createElement('p'); note.className = 'dialog-description';
    note.textContent = 'Для этого файла GitHub не предоставил SHA-256. Контрольная сумма пока недоступна.'; content.append(note);
  }
  showDialog('Проверка файла', content);
});

async function loadReleases() {
  if (loading) return;
  loading = true;
  try {
    const response = await fetch('/api/releases', { cache: 'no-store', signal: AbortSignal.timeout(30000) });
    if (!response.ok) throw new Error('Release request failed');
    const next = await response.json();
    if (!Array.isArray(next.releases) || !next.latest) throw new Error('Invalid release catalog');
    catalog = next; lastLoad = Date.now();
    renderLatest(); renderArchive();
    const latest = catalog.latest.windows;
    if ($('[data-latest-badge]')) $('[data-latest-badge]').textContent = latest ? `Reqon ${latest.version} — стабильная версия` : 'Версии для Windows и Linux';
    if ($('[data-latest-meta]')) $('[data-latest-meta]').textContent = latest ? `v${latest.version} · Windows x64 · ${formatSize(latest.size)}` : 'Windows x64';
    if ($('#release-status')) $('#release-status').textContent = catalog.githubStale ? 'Не удалось обновить список с GitHub. Доступны сохранённые релизы и файлы на сервере; проверка повторится автоматически.' : '';
  } catch {
    const message = '<div class="empty-state"><h3>Не удалось загрузить релизы</h3><p>Проверьте соединение и попробуйте ещё раз.</p><button class="button button-secondary" data-retry>Повторить</button></div>';
    if (!catalog) {
      if ($('#latest-release')) $('#latest-release').innerHTML = message;
      if ($('#release-list')) $('#release-list').innerHTML = message;
    } else if ($('#release-status')) $('#release-status').textContent = 'Не удалось обновить список. Показаны ранее загруженные версии.';
  } finally { loading = false; }
}

if ($('#release-list') || $('[data-latest-meta]')) {
  loadReleases();
  setInterval(() => { if (!document.hidden) loadReleases(); }, 300000);
  document.addEventListener('visibilitychange', () => { if (!document.hidden && Date.now() - lastLoad > 300000) loadReleases(); });
}
