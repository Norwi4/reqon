import { button, icon, textLink, cta } from './components.mjs';

const screenshot = (file, alt) => `<figure class="feature-screenshot"><button class="screenshot-button" data-image-dialog aria-label="Увеличить скриншот"><img src="/assets/screenshots/${file}.png" alt="${alt}" loading="lazy"></button></figure>`;

const preview = `<div class="container product-preview">
  <figure id="screenshot-panel" role="tabpanel" aria-labelledby="tab-http">
    <button class="screenshot-button" data-zoom aria-label="Открыть скриншот приложения в полном размере">
      <img id="product-screenshot" src="/assets/screenshots/http-current.png" width="1370" height="860" alt="Reqon: HTTP-запрос к локальному демонстрационному API и JSON-ответ" fetchpriority="high">
    </button>
    <figcaption id="screenshot-caption">HTTP-запрос к локальному демонстрационному API</figcaption>
  </figure>
  <div class="tabs preview-tabs" role="tablist" aria-label="Интерфейс приложения">
    <button id="tab-http" role="tab" aria-selected="true" aria-controls="screenshot-panel" data-screenshot="http">HTTP-запросы</button>
    <button id="tab-grpc" role="tab" aria-selected="false" aria-controls="screenshot-panel" tabindex="-1" data-screenshot="grpc">gRPC</button>
    <button id="tab-environments" role="tab" aria-selected="false" aria-controls="screenshot-panel" tabindex="-1" data-screenshot="environments">Окружения</button>
  </div>
</div>`;

export const home = `
<section class="product-hero">
  <div class="container hero-intro">
    <p class="product-label">Reqon API Client</p>
    <h1>Работа с API на вашем компьютере</h1>
    <p class="hero-description">Отправляйте HTTP- и gRPC-запросы, изучайте ответы<br class="desktop-break"> и сохраняйте рабочие сценарии в коллекциях.</p>
    <div class="actions">
      ${button('/download/latest/windows', 'Скачать для Windows', true, 'down')}
      ${button('/docs/', 'Документация', false)}
    </div>
    <p class="hero-meta"><span data-latest-meta>Windows x64</span><span aria-hidden="true">·</span><a href="/download/?platform=linux">Скачать для Linux</a></p>
  </div>
  ${preview}
</section>

<section class="container overview-section">
  <h2>Подготовка запросов, проверка ответов<br> и организация API в одном приложении</h2>
  <div class="feature-columns">
    <article>${icon('code')}<h3>HTTP и gRPC</h3><p>Работайте с REST API и gRPC-сервисами. Настраивайте параметры, заголовки и тело запроса, просматривайте статус и содержимое ответа.</p></article>
    <article>${icon('lock')}<h3>Авторизация и соединение</h3><p>Используйте Bearer, Basic и API Key для HTTP. Настраивайте TLS, клиентские сертификаты, metadata и параметры Proxy.</p></article>
    <article>${icon('folder')}<h3>Коллекции и окружения</h3><p>Группируйте запросы по проектам. Храните адреса и переменные в окружениях, переключайте их без изменения каждого запроса.</p></article>
  </div>
</section>

<section class="section-tint">
  <div class="container feature-detail" id="grpc">
    <div><h2>Исследуйте gRPC-сервисы</h2><p>Reqon получает список сервисов и методов через reflection. Если reflection недоступен, загрузите файл .proto или descriptor set.</p><p>Отправляйте JSON-сообщения, проверяйте metadata и trailers. Поддерживаются unary и streaming RPC.</p>${textLink('/features/#grpc', 'Подробнее о gRPC')}</div>
    ${screenshot('grpc-current', 'Reqon: выбор gRPC-метода, сообщение и ответ локального сервиса')}
  </div>
</section>

<section class="container feature-detail reverse" id="collections">
  <div><h2>Сохраняйте запросы<br> в контексте проекта</h2><p>Коллекции, папки и история помогают возвращаться к уже выполненным запросам. Переменные окружения позволяют использовать один запрос с разными адресами и параметрами.</p><p>Данные хранятся на устройстве. Для переноса на другой компьютер используйте импорт и экспорт коллекций.</p>${textLink('/docs/#collections', 'Работа с коллекциями и окружениями')}</div>
  ${screenshot('environments-compact', 'Reqon: переменные локального окружения')}
</section>

<section class="resources-section"><div class="container">
  <h2>Документация и загрузки</h2>
  <div class="resource-links">
    <a href="/docs/#installation"><h3>Установка и первый запуск</h3><p>Требования к системе и инструкция для каждой платформы.</p><span>Открыть инструкцию ${icon('arrow')}</span></a>
    <a href="/docs/#first-request"><h3>Первый запрос</h3><p>Как отправить HTTP-запрос или подключиться к gRPC-сервису.</p><span>Перейти к документации ${icon('arrow')}</span></a>
    <a href="/download/#archive"><h3>Архив релизов</h3><p>Предыдущие версии, описания изменений и контрольные суммы файлов.</p><span>Все версии ${icon('arrow')}</span></a>
  </div>
</div></section>
${cta}`;

