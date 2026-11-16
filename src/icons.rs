// Bruno/Tabler interface assets and the supplied Reqon circular brand mark, rendered in Rust.
use crate::widgets::Icon;
pub fn svg(icon: Icon) -> Option<(&'static str, &'static [u8])> {
    Some(match icon {
        Icon::Wand => (
            "bytes://icons/Wand.svg",
            include_bytes!("../assets/icons/Wand.svg") as &[u8],
        ),
        Icon::MessageRefresh => (
            "bytes://icons/MessageRefresh.svg",
            include_bytes!("../assets/icons/MessageRefresh.svg") as &[u8],
        ),
        Icon::Reqon => (
            "bytes://brand/circular-v1/reqon-circle.svg",
            include_bytes!("../assets/brand/circular-v1/svg/reqon-circle.svg") as &[u8],
        ),
        Icon::Upload => (
            "bytes://icons/Upload.svg",
            include_bytes!("../assets/icons/Upload.svg") as &[u8],
        ),
        Icon::Pin => (
            "bytes://icons/Pin.svg",
            include_bytes!("../assets/icons/Pin.svg") as &[u8],
        ),
        Icon::Pinned => (
            "bytes://icons/Pinned.svg",
            include_bytes!("../assets/icons/Pinned.svg") as &[u8],
        ),
        Icon::Login => (
            "bytes://icons/Login.svg",
            include_bytes!("../assets/icons/Login.svg") as &[u8],
        ),
        Icon::ArrowLeft => (
            "bytes://icons/ArrowLeft.svg",
            include_bytes!("../assets/icons/ArrowLeft.svg") as &[u8],
        ),
        Icon::GitBrand => (
            "bytes://icons/GitBrand.svg",
            include_bytes!("../assets/icons/GitBrand.svg") as &[u8],
        ),
        Icon::Network => (
            "bytes://icons/Network.svg",
            include_bytes!("../assets/icons/Network.svg") as &[u8],
        ),
        Icon::Dashboard => (
            "bytes://icons/Dashboard.svg",
            include_bytes!("../assets/icons/Dashboard.svg") as &[u8],
        ),
        Icon::Bell => (
            "bytes://icons/Bell.svg",
            include_bytes!("../assets/icons/Bell.svg") as &[u8],
        ),
        Icon::Github => (
            "bytes://icons/Github.svg",
            include_bytes!("../assets/icons/Github.svg") as &[u8],
        ),
        Icon::TerminalBox => (
            "bytes://icons/TerminalBox.svg",
            include_bytes!("../assets/icons/TerminalBox.svg") as &[u8],
        ),
        Icon::Terminal => (
            "bytes://icons/Terminal.svg",
            include_bytes!("../assets/icons/Terminal.svg") as &[u8],
        ),
        Icon::GitCommit => (
            "bytes://icons/GitCommit.svg",
            include_bytes!("../assets/icons/GitCommit.svg") as &[u8],
        ),
        Icon::GitFork => (
            "bytes://icons/GitFork.svg",
            include_bytes!("../assets/icons/GitFork.svg") as &[u8],
        ),
        Icon::Undo => (
            "bytes://icons/Undo.svg",
            include_bytes!("../assets/icons/Undo.svg") as &[u8],
        ),
        Icon::GitMerge => (
            "bytes://icons/GitMerge.svg",
            include_bytes!("../assets/icons/GitMerge.svg") as &[u8],
        ),
        Icon::Menu => (
            "bytes://icons/Menu.svg",
            include_bytes!("../assets/icons/Menu.svg") as &[u8],
        ),
        Icon::Home => (
            "bytes://icons/Home.svg",
            include_bytes!("../assets/icons/Home.svg") as &[u8],
        ),
        Icon::Panel => (
            "bytes://icons/Panel.svg",
            include_bytes!("../assets/icons/Panel.svg") as &[u8],
        ),
        Icon::BottomPanel => (
            "bytes://icons/BottomPanel.svg",
            include_bytes!("../assets/icons/BottomPanel.svg") as &[u8],
        ),
        Icon::Split => (
            "bytes://icons/Split.svg",
            include_bytes!("../assets/icons/Split.svg") as &[u8],
        ),
        Icon::SplitRows => (
            "bytes://icons/SplitRows.svg",
            include_bytes!("../assets/icons/SplitRows.svg") as &[u8],
        ),
        Icon::Minimize => (
            "bytes://icons/Minimize.svg",
            include_bytes!("../assets/icons/Minimize.svg") as &[u8],
        ),
        Icon::Maximize => (
            "bytes://icons/Maximize.svg",
            include_bytes!("../assets/icons/Maximize.svg") as &[u8],
        ),
        Icon::Close => (
            "bytes://icons/Close.svg",
            include_bytes!("../assets/icons/Close.svg") as &[u8],
        ),
        Icon::TabClose => (
            "bytes://icons/TabClose.svg",
            include_bytes!("../assets/icons/TabClose.svg") as &[u8],
        ),
        Icon::Search => (
            "bytes://icons/Search.svg",
            include_bytes!("../assets/icons/Search.svg") as &[u8],
        ),
        Icon::Plus => (
            "bytes://icons/Plus.svg",
            include_bytes!("../assets/icons/Plus.svg") as &[u8],
        ),
        Icon::DotsVertical => (
            "bytes://icons/DotsVertical.svg",
            include_bytes!("../assets/icons/DotsVertical.svg") as &[u8],
        ),
        Icon::Dots => (
            "bytes://icons/Dots.svg",
            include_bytes!("../assets/icons/Dots.svg") as &[u8],
        ),
        Icon::Run => (
            "bytes://icons/Run.svg",
            include_bytes!("../assets/icons/Run.svg") as &[u8],
        ),
        Icon::Shield => (
            "bytes://icons/Shield.svg",
            include_bytes!("../assets/icons/Shield.svg") as &[u8],
        ),
        Icon::Send => (
            "bytes://icons/Send.svg",
            include_bytes!("../assets/icons/Send.svg") as &[u8],
        ),
        Icon::Settings => (
            "bytes://icons/Settings.svg",
            include_bytes!("../assets/icons/Settings.svg") as &[u8],
        ),
        Icon::Key => (
            "bytes://icons/Key.svg",
            include_bytes!("../assets/icons/Key.svg") as &[u8],
        ),
        Icon::Palette => (
            "bytes://icons/Palette.svg",
            include_bytes!("../assets/icons/Palette.svg") as &[u8],
        ),
        Icon::Cookie => (
            "bytes://icons/Cookie.svg",
            include_bytes!("../assets/icons/Cookie.svg") as &[u8],
        ),
        Icon::Tool => (
            "bytes://icons/Tool.svg",
            include_bytes!("../assets/icons/Tool.svg") as &[u8],
        ),
        Icon::Box => (
            "bytes://icons/Box.svg",
            include_bytes!("../assets/icons/Box.svg") as &[u8],
        ),
        Icon::Down => (
            "bytes://icons/Down.svg",
            include_bytes!("../assets/icons/Down.svg") as &[u8],
        ),
        Icon::Chevron => (
            "bytes://icons/Chevron.svg",
            include_bytes!("../assets/icons/Chevron.svg") as &[u8],
        ),
        Icon::File => (
            "bytes://icons/File.svg",
            include_bytes!("../assets/icons/File.svg") as &[u8],
        ),
        Icon::Code => (
            "bytes://icons/Code.svg",
            include_bytes!("../assets/icons/Code.svg") as &[u8],
        ),
        Icon::Save => (
            "bytes://icons/Save.svg",
            include_bytes!("../assets/icons/Save.svg") as &[u8],
        ),
        Icon::Help => (
            "bytes://icons/Help.svg",
            include_bytes!("../assets/icons/Help.svg") as &[u8],
        ),
        Icon::Flask => (
            "bytes://icons/Flask.svg",
            include_bytes!("../assets/icons/Flask.svg") as &[u8],
        ),
        Icon::Workspace => (
            "bytes://icons/Workspace.svg",
            include_bytes!("../assets/icons/Workspace.svg") as &[u8],
        ),
        Icon::Browser => (
            "bytes://icons/Browser.svg",
            include_bytes!("../assets/icons/Browser.svg") as &[u8],
        ),
        Icon::UserCircle => (
            "bytes://icons/UserCircle.svg",
            include_bytes!("../assets/icons/UserCircle.svg") as &[u8],
        ),
        Icon::Certificate => (
            "bytes://icons/Certificate.svg",
            include_bytes!("../assets/icons/Certificate.svg") as &[u8],
        ),
        Icon::Star => (
            "bytes://icons/Star.svg",
            include_bytes!("../assets/icons/Star.svg") as &[u8],
        ),
        Icon::Lock => (
            "bytes://icons/Lock.svg",
            include_bytes!("../assets/icons/Lock.svg") as &[u8],
        ),
        Icon::GitBranch => (
            "bytes://icons/GitBranch.svg",
            include_bytes!("../assets/icons/GitBranch.svg") as &[u8],
        ),
        Icon::Keyboard => (
            "bytes://icons/Keyboard.svg",
            include_bytes!("../assets/icons/Keyboard.svg") as &[u8],
        ),
        Icon::Sparkles => (
            "bytes://icons/Sparkles.svg",
            include_bytes!("../assets/icons/Sparkles.svg") as &[u8],
        ),
        Icon::Database => (
            "bytes://icons/Database.svg",
            include_bytes!("../assets/icons/Database.svg") as &[u8],
        ),
        Icon::Support => (
            "bytes://icons/Support.svg",
            include_bytes!("../assets/icons/Support.svg") as &[u8],
        ),
        Icon::Beta => (
            "bytes://icons/Beta.svg",
            include_bytes!("../assets/icons/Beta.svg") as &[u8],
        ),
        Icon::Info => (
            "bytes://icons/Info.svg",
            include_bytes!("../assets/icons/Info.svg") as &[u8],
        ),
        Icon::CookieOff => (
            "bytes://icons/CookieOff.svg",
            include_bytes!("../assets/icons/CookieOff.svg") as &[u8],
        ),
        Icon::Book => (
            "bytes://icons/Book.svg",
            include_bytes!("../assets/icons/Book.svg") as &[u8],
        ),
        Icon::Speakerphone => (
            "bytes://icons/Speakerphone.svg",
            include_bytes!("../assets/icons/Speakerphone.svg") as &[u8],
        ),
        Icon::Discord => (
            "bytes://icons/Discord.svg",
            include_bytes!("../assets/icons/Discord.svg") as &[u8],
        ),
        Icon::Twitter => (
            "bytes://icons/Twitter.svg",
            include_bytes!("../assets/icons/Twitter.svg") as &[u8],
        ),
        Icon::Linkedin => (
            "bytes://icons/Linkedin.svg",
            include_bytes!("../assets/icons/Linkedin.svg") as &[u8],
        ),
        Icon::Eraser => (
            "bytes://icons/Eraser.svg",
            include_bytes!("../assets/icons/Eraser.svg") as &[u8],
        ),
        Icon::BrightnessUp => (
            "bytes://icons/BrightnessUp.svg",
            include_bytes!("../assets/icons/BrightnessUp.svg") as &[u8],
        ),
        Icon::ThemeMoon => (
            "bytes://icons/ThemeMoon.svg",
            include_bytes!("../assets/icons/ThemeMoon.svg") as &[u8],
        ),
        Icon::DeviceDesktop => (
            "bytes://icons/DeviceDesktop.svg",
            include_bytes!("../assets/icons/DeviceDesktop.svg") as &[u8],
        ),
        _ => return None,
    })
}
