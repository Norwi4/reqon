"""Local UI translations. Request data, protocol names and server replies stay intact."""
from PySide6.QtCore import QObject,QEvent
from PySide6.QtGui import QAction
from PySide6.QtWidgets import QApplication,QWidget,QLabel,QAbstractButton,QLineEdit,QPlainTextEdit,QTabWidget

RU={
 'Theme selection':'Выбор темы','Sync with system':'Как в системе','Manual':'Вручную','Day Theme':'Дневная тема','Night Theme':'Ночная тема','ACTIVE':'АКТИВНА',
 'Used when your system is in light mode.':'Используется при светлом оформлении системы.',
 'Used when your system is in dark mode.':'Используется при тёмном оформлении системы.',
 'Choose a theme for your workspace. Sync with your system or select your preferred appearance.':'Выберите оформление рабочего пространства. Следуйте настройкам системы или выберите тему вручную.',
 'General':'Основные','Themes':'Темы','Shortcuts':'Горячие клавиши','Data':'Данные','Add-ons':'Дополнения','Certificates':'Сертификаты','Proxy':'Прокси','Update':'Обновление','About':'О программе',
 'Settings':'Настройки','Close settings':'Закрыть настройки','Not available yet':'Пока недоступно','Request':'Запрос','HTTP version':'Версия HTTP',
 'Select the HTTP version to use for sending the request.':'Выберите версию HTTP для отправки запросов.',
 'Request timeout':'Тайм-аут запроса','Maximum request duration. Set to 0 to wait without a timeout.':'Максимальная длительность запроса. Значение 0 отключает тайм-аут.',
 'Max response size':'Максимальный размер ответа','Maximum response size to download and decode. Set to 0 for no limit.':'Максимальный размер загружаемого и распакованного ответа. Значение 0 снимает ограничение.',
 'SSL certificate verification':'Проверка SSL-сертификатов','SSL/TLS key log':'Журнал ключей SSL/TLS',
 'Save TLS session keys locally for debugging encrypted connections.':'Сохранять ключи сеансов TLS локально для отладки зашифрованных соединений.',
 'Export TLS key log':'Экспорт ключей TLS','Disable cookies':'Отключить cookies','Disable the local cookie jar for all requests.':'Отключить локальное хранилище cookies для всех запросов.',
 'Request validation':'Проверка запроса','Validate JSON and linked local JSON Schema / OpenAPI request bodies before sending.':'Проверять JSON и тело запроса по локальной JSON Schema / OpenAPI перед отправкой.',
 'Response format detection':'Определение формата ответа','Auto':'Авто','Working directory':'Рабочая папка',
 'Resolve request body files and local validation schemas relative to this directory.':'Относительные пути к файлам тела запроса и схемам проверки отсчитываются от этой папки.',
 'Location':'Расположение','Choose':'Выбрать','Read files outside working directory':'Читать файлы вне рабочей папки',
 'Allow request body and schema files outside the selected directory.':'Разрешить файлы тела запроса и схемы за пределами выбранной папки.',
 'Headers':'Заголовки','Send no-cache header':'Отправлять заголовок no-cache','Send Postman Token header':'Отправлять заголовок Postman Token',
 'Send a fresh Postman-Token UUID with each HTTP request.':'Добавлять новый UUID в заголовок Postman-Token каждого HTTP-запроса.',
 'Retain headers when clicking on links':'Сохранять заголовки при переходе по ссылкам','Automatically follow redirects':'Автоматически следовать перенаправлениям',
 'User interface':'Интерфейс','Always open sidebar item in new tab':'Всегда открывать элемент в новой вкладке','Always ask when closing unsaved tabs':'Спрашивать при закрытии несохранённых вкладок',
 'Applies to unsaved changes when autosave is off.':'Действует для несохранённых изменений при выключенном автосохранении.',
 'Show icons with tab names':'Показывать иконки вкладок','Two-pane view':'Две панели','Show the request and response side by side.':'Показывать запрос и ответ рядом по горизонтали.',
 'Variable autocomplete':'Автодополнение переменных','Default documentation editor':'Редактор документации по умолчанию','Reqon editor':'Редактор Reqon','Markdown editor':'Редактор Markdown',
 'Editor settings':'Настройки редактора','Reset':'Сбросить','Font Family':'Семейство шрифтов','Comma-separated family names. First available font is used.':'Названия через запятую. Используется первый доступный шрифт.',
 'Font Size (px)':'Размер шрифта (px)','Indentation count':'Размер отступа','Number of spaces or tab width per code level.':'Количество пробелов или ширина табуляции для уровня кода.',
 'Indentation type':'Тип отступа','Space':'Пробелы','Tab':'Табуляция','Auto close brackets':'Автозакрытие скобок','Auto close quotes':'Автозакрытие кавычек',
 'Application':'Приложение','Language':'Язык','Autosave':'Автосохранение','Automatically save changes to requests on this computer.':'Автоматически сохранять изменения запросов на этом компьютере.',
 'Reqon works locally. No cloud accounts or usage telemetry.':'Reqon работает локально. Без облачных аккаунтов и отправки телеметрии.',
 'Saved locally':'Сохранено локально','Saving…':'Сохранение…','Draft saved locally':'Черновик сохранён локально','Unsaved changes':'Несохранённые изменения',
 'Enable TLS key logging and send a TLS request first.':'Включите журнал ключей TLS и отправьте запрос с TLS.','TLS key log exported':'Журнал ключей TLS экспортирован',
 'Home':'Главная','Workspaces':'Рабочие пространства','My Workspace':'Моё рабочее пространство','Local workspace':'Локальное пространство','Collections':'Коллекции','Environments':'Окружения','History':'История',
 'New':'Создать','Import':'Импорт','Export':'Экспорт','Save':'Сохранить','Send':'Отправить','Invoke':'Вызвать','Cancel':'Отмена','Close':'Закрыть','Done':'Готово',
 'Message':'Сообщение','Authorization':'Авторизация','Metadata':'Метаданные','Service definition':'Описание сервиса','Docs':'Документация','Response':'Ответ','Body':'Тело','Params':'Параметры','Trailers':'Трейлеры',
 'Compose message':'Введите сообщение','Enter server URL':'Введите адрес сервера','Enter request URL':'Введите URL запроса','Select a method':'Выберите метод','Use Example Message':'Использовать пример сообщения',
 'Use Server Reflection':'Использовать рефлексию','Import definition':'Импортировать описание','Find in response':'Поиск в ответе','Bulk Edit':'Массовое редактирование',
 'New HTTP Request':'Новый HTTP-запрос','New gRPC Request':'Новый gRPC-запрос','New Collection':'Новая коллекция','Import cURL / gRPCurl':'Импорт cURL / gRPCurl',
 'Import Postman Collection':'Импорт коллекции Postman','Export Collection':'Экспорт коллекции','Export collections…':'Экспорт коллекций…','Open local link':'Открыть локальную ссылку','Open local draft':'Открыть черновик',
 'Code snippet':'Фрагмент кода','Copy as cURL / gRPCurl':'Копировать как cURL / gRPCurl','Quit':'Выход','Send Request':'Отправить запрос','Save Request':'Сохранить запрос',
 'Search collections':'Поиск коллекций','Search Ctrl+K':'Поиск Ctrl+K','No Environment':'Без окружения','Overview':'Обзор','Variables':'Переменные','Summary':'Описание',
 'No collections':'Нет коллекций','Create a collection to organize your requests.':'Создайте коллекцию для ваших запросов.','Create collection':'Создать коллекцию',
 'Request in progress':'Запрос выполняется','Enter a server URL':'Введите адрес сервера','Sending…':'Отправка…','Copy response':'Копировать ответ','Copy error details':'Копировать ошибку',
 'Override General settings':'Переопределить основные настройки','Enable server certificate verification':'Проверять сертификат сервера','Request timeout':'Тайм-аут запроса',
 'Validation schema':'Схема проверки','Optional local JSON Schema or OpenAPI JSON file':'Локальный JSON-файл JSON Schema или OpenAPI (необязательно)',
 'CA certificate':'Сертификат CA','Client certificate':'Сертификат клиента','Client private key':'Закрытый ключ клиента','Request body file':'Файл тела запроса','Choose file':'Выбрать файл',
 'Path relative to the working directory':'Путь относительно рабочей папки','Open link in new request':'Открыть ссылку в новом запросе',
 'Save changes before closing this tab?':'Сохранить изменения перед закрытием вкладки?','Discard':'Не сохранять',
 'Default proxy configuration':'Прокси по умолчанию','Reqon uses your system’s proxy configuration by default to send API requests.':'По умолчанию Reqon использует системные настройки прокси для API-запросов.',
 'This proxy requires authentication':'Этот прокси требует авторизации','Proxy configurations for sending requests':'Настройки прокси для отправки запросов',
 'Specify the proxy used to send HTTP and gRPC requests. Custom settings take priority over system settings.':'Прокси для HTTP- и gRPC-запросов. Пользовательские настройки имеют приоритет над системными.',
 'Use system proxy':'Использовать системный прокси','Respect HTTP_PROXY, HTTPS_PROXY, and NO_PROXY environment variables':'Учитывать переменные HTTP_PROXY, HTTPS_PROXY и NO_PROXY',
 'Use custom proxy configuration':'Использовать пользовательский прокси','Use proxy for':'Использовать прокси для','Choose which request types use the proxy.':'Выберите типы запросов для прокси.',
 'Proxy server':'Сервер прокси','Proxy auth':'Авторизация прокси','Authenticate with the proxy server.':'Авторизация на сервере прокси.',
 'Username':'Имя пользователя','Password':'Пароль','Proxy bypass':'Исключения прокси','Comma-separated hosts that connect directly.':'Адреса прямого подключения через запятую.',
 'SOCKS4 uses the username only; passwords are not part of that protocol.':'SOCKS4 использует только имя пользователя; пароли этим протоколом не поддерживаются.',
 'Saved locally · Applies to new requests and reflection calls.':'Сохранено локально · Применяется к новым запросам и рефлексии.',
 'Auth Type':'Тип авторизации','Value':'Значение','Key':'Ключ','Description':'Описание','Field':'Поле','Type':'Тип',
 'Add HTTP request':'Добавить HTTP-запрос','Add gRPC request':'Добавить gRPC-запрос','Add folder':'Добавить папку','Add collection description':'Добавьте описание коллекции',
 'Bold':'Полужирный','Italic':'Курсив','Bullet list':'Маркированный список',
 'Ready to send':'Готово к отправке','Send a request to inspect the response.':'Отправьте запрос, чтобы увидеть ответ.',
 'Create a request':'Создайте запрос','Create a request to start working with your API.':'Создайте запрос для работы с вашим API.',
 'No activity':'Нет активности','Run a request to see activity here.':'Выполните запрос, чтобы увидеть историю.',
 'No response body':'Ответ без тела','The request completed without a response body.':'Запрос выполнен, тело ответа отсутствует.',
 'No results':'Ничего не найдено','Try another search query.':'Попробуйте изменить поисковый запрос.',
 'Search  Ctrl+K':'Поиск  Ctrl+K',
}