export const features = `
<section class="container page-intro"><p class="breadcrumb"><a href="/">Reqon</a> / Возможности</p><h1>Инструменты для работы с API</h1><p>HTTP, gRPC, коллекции и окружения в нативном настольном приложении.</p></section>
<section id="http" class="container feature-detail feature-first">
  <div><h2>HTTP-запросы</h2><p>Задайте метод и URL, добавьте параметры и отправьте запрос. Ответ, его статус и заголовки отображаются в том же окне.</p>
    <ul class="feature-list"><li>Query-параметры, заголовки, JSON, текст и формы</li><li>Bearer, Basic и API Key</li><li>Импорт и экспорт cURL</li><li>HTTP/2, cookies и Proxy в Windows 0.13.8</li></ul>
    ${textLink('/docs/#first-request', 'Как отправить первый запрос')}
  </div>${screenshot('http-current', 'HTTP-запрос и JSON-ответ в Reqon')}
</section>
<section class="section-tint"><div id="grpc" class="container feature-detail reverse">
  <div><h2>gRPC-сервисы</h2><p>Находите методы через server reflection или импортируйте контракт сервиса. Редактируйте сообщение в JSON и просматривайте результат вызова.</p>
    <ul class="feature-list"><li>Файлы .proto и descriptor set</li><li>Unary, server и client streaming</li><li>Metadata и trailers</li><li>TLS и клиентские сертификаты</li><li>Импорт и экспорт gRPCurl</li></ul>
    ${textLink('/docs/#grpc', 'Документация по gRPC')}
  </div>${screenshot('grpc-current', 'Вызов gRPC-метода в Reqon')}
</div></section>
<section id="collections" class="container feature-detail">
  <div><h2>Коллекции и окружения</h2><p>Храните связанные запросы вместе, создавайте вложенные папки и переключайте переменные между окружениями.</p>
    <ul class="feature-list"><li>Коллекции, папки и история запросов</li><li>Переменные вида {{base_url}}</li><li>Автосохранение и ручное сохранение</li><li>Импорт HTTP-коллекций Postman v2.1</li><li>Формат Reqon для смешанных HTTP/gRPC-коллекций</li></ul>
  </div>${screenshot('environments-compact', 'Настройки переменных окружения в Reqon')}
</section>
${cta}`;

export const archive = `<section class="container archive" id="archive">
  <div class="archive-heading"><div><h2>Все версии</h2><p>Скачать предыдущий выпуск или посмотреть изменения.</p></div>
    <label class="search-field">${icon('search')}<input id="release-search" type="search" placeholder="Найти версию" aria-label="Поиск по версии" autocomplete="off"></label>
  </div>
  <div class="archive-toolbar"><div class="filter-group" role="group" aria-label="Фильтр платформы">
    <button class="selected" aria-pressed="true" data-filter="all">Все платформы</button><button aria-pressed="false" data-filter="windows">Windows</button><button aria-pressed="false" data-filter="linux">Linux</button><button aria-pressed="false" data-filter="astra">Astra Linux</button>
  </div><span id="release-count" class="release-count" aria-live="polite"></span></div>
  <div class="release-table-heading" aria-hidden="true"><span>Версия</span><span>Платформа и дата</span><span>Размер</span><span>Файлы</span></div>
  <div id="release-list" class="release-list" aria-live="polite"><p class="loading">Загружаем список релизов…</p></div>
  <button id="load-more" class="button button-secondary load-more" hidden>Показать ещё</button>
  <p class="archive-footnote">Для каждой платформы последней считается стабильная сборка с наибольшим номером версии. При одинаковом номере выбирается последний загруженный файл.</p>
</section>`;

