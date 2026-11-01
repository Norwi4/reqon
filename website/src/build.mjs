import { mkdir, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { header, footer, dialogs } from './components.mjs';
import { home, features, downloads, changelog, about, notFound } from './pages.mjs';
import { docs } from './docs.mjs';

const publicDir = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../public');
const origin = process.env.SITE_URL ? new URL(process.env.SITE_URL).origin : '';
const pages = [
  ['/', 'Reqon — API-клиент для HTTP и gRPC', 'Настольный API-клиент для HTTP и gRPC. Отправка запросов, коллекции и окружения. Скачать Reqon для Windows и Linux.', home, 'home'],
  ['/features/', 'Возможности — Reqon', 'HTTP, gRPC, коллекции и окружения в настольном API-клиенте Reqon.', features, 'features'],
  ['/download/', 'Скачать Reqon — Windows и Linux', 'Последние стабильные версии Reqon, архив релизов и контрольные суммы SHA-256.', downloads, 'download'],
  ['/docs/', 'Документация — Reqon', 'Установка Reqon, первый HTTP-запрос, gRPC, коллекции и окружения.', docs, 'docs'],
  ['/changelog/', 'История релизов — Reqon', 'Изменения в версиях Reqon и загрузка предыдущих релизов.', changelog, 'changelog'],
  ['/about/', 'О проекте — Reqon', 'Reqon — настольный API-клиент с локальным хранением данных и нативным интерфейсом.', about, 'about'],
  ['/404.html', 'Страница не найдена — Reqon', 'Запрошенная страница не найдена.', notFound, '404'],
];

for (const [route, title, description, content, page] of pages) {
  const html = `<!doctype html>
<html lang="ru">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>${title}</title>
  <meta name="description" content="${description}">
  <meta name="theme-color" content="#ffffff">
  <meta property="og:type" content="website">
  <meta property="og:locale" content="ru_RU">
  <meta property="og:site_name" content="Reqon">
  <meta property="og:title" content="${title}">
  <meta property="og:description" content="${description}">
  <meta property="og:image" content="${origin}/assets/screenshots/http-current.png">
  ${origin ? `<link rel="canonical" href="${origin}${route}"><meta property="og:url" content="${origin}${route}">` : ''}
  <link rel="icon" type="image/x-icon" href="/assets/reqon-approved.ico">
  <link rel="apple-touch-icon" href="/assets/reqon-approved-apple-touch-icon.png">
  <link rel="preload" href="/assets/fonts/Inter-Regular.ttf" as="font" type="font/ttf" crossorigin>
  <link rel="stylesheet" href="/styles.css">
  <script src="/app.js" defer></script>
</head>
<body data-page="${page}">
  <a class="skip-link" href="#main">Перейти к содержимому</a>
  ${header(route)}
  <main id="main">${content}</main>
  ${footer}
  ${dialogs}
  <noscript><div class="noscript-note">Для фильтрации релизов включите JavaScript. <a href="/download/latest/windows">Скачать последнюю Windows-версию</a> · <a href="/download/latest/linux">Linux</a> · <a href="/download/latest/astra">Astra Linux</a></div></noscript>
</body>
</html>`;
  const file = path.join(publicDir, route === '/404.html' ? '404.html' : `${route.replace(/^\//, '')}index.html`);
  await mkdir(path.dirname(file), { recursive: true });
  await writeFile(file, html, 'utf8');
}
console.log(`Built ${pages.length} pages in ${publicDir}`);
