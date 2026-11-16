# Reqon для Linux x86-64

Нативный клиент для 64-разрядного Linux. Базовая среда сборки — Debian 12 (glibc 2.36). Поддерживаются X11 и Wayland; для интерфейса нужен драйвер Vulkan или OpenGL/EGL.

## Пакет DEB

Для Debian 12+ / Ubuntu 24.04+:

```bash
sudo apt install ./reqon_0.21.1_amd64.deb
reqon
reqon-cli --version
```

Приложение также появляется в меню рабочего стола. APT устанавливает зависимости. Обновление: установите новый DEB той же командой. Удаление: `sudo apt remove reqon`; личные коллекции сохраняются.

## Архив TAR.GZ

```bash
tar -xzf Reqon-0.21.1-linux-x86_64.tar.gz
./Reqon/reqon
./Reqon/reqon-cli --version
```

На минимальной Debian/Ubuntu-системе установите библиотеки интерфейса:

```bash
sudo apt install libsecret-tools gnome-keyring ca-certificates libxkbcommon0 libxkbcommon-x11-0 libx11-6 libx11-xcb1 libxcursor1 libxi6 libxrandr2 libxinerama1 libxcb1 libxcb-render0 libxcb-shape0 libxcb-xfixes0 libwayland-client0 libwayland-cursor0 libwayland-egl1 libegl1 libgl1 libvulkan1 mesa-vulkan-drivers
```

Для Git-интеграции нужен `git`, для системного выбора файлов — `xdg-desktop-portal` с backend вашего рабочего стола (например, `xdg-desktop-portal-gtk`).

Профиль: `$XDG_DATA_HOME/Reqon/native`, по умолчанию `~/.local/share/Reqon/native`. Собственную папку можно выбрать через `--data-dir /path/to/profile`. Файл `portable.flag` рядом с бинарником включает профиль `data` в этой же папке. Не запускайте интерфейс через `sudo`.

Оба комплекта содержат GUI `reqon` и CLI `reqon-cli` версии 0.21.1. В архиве находятся те же бинарники, что и в DEB. CLI собран без графических зависимостей. Руководство `cli-runner.md` находится рядом с бинарниками в архиве и в `/usr/share/doc/reqon/` после установки DEB. Python, Qt и Rust для запуска не требуются. Пользовательские запросы и профили в дистрибутив не входят.

Контрольные суммы: `Reqon-0.21.1-linux-SHA256SUMS.txt`.

## Сборка из исходников

В Windows с Docker Desktop:

```powershell
./scripts/build-linux.ps1
```

В Linux с установленными зависимостями из `packaging/Dockerfile.linux`:

```bash
cargo build --release --locked --bin reqon
cargo build --release --locked --no-default-features --bin reqon-cli
bash scripts/package-linux.sh
```

Секреты, cookies и Auth защищены AES-256-GCM; ключ хранится в системном Secret Service (GNOME Keyring или совместимом KWallet). Нужны `libsecret-tools` и разблокированное системное хранилище ключей. Ключ не сохраняется в открытом виде рядом с профилем.