export const downloads = `
<section class="container page-intro"><h1>Скачать Reqon</h1><p>Последние стабильные сборки для Windows и Linux.</p></section>
<section class="container download-section" aria-label="Последняя стабильная версия">
  <div class="platform-tabs tabs" role="tablist" aria-label="Платформа для скачивания">
    <button id="platform-windows" role="tab" aria-selected="true" aria-controls="latest-release" data-platform="windows">${icon('windows')} Windows</button>
    <button id="platform-linux" role="tab" aria-selected="false" aria-controls="latest-release" tabindex="-1" data-platform="linux">${icon('terminal')} Linux</button>
    <button id="platform-astra" role="tab" aria-selected="false" aria-controls="latest-release" tabindex="-1" data-platform="astra">${icon('terminal')} Astra Linux</button>
  </div>
  <div id="latest-release" class="latest-release" role="tabpanel" aria-labelledby="platform-windows"><p class="loading">Загружаем данные релиза…</p></div>
  <p id="release-status" class="release-status" role="status"></p>
</section>
${archive}
<section class="container download-help"><div><h3>Установка приложения</h3><p>Требования к системе и инструкции для Windows, Linux и Astra Linux.</p>${textLink('/docs/#installation', 'Инструкция по установке')}</div><div><h3>Проверка скачанного файла</h3><p>Сравните SHA-256 файла с контрольной суммой на странице релиза.</p>${textLink('/docs/#checksums', 'Как проверить SHA-256')}</div></section>`;

export const changelog = `<section class="container page-intro"><h1>История релизов</h1><p>Изменения в версиях Reqon и установочные файлы.</p></section><p id="release-status" class="container release-status" role="status"></p>${archive}`;

export const about = `<section class="container about-intro">
  <div><h1>О Reqon</h1><p>Reqon — настольный API-клиент для разработчиков и тестировщиков. Он объединяет работу с HTTP и gRPC, коллекциями запросов и окружениями.</p><p>Приложение построено на Qt Widgets и PySide6. Коллекции, история и настройки хранятся локально; для начала работы не нужен аккаунт.</p>${textLink('https://github.com/Norwi4/reqon', 'Исходный код на GitHub')}</div>
  <figure><img src="/assets/mascot.png" width="400" height="299" alt="Маскот Reqon — енот в оранжевых очках с ноутбуком"><figcaption>Енот — маскот Reqon.</figcaption></figure>
</section><section class="container about-details"><h2>Платформы и компоненты</h2><dl><div><dt>Платформы</dt><dd>Windows x64, Linux x86-64, Astra Linux 1.8. <a href="/download/">Доступные версии и требования</a>.</dd></div><div><dt>Интерфейс</dt><dd>Нативное приложение на Qt Widgets / PySide6.</dd></div><div><dt>Шрифты</dt><dd>Inter и JetBrains Mono. Лицензии: <a href="/assets/fonts/Inter-OFL.txt">Inter OFL</a>, <a href="/assets/fonts/JetBrainsMono-OFL.txt">JetBrains Mono OFL</a>.</dd></div></dl></section>${cta}`;

export const notFound = `<section class="container error-page"><p>Ошибка 404</p><h1>Страница не найдена</h1><p>Проверьте адрес или перейдите на главную страницу.</p><div class="actions">${button('/', 'На главную')}${button('/download/', 'Скачать Reqon', false)}</div></section>`;
