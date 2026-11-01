import { button } from './components.mjs';

export const docs = `
<section class="container page-intro"><h1>Документация</h1><p>Установка приложения и основные сценарии работы.</p></section>
<div class="container docs-layout">
  <aside class="docs-nav" aria-label="Разделы документации">
    <a href="#installation">Установка</a><a href="#first-request">Первый HTTP-запрос</a><a href="#grpc">Работа с gRPC</a><a href="#collections">Коллекции и окружения</a><a href="#checksums">Проверка SHA-256</a><a href="#updates">Обновление</a>
  </aside>
  <div class="docs-content">
    <section id="installation"><h2>Установка</h2>
      <h3>Windows x64</h3>
      <ol><li><a href="/download/">Скачайте установщик</a> для Windows.</li><li>Запустите EXE и выберите папку установки.</li><li>Откройте Reqon из меню «Пуск» или с рабочего стола.</li></ol>
      <p>Python отдельно устанавливать не нужно. Данные приложения находятся в <code>%APPDATA%/Reqon</code>.</p>
      <h3>Linux x86-64</h3>
      <p>Переносимому архиву нужна glibc ≥ 2.36 и графическая среда. Распакуйте его в новую папку и запустите приложение:</p>
      <pre><code>tar -xzf Reqon-&lt;версия&gt;-linux-x86_64.tar.gz
./Reqon/reqon</code></pre>
      <p>Сохраните папку <code>_internal</code> рядом с исполняемым файлом. Данные находятся в <code>~/.local/share/Reqon</code> либо <code>$XDG_DATA_HOME/Reqon</code>.</p>
      <div class="callout">Сборка 0.13.4 подготовлена для ALT Linux 11. Запуск на этой системе пока не подтверждён тестами; подробности есть в описании релиза.</div>
      <h3>Astra Linux 1.8</h3>
      <p>Скачайте DEB-пакет и установите его через APT, указав имя скачанного файла:</p>
      <pre><code>sudo apt install ./&lt;имя-файла&gt;.deb</code></pre>
      <p>В пакете 0.13.2 сохранён прежний комплект Qt. Если возникает ошибка <code>none_dealloc</code>, используйте переносимую Linux-сборку 0.13.4 при совместимой glibc.</p>
    </section>
    <section id="first-request"><h2>Первый HTTP-запрос</h2>
      <ol><li>Создайте HTTP-запрос через <strong>New</strong>.</li><li>Выберите метод <strong>GET</strong> и укажите адрес своего API.</li><li>При необходимости добавьте параметры, заголовки и авторизацию.</li><li>Нажмите <strong>Send</strong> и изучите тело, статус и заголовки ответа.</li><li>Сохраните запрос в коллекцию, чтобы вернуться к нему позже.</li></ol>
      <p>Для адреса сервиса можно использовать переменную, например <code>{{base_url}}/health</code>.</p>
    </section>
    <section id="grpc"><h2>Работа с gRPC</h2>
      <p>Создайте gRPC-запрос и введите адрес сервера. Если сервер поддерживает reflection, Reqon загрузит список сервисов и методов. Иначе импортируйте файл <code>.proto</code> или descriptor set.</p>
      <ol><li>Выберите сервис и метод.</li><li>Заполните JSON-сообщение или нажмите <strong>Use Example Message</strong>.</li><li>Укажите metadata и параметры TLS, если они требуются серверу.</li><li>Нажмите <strong>Invoke</strong> и изучите ответ, Metadata и Trailers.</li></ol>
      <p>Для client-streaming передайте JSON-массив сообщений. Настройки сертификатов доступны через замочек у адреса и вкладку Settings.</p>
    </section>
    <section id="collections"><h2>Коллекции и окружения</h2>
      <p>Создавайте коллекции и папки для связанных запросов. В разделе Environments добавьте переменные и выберите окружение перед отправкой.</p>
      <p>HTTP-коллекции импортируются и экспортируются в формате Postman v2.1. Для gRPC и смешанных коллекций используйте <code>.reqon_collection.json</code>: в него включается descriptor set.</p>
      <p>Коллекции и история хранятся на устройстве. Чтобы перенести данные, экспортируйте коллекцию и импортируйте файл на другом компьютере. Перед передачей файла проверьте его содержимое: заголовки и переменные могут содержать ключи доступа.</p>
    </section>
    <section id="checksums"><h2>Проверка SHA-256</h2>
      <p>На странице загрузки откройте «SHA-256» и сравните значение с результатом команды для скачанного файла.</p>
      <h3>Windows / PowerShell</h3>
      <pre><code>Get-FileHash .\\Reqon-Setup-&lt;версия&gt;-win64.exe -Algorithm SHA256</code></pre>
      <h3>Linux</h3><pre><code>sha256sum Reqon-&lt;версия&gt;-linux-x86_64.tar.gz</code></pre>
      <p>Все 64 символа должны совпасть. Если они отличаются, скачайте файл заново.</p>
    </section>
    <section id="updates"><h2>Обновление</h2>
      <p>Закройте приложение перед установкой новой версии. Для Windows запустите новый установщик. В Linux распакуйте новую сборку в отдельную папку: не смешивайте файлы разных версий.</p>
      <p>Рабочие данные хранятся отдельно от приложения. Перед переходом на более старую версию сохраните резервную копию данных: совместимость новых данных со старыми версиями не гарантируется.</p>
      ${button('/download/', 'Выбрать версию', true, 'down')}
    </section>
  </div>
</div>`;
