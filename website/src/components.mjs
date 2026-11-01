const paths = {
  arrow: '<path d="M5 12h14m-5-5 5 5-5 5"/>',
  external: '<path d="M7 17 17 7M7 7h10v10"/>',
  down: '<path d="M12 3v12m-5-5 5 5 5-5M5 16v4h14v-4"/>',
  code: '<path d="m8 7-5 5 5 5m8-10 5 5-5 5m-3-14-2 18"/>',
  folder: '<path d="M3 7V5h6l2 2h10v13H3Z"/>',
  lock: '<rect x="5" y="10" width="14" height="11" rx="2"/><path d="M8 10V7a4 4 0 0 1 8 0v3m-4 5v2"/>',
  terminal: '<rect x="3" y="4" width="18" height="16" rx="2"/><path d="m7 8 4 4-4 4m7 0h3"/>',
  search: '<circle cx="10.5" cy="10.5" r="6.5"/><path d="m16 16 5 5"/>',
  close: '<path d="m6 6 12 12M6 18 18 6"/>',
  menu: '<path d="M4 6h16M4 12h16M4 18h16"/>',
  github: '<path d="M9 19c-4 1-4-2-6-2m12 5v-4c0-1-.3-2-1-2 4-.4 7-2 7-6a5 5 0 0 0-1.5-3.5A5 5 0 0 0 19 3s-1 0-3 1a13 13 0 0 0-8 0C6 3 5 3 5 3a5 5 0 0 0-.5 3.5A5 5 0 0 0 3 10c0 4 3 5.6 7 6-.7.5-1 1-1 2v4"/>',
  windows: '<path d="M3 5 10 4v7H3Zm10-1 8-1v8h-8ZM3 14h7v7l-7-1Zm10 0h8v9l-8-1Z"/>',
};

export const icon = name => `<svg class="icon" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${paths[name] || paths.code}</svg>`;
export const brand = '<img src="/assets/reqon-approved-96.png" width="32" height="32" alt=""><span>Reqon</span>';
export const button = (href, text, primary = true, symbol = '') => `<a class="button ${primary ? 'button-primary' : 'button-secondary'}" href="${href}">${text}${symbol ? icon(symbol) : ''}</a>`;
export const textLink = (href, text) => `<a class="text-link" href="${href}">${text}${icon('arrow')}</a>`;

export function header(route) {
  const links = [['/', 'API-клиент'], ['/features/', 'Возможности'], ['/docs/', 'Документация'], ['/changelog/', 'Релизы']];
  return `<header class="site-header"><div class="header-inner">
    <a class="brand" href="/" aria-label="Reqon — главная">${brand}</a>
    <nav id="main-nav" aria-label="Основная навигация">${links.map(([href, label]) => `<a href="${href}"${route === href ? ' aria-current="page"' : ''}>${label}</a>`).join('')}</nav>
    <a class="header-github" href="https://github.com/Norwi4/reqon" target="_blank" rel="noopener noreferrer">${icon('github')}<span>GitHub</span></a>
    <a class="button button-primary header-download" href="/download/">Скачать Reqon</a>
    <button class="menu-toggle" aria-label="Открыть меню" aria-expanded="false" aria-controls="main-nav">${icon('menu')}</button>
  </div></header>`;
}

export const footer = `<footer class="site-footer">
  <div class="container footer-main">
    <div class="footer-product"><a class="brand" href="/" aria-label="Reqon — главная">${brand}</a><p>Настольный API-клиент<br>для Windows и Linux.</p></div>
    <div><h2>Продукт</h2><a href="/features/">Возможности</a><a href="/download/">Скачать Reqon</a><a href="/about/">О проекте</a></div>
    <div><h2>Ресурсы</h2><a href="/docs/">Документация</a><a href="/changelog/">История изменений</a><a href="/download/#archive">Предыдущие версии</a></div>
    <div><h2>Разработка</h2><a href="https://github.com/Norwi4/reqon" target="_blank" rel="noopener noreferrer">Проект на GitHub ${icon('external')}</a><a href="https://github.com/Norwi4/reqon/issues" target="_blank" rel="noopener noreferrer">Сообщить об ошибке ${icon('external')}</a></div>
  </div><div class="container footer-bottom">© ${new Date().getFullYear()} Reqon</div>
</footer>`;

export const cta = `<section class="download-banner"><div class="container">
  <div><h2>Скачать Reqon</h2><p>Приложение для Windows и Linux. Регистрация не требуется.</p></div>
  ${button('/download/', 'Выбрать версию', true, 'down')}
</div></section>`;

export const dialogs = `<dialog id="detail-dialog" aria-labelledby="dialog-title">
  <div class="dialog-heading"><h2 id="dialog-title"></h2><button class="icon-button" data-close-dialog aria-label="Закрыть">${icon('close')}</button></div>
  <div id="dialog-body"></div>
</dialog><p id="toast" class="toast" role="status" hidden></p>`;
