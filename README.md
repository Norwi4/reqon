<img src="Reqon_Icon_Pack_Approved/png/transparent/reqon-96.png" alt="Логотип Reqon" width="72" height="72">

# Reqon

Настольный API-клиент для HTTP и gRPC. Отправляйте запросы, изучайте ответы и сохраняйте рабочие сценарии в коллекциях. Коллекции, окружения, история и настройки хранятся на вашем компьютере; для работы не нужен аккаунт.

Интерфейс построен на Qt Widgets и PySide6. Поддерживаются Windows x64 и Linux x86-64.

[Сайт](https://reqon.ru/) · [Скачать](https://github.com/Norwi4/reqon/releases) · [Документация](https://reqon.ru/docs/) · [Сообщить об ошибке](https://github.com/Norwi4/reqon/issues)

![Reqon — HTTP-запрос и просмотр JSON-ответа](website/public/assets/screenshots/http-current.png)

## Возможности

- **HTTP:** методы, query-параметры, заголовки, Bearer / Basic / API Key, JSON, текст и формы, просмотр тела и заголовков ответа.
- **gRPC:** поиск сервисов через reflection, импорт `.proto` и descriptor set, unary и streaming RPC, metadata, trailers и примеры сообщений по protobuf-контракту.
- **Коллекции и окружения:** папки, переменные `{{name}}`, история запросов, сохранённые примеры ответов, импорт и экспорт коллекций. HTTP-коллекции совместимы с Postman v2.1; для смешанных HTTP/gRPC-коллекций используется формат Reqon.
- **TLS:** настройки проверки сертификатов, собственный CA, клиентские сертификаты и переопределение имени сервера для запроса.

В Windows 0.13.8 также доступны HTTP/2, локальные cookies, Proxy, импорт cURL/gRPCurl, проверка тела запроса по локальной JSON Schema и раздел General с настройками редактора, языка и автосохранения. Набор возможностей зависит от версии сборки; подробности — в [истории релизов](https://github.com/Norwi4/reqon/releases).

## Скачать

| Платформа | Версия | Установочный файл |
| --- | --- | --- |
| Windows x64 | **0.13.8** | [Скачать EXE](https://github.com/Norwi4/reqon/releases/download/v0.13.8/Reqon-Setup-0.13.8-win64.exe) |
| Linux x86-64, glibc ≥ 2.36 | **0.13.4** | [Скачать TAR.GZ](https://github.com/Norwi4/reqon/releases/download/v0.13.4/Reqon-0.13.4-linux-x86_64.tar.gz) |
| Astra Linux 1.8, amd64 | **0.13.2** | [Скачать DEB](https://github.com/Norwi4/reqon/releases/download/v0.13.2/getman_0.13.2_astra1.8_amd64.deb) |

Все версии доступны в [GitHub Releases](https://github.com/Norwi4/reqon/releases) и в [архиве на сайте](https://reqon.ru/download/). Для установки выбирайте EXE, TAR.GZ или DEB из раздела **Assets**. Python в готовых сборках уже включён.

На странице каждого релиза есть файл `SHA256SUMS.txt` для проверки скачанных файлов. Пример проверки Windows-установщика:

```powershell
Get-FileHash .\Reqon-Setup-0.13.8-win64.exe -Algorithm SHA256
```

### Windows

Запустите установщик и выберите папку установки. Приложение появится в меню «Пуск»; в мастере можно добавить ярлык на рабочий стол.

Начиная с версии 0.13.3, данные хранятся в `%APPDATA%\Reqon\reqon.sqlite3`. При первом запуске база предыдущего приложения переносится автоматически, а исходная копия сохраняется для восстановления. Обновление приложения не заменяет существующую базу Reqon.

### Linux — переносимая сборка

Распакуйте архив в отдельную папку и запустите приложение:

```sh
tar -xzf Reqon-0.13.4-linux-x86_64.tar.gz
./Reqon/reqon
```

Нужны glibc ≥ 2.36 и графическая среда. Сохраняйте каталог `_internal` рядом с исполняемым файлом; при обновлении распаковывайте архив в новую папку. Данные находятся в `$XDG_DATA_HOME/Reqon` или `~/.local/share/Reqon`.

Сборка подготовлена для ALT Linux 11 (p11). В 0.13.4 обновлены библиотеки Qt/PySide6/Shiboken; запуск на целевой Linux-системе пока не подтверждён. [Подробности выпуска](docs/linux-0.13.4.md).

### Astra Linux 1.8

```sh
sudo apt install ./getman_0.13.2_astra1.8_amd64.deb
```

Запускайте приложение из меню или командой `getman`: имя пакета и команды в этой архивной сборке сохранено. Пакет 0.13.2 содержит прежний комплект Qt; при ошибке `none_dealloc` используйте переносимую сборку 0.13.4.

## Запуск из исходников

```sh
git clone https://github.com/Norwi4/reqon.git
cd reqon
```

Windows / PowerShell:

```powershell
python -m venv .venv
.venv\Scripts\Activate.ps1
python -m pip install -r requirements.txt
python main.py
```

Linux:

```sh
python3 -m venv .venv
. .venv/bin/activate
python -m pip install -r requirements.txt
python main.py
```

Чтобы использовать отдельный рабочий каталог, передайте `--data-dir`:

```sh
python main.py --data-dir ./workspace
```

## Сборка и проверки

Windows: `powershell -File packaging/build-windows.ps1`. Скрипт создаёт приложение, ZIP-архив и EXE-установщик в `dist/`. Для установщика нужен Inno Setup с компилятором `ISCC.exe`.

Для сборки DEB используйте Linux amd64 и [Dockerfile](packaging/Dockerfile.astra18), либо выполните `bash packaging/build-deb.sh` в подготовленной Astra Linux 1.8. Сценарии упаковки находятся в [packaging/](packaging/).

Проверки приложения используют временные базы и локальные HTTP/gRPC-серверы:

```sh
python -X faulthandler -m unittest discover -s tests -p "test_*.py" -v
```

Изменения Windows 0.13.8 описаны в [заметках к выпуску](docs/windows-0.13.8.md).

## Сайт

Исходники сайта находятся в [website/](website/). Для запуска нужен Node.js 22 или новее:

```sh
node website/src/build.mjs
node website/server.mjs
```

Сайт откроется по адресу <http://127.0.0.1:4173/>. Установка npm-зависимостей не требуется. Он показывает последние стабильные сборки для каждой платформы и архив версий; скачивание начинается по прямой ссылке на файл GitHub Releases.

[Запуск, проверки и развёртывание сайта на VDS](website/README.md).