class LanguageFilter(QObject):
    def __init__(self):super().__init__();self.language='English';self.busy=False
    def field(self,obj,getter,setter,key):
        value=getter();history=getattr(obj,'_reqon_translations',{})
        original,last=history.get(key,(value,value))
        if value!=last:original=value
        translated=RU.get(original,original) if self.language=='Русский' else original
        if translated!=value:setter(translated)
        history[key]=(original,translated);obj._reqon_translations=history
    def translate(self,obj):
        if isinstance(obj,(QLabel,QAbstractButton,QAction)):self.field(obj,obj.text,obj.setText,'text')
        if isinstance(obj,(QLineEdit,QPlainTextEdit)):self.field(obj,obj.placeholderText,obj.setPlaceholderText,'placeholder')
        if isinstance(obj,QWidget):self.field(obj,obj.toolTip,obj.setToolTip,'tooltip')
        if isinstance(obj,QTabWidget) and obj.objectName()!='workbench':
            for i in range(obj.count()):self.field(obj,lambda i=i:obj.tabText(i),lambda text,i=i:obj.setTabText(i,text),'tab'+str(i))
    def eventFilter(self,obj,event):
        if not self.busy and event.type() in (QEvent.Type.Show,QEvent.Type.Polish,QEvent.Type.Paint,QEvent.Type.ActionAdded):
            self.busy=True
            try:self.translate(obj)
            finally:self.busy=False
        return False


def apply_language(window,language):
    app=QApplication.instance();translator=getattr(app,'reqon_language_filter',None)
    if translator is None:
        translator=LanguageFilter();app.reqon_language_filter=translator;app.installEventFilter(translator)
    translator.language=language;translator.busy=True
    try:
        for obj in [window,*window.findChildren(QObject)]:translator.translate(obj)
    finally:translator.busy=False
